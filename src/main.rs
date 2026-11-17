// KeyMacroBlocker —— 拦掉键盘固件宏键弹出的"运行"框，并可吞掉指定虚拟键
// 托盘常驻 + 开机自启，纯 std + 直接 bind user32，无第三方 crate
#![windows_subsystem = "windows"]

use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

type Bool = i32;
type Hwnd = isize;
type Hhook = isize;
type Hmodule = isize;
type Wparam = usize;
type Lparam = isize;
type Hmenu = isize;
type Hkey = isize;

const WH_KEYBOARD_LL: i32 = 13;
const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_SYSKEYDOWN: u32 = 0x0104;
const WM_SYSKEYUP: u32 = 0x0105;
const WM_GETTEXT: u32 = 0x000D;
const WM_CLOSE: u32 = 0x0010;
const WM_DESTROY: u32 = 0x0002;
const WM_COMMAND: u32 = 0x0111;
const WM_NULL: u32 = 0x0000;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_LBUTTONDBLCLK: u32 = 0x0203;
const LLKHF_INJECTED: u32 = 0x10;
const LLKHF_UP: u32 = 0x80;
const ERROR_ALREADY_EXISTS: u32 = 183;

const WM_TRAY: u32 = 0x8000 + 1;

const NIM_ADD: u32 = 0;
const NIM_DELETE: u32 = 2;
const NIF_MESSAGE: u32 = 1;
const NIF_ICON: u32 = 2;
const NIF_TIP: u32 = 4;

const MF_STRING: u32 = 0x0000;
const MF_SEPARATOR: u32 = 0x0800;
const MF_CHECKED: u32 = 0x0008;
const TPM_RIGHTBUTTON: u32 = 0x0002;
const TPM_RETURNCMD: u32 = 0x0100;

const ID_PAUSE: u32 = 1;
const ID_AUTOSTART: u32 = 2;
const ID_OPENLOG: u32 = 3;
const ID_EXIT: u32 = 4;

const HKEY_CURRENT_USER: Hkey = 0x80000001u32 as i32 as Hkey;
const KEY_QUERY_VALUE: u32 = 0x0001;
const KEY_SET_VALUE: u32 = 0x0002;
const REG_SZ: u32 = 1;
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE: &str = "KeyMacroBlocker";

#[repr(C)]
struct KbdLlHookStruct {
    vk_code: u32,
    scan_code: u32,
    flags: u32,
    time: u32,
    dw_extra_info: usize,
}

#[repr(C)]
struct Msg {
    hwnd: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
    time: u32,
    pt_x: i32,
    pt_y: i32,
    lprivate: u32,
}

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
struct WndClassW {
    style: u32,
    lpfn_wnd_proc: Option<unsafe extern "system" fn(Hwnd, u32, Wparam, Lparam) -> Lparam>,
    cb_cls_extra: i32,
    cb_wnd_extra: i32,
    h_instance: Hmodule,
    h_icon: isize,
    h_cursor: isize,
    hbr_background: isize,
    lpsz_menu_name: *const u16,
    lpsz_class_name: *const u16,
}

#[repr(C)]
struct NotifyIconDataW {
    cb_size: u32,
    h_wnd: Hwnd,
    u_id: u32,
    u_flags: u32,
    u_callback_message: u32,
    h_icon: isize,
    sz_tip: [u16; 128],
    dw_state: u32,
    dw_state_mask: u32,
    sz_info: [u16; 256],
    u_timeout_or_version: u32,
    sz_info_title: [u16; 64],
    dw_info_flags: u32,
    guid_item: [u8; 16],
    h_balloon_icon: isize,
}

#[repr(C)]
struct LocalTime {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    millis: u16,
}

#[link(name = "user32")]
extern "system" {
    fn SetWindowsHookExW(
        id: i32,
        lpfn: Option<unsafe extern "system" fn(i32, Wparam, Lparam) -> Lparam>,
        hmod: Hmodule,
        tid: u32,
    ) -> Hhook;
    fn CallNextHookEx(hhk: Hhook, code: i32, wparam: Wparam, lparam: Lparam) -> Lparam;
    fn UnhookWindowsHookEx(hhk: Hhook) -> Bool;
    fn GetMessageW(msg: *mut Msg, hwnd: Hwnd, min: u32, max: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> Bool;
    fn DispatchMessageW(msg: *const Msg) -> Lparam;
    fn PostQuitMessage(code: i32);
    fn PostMessageW(hwnd: Hwnd, msg: u32, wparam: Wparam, lparam: Lparam) -> Bool;
    fn EnumWindows(cb: Option<unsafe extern "system" fn(Hwnd, Lparam) -> Bool>, lparam: Lparam) -> Bool;
    fn EnumChildWindows(
        parent: Hwnd,
        cb: Option<unsafe extern "system" fn(Hwnd, Lparam) -> Bool>,
        lparam: Lparam,
    ) -> Bool;
    fn GetWindowTextW(hwnd: Hwnd, buf: *mut u16, max: i32) -> i32;
    fn GetClassNameW(hwnd: Hwnd, buf: *mut u16, max: i32) -> i32;
    fn IsWindowVisible(hwnd: Hwnd) -> Bool;
    fn SendMessageW(hwnd: Hwnd, msg: u32, wparam: Wparam, lparam: Lparam) -> Lparam;
    fn RegisterClassW(class: *const WndClassW) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: Hwnd,
        menu: Hmenu,
        instance: Hmodule,
        param: *const c_void,
    ) -> Hwnd;
    fn DefWindowProcW(hwnd: Hwnd, msg: u32, wparam: Wparam, lparam: Lparam) -> Lparam;
    fn DestroyWindow(hwnd: Hwnd) -> Bool;
    fn LoadIconW(instance: Hmodule, name: *const u16) -> isize;
    fn LoadCursorW(instance: Hmodule, name: *const u16) -> isize;
    fn GetCursorPos(p: *mut Point) -> Bool;
    fn SetForegroundWindow(hwnd: Hwnd) -> Bool;
    fn CreatePopupMenu() -> Hmenu;
    fn AppendMenuW(menu: Hmenu, flags: u32, id: usize, text: *const u16) -> Bool;
    fn TrackPopupMenu(
        menu: Hmenu,
        flags: u32,
        x: i32,
        y: i32,
        reserved: i32,
        hwnd: Hwnd,
        rect: *const c_void,
    ) -> Bool;
    fn DestroyMenu(menu: Hmenu) -> Bool;
    fn CreateIconFromResourceEx(
        bits: *const u8,
        size: u32,
        is_icon: Bool,
        ver: u32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> isize;
    fn GetSystemMetrics(index: i32) -> i32;
}

#[link(name = "shell32")]
extern "system" {
    fn Shell_NotifyIconW(msg: u32, data: *const NotifyIconDataW) -> Bool;
    fn ShellExecuteW(
        hwnd: Hwnd,
        op: *const u16,
        file: *const u16,
        params: *const u16,
        dir: *const u16,
        show: i32,
    ) -> Hwnd;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Hmodule;
    fn CreateMutexW(attr: *const c_void, owner: Bool, name: *const u16) -> isize;
    fn GetLastError() -> u32;
    fn GetLocalTime(st: *mut LocalTime);
}

#[link(name = "advapi32")]
extern "system" {
    fn RegCreateKeyExW(
        hkey: Hkey,
        sub_key: *const u16,
        reserved: u32,
        class: *const u16,
        options: u32,
        sam: u32,
        sa: *const c_void,
        result: *mut Hkey,
        disposition: *mut u32,
    ) -> i32;
    fn RegSetValueExW(
        hkey: Hkey,
        name: *const u16,
        reserved: u32,
        typ: u32,
        data: *const u8,
        cb: u32,
    ) -> i32;
    fn RegQueryValueExW(
        hkey: Hkey,
        name: *const u16,
        reserved: *const u32,
        typ: *mut u32,
        data: *mut u8,
        cb: *mut u32,
    ) -> i32;
    fn RegDeleteValueW(hkey: Hkey, name: *const u16) -> i32;
    fn RegCloseKey(hkey: Hkey) -> i32;
}

static SUPPRESS_UNTIL: AtomicI64 = AtomicI64::new(0);
static SUPPRESS_START: AtomicI64 = AtomicI64::new(0);
static DEBUG_INJECTED: AtomicBool = AtomicBool::new(false);
static PAUSED: AtomicBool = AtomicBool::new(false);
static PENDING_SNIP: AtomicBool = AtomicBool::new(false);
static TRACE_VK: AtomicI64 = AtomicI64::new(-1);
static PATTERNS: OnceLock<Vec<String>> = OnceLock::new();
static BLOCKED_VKS: OnceLock<Vec<(u32, bool)>> = OnceLock::new(); // (虚拟键码, 是否顺带截图)
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static EXE_PATH: OnceLock<String> = OnceLock::new();

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn timestamp() -> String {
    let mut t = LocalTime {
        year: 0,
        month: 0,
        day_of_week: 0,
        day: 0,
        hour: 0,
        minute: 0,
        second: 0,
        millis: 0,
    };
    unsafe { GetLocalTime(&mut t) };
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        t.hour, t.minute, t.second, t.millis
    )
}

fn log_stamped(line: &str) {
    if let Some(p) = LOG_PATH.get() {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{} {}", timestamp(), line);
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_fixed(s: &str, cap: usize) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().take(cap - 1).collect();
    v.push(0);
    v.resize(cap, 0);
    v
}

// ---------- 运行框拦截 ----------

unsafe fn window_text(hwnd: Hwnd) -> String {
    let mut buf = [0u16; 256];
    let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    let n = n.clamp(0, buf.len() as i32) as usize;
    String::from_utf16_lossy(&buf[..n])
}

unsafe fn class_name(hwnd: Hwnd) -> String {
    let mut buf = [0u16; 128];
    let n = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    let n = n.clamp(0, buf.len() as i32) as usize;
    String::from_utf16_lossy(&buf[..n])
}

unsafe extern "system" fn find_run_cb(hwnd: Hwnd, lparam: Lparam) -> Bool {
    let out = &mut *(lparam as *mut Hwnd);
    if IsWindowVisible(hwnd) == 0 {
        return 1;
    }
    if class_name(hwnd) != "#32770" {
        return 1;
    }
    let title = window_text(hwnd);
    if title == "运行" || title == "Run" {
        *out = hwnd;
        return 0;
    }
    1
}

unsafe fn find_run_dialog() -> Hwnd {
    let mut found: Hwnd = 0;
    EnumWindows(Some(find_run_cb), &mut found as *mut Hwnd as Lparam);
    found
}

unsafe extern "system" fn read_edit_cb(hwnd: Hwnd, lparam: Lparam) -> Bool {
    let out = &mut *(lparam as *mut String);
    if class_name(hwnd) == "Edit" {
        let mut buf = vec![0u16; 2048];
        let n = SendMessageW(hwnd, WM_GETTEXT, buf.len(), buf.as_mut_ptr() as Lparam);
        let n = n.clamp(0, buf.len() as isize) as usize;
        *out = String::from_utf16_lossy(&buf[..n]);
        return 0;
    }
    1
}

// 别的进程里的控件只能靠 WM_GETTEXT 读
unsafe fn read_run_text(dlg: Hwnd) -> String {
    let mut text = String::new();
    EnumChildWindows(dlg, Some(read_edit_cb), &mut text as *mut String as Lparam);
    text
}

fn is_blocked_text(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let lower = text.to_lowercase();
    PATTERNS
        .get()
        .map(|ps| ps.iter().any(|p| lower.contains(&p.to_lowercase())))
        .unwrap_or(false)
}

// 返回 Some(是否截图) 表示这个键要吞掉
fn blocked_vk_action(vk: u32) -> Option<bool> {
    BLOCKED_VKS.get().and_then(|vs| {
        vs.iter()
            .find(|(code, _)| *code == vk)
            .map(|(_, snip)| *snip)
    })
}

// 触发系统截图（等同 Win+Shift+S）
fn fire_snip() {
    unsafe {
        let op = wide("open");
        let uri = wide("ms-screenclip:");
        ShellExecuteW(0, op.as_ptr(), uri.as_ptr(), std::ptr::null(), std::ptr::null(), 1);
    }
    log_stamped("snip triggered");
}

fn watchdog() {
    loop {
        thread::sleep(Duration::from_millis(10));

        if PENDING_SNIP.swap(false, Ordering::Relaxed) {
            fire_snip();
        }

        if PAUSED.load(Ordering::Relaxed) {
            continue;
        }

        let now = now_ms();
        let until = SUPPRESS_UNTIL.load(Ordering::SeqCst);
        if until != 0 && now > until {
            SUPPRESS_UNTIL.store(0, Ordering::SeqCst);
            log_stamped("burst end");
        }
        if SUPPRESS_UNTIL.load(Ordering::SeqCst) != 0 {
            continue;
        }

        unsafe {
            let dlg = find_run_dialog();
            if dlg == 0 {
                continue;
            }
            let text = read_run_text(dlg);
            if !is_blocked_text(&text) {
                continue;
            }
            PostMessageW(dlg, WM_CLOSE, 0, 0);
            SUPPRESS_START.store(now, Ordering::SeqCst);
            SUPPRESS_UNTIL.store(now + 1200, Ordering::SeqCst);
            log_stamped(&format!("blocked run: {}", text));
        }
    }
}

// 只吞按键按下；抬起一律放行，避免出现卡住的修饰键
unsafe extern "system" fn hook_proc(code: i32, wparam: Wparam, lparam: Lparam) -> Lparam {
    if code < 0 {
        return CallNextHookEx(0, code, wparam, lparam);
    }
    let msg = wparam as u32;
    if msg != WM_KEYDOWN && msg != WM_SYSKEYDOWN && msg != WM_KEYUP && msg != WM_SYSKEYUP {
        return CallNextHookEx(0, code, wparam, lparam);
    }
    if PAUSED.load(Ordering::Relaxed) {
        return CallNextHookEx(0, code, wparam, lparam);
    }

    let k = &*(lparam as *const KbdLlHookStruct);
    if !DEBUG_INJECTED.load(Ordering::Relaxed) && (k.flags & LLKHF_INJECTED) != 0 {
        return CallNextHookEx(0, code, wparam, lparam);
    }

    if TRACE_VK.load(Ordering::Relaxed) == k.vk_code as i64 {
        log_stamped(&format!(
            "trace pass vk=0x{:02X} scan=0x{:02X}",
            k.vk_code, k.scan_code
        ));
    }

    if let Some(snip) = blocked_vk_action(k.vk_code) {
        if snip && (k.flags & LLKHF_UP) == 0 {
            PENDING_SNIP.store(true, Ordering::Relaxed);
        }
        log_stamped(&format!(
            "blocked vk=0x{:02X} scan=0x{:02X}{}",
            k.vk_code,
            k.scan_code,
            if snip { " (snip)" } else { "" }
        ));
        return 1;
    }

    let now = now_ms();
    let until = SUPPRESS_UNTIL.load(Ordering::SeqCst);
    if until == 0 || now > until {
        return CallNextHookEx(0, code, wparam, lparam);
    }

    let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    if !down {
        return CallNextHookEx(0, code, wparam, lparam);
    }

    let cap = SUPPRESS_START.load(Ordering::SeqCst) + 6000;
    let next = (now + 1200).min(cap);
    SUPPRESS_UNTIL.store(next, Ordering::SeqCst);
    1
}

// ---------- 开机自启 ----------

fn autostart_enabled() -> bool {
    unsafe {
        let sub = wide(RUN_KEY);
        let mut key: Hkey = 0;
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            std::ptr::null(),
            0,
            KEY_QUERY_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        ) != 0
        {
            return false;
        }
        let name = wide(RUN_VALUE);
        let mut buf = [0u8; 1024];
        let mut cb: u32 = buf.len() as u32;
        let mut typ: u32 = 0;
        let rc = RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            &mut typ,
            buf.as_mut_ptr(),
            &mut cb,
        );
        RegCloseKey(key);
        if rc != 0 {
            return false;
        }
        let u16s: Vec<u16> = buf[..cb as usize]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let s = String::from_utf16_lossy(&u16s);
        s.replace('\u{0}', "").to_lowercase().contains("keymacroblocker")
    }
}

fn set_autostart(on: bool) {
    unsafe {
        let sub = wide(RUN_KEY);
        let mut key: Hkey = 0;
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            std::ptr::null(),
            0,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        ) != 0
        {
            log_stamped("autostart: open key failed");
            return;
        }
        let name = wide(RUN_VALUE);
        if on {
            let exe = EXE_PATH.get().cloned().unwrap_or_default();
            let quoted = format!("\"{}\"", exe);
            let data = wide(&quoted);
            let bytes =
                std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2);
            let rc = RegSetValueExW(key, name.as_ptr(), 0, REG_SZ, bytes.as_ptr(), bytes.len() as u32);
            log_stamped(&format!("autostart -> on rc={} path={}", rc, quoted));
        } else {
            let rc = RegDeleteValueW(key, name.as_ptr());
            log_stamped(&format!("autostart -> off rc={}", rc));
        }
        RegCloseKey(key);
    }
}

// ---------- 托盘 ----------

unsafe fn show_menu(hwnd: Hwnd) {
    let mut pt = Point { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);

    let menu = CreatePopupMenu();
    if menu == 0 {
        return;
    }

    let paused = PAUSED.load(Ordering::Relaxed);
    let pause_text = wide(if paused { "恢复拦截" } else { "暂停拦截" });
    AppendMenuW(menu, MF_STRING, ID_PAUSE as usize, pause_text.as_ptr());

    let auto_on = autostart_enabled();
    let auto_text = wide("开机自启");
    let flags = if auto_on { MF_STRING | MF_CHECKED } else { MF_STRING };
    AppendMenuW(menu, flags, ID_AUTOSTART as usize, auto_text.as_ptr());

    let log_text = wide("打开日志");
    AppendMenuW(menu, MF_STRING, ID_OPENLOG as usize, log_text.as_ptr());

    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());

    let exit_text = wide("退出");
    AppendMenuW(menu, MF_STRING, ID_EXIT as usize, exit_text.as_ptr());

    let cmd = TrackPopupMenu(
        menu,
        TPM_RIGHTBUTTON | TPM_RETURNCMD,
        pt.x,
        pt.y,
        0,
        hwnd,
        std::ptr::null(),
    ) as u32;
    DestroyMenu(menu);
    PostMessageW(hwnd, WM_NULL, 0, 0);

    match cmd {
        ID_PAUSE => {
            let now_paused = !paused;
            PAUSED.store(now_paused, Ordering::Relaxed);
            log_stamped(if now_paused { "paused" } else { "resumed" });
        }
        ID_AUTOSTART => {
            set_autostart(!auto_on);
        }
        ID_OPENLOG => {
            if let Some(p) = LOG_PATH.get() {
                let op = wide("open");
                let file = wide(&p.to_string_lossy());
                ShellExecuteW(0, op.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), 1);
            }
        }
        ID_EXIT => {
            log_stamped("exit by tray");
            PostQuitMessage(0);
        }
        _ => {}
    }
}

unsafe extern "system" fn wnd_proc(hwnd: Hwnd, msg: u32, wparam: Wparam, lparam: Lparam) -> Lparam {
    match msg {
        WM_TRAY => {
            let ev = lparam as u32;
            if ev == WM_RBUTTONUP || ev == WM_LBUTTONDBLCLK {
                show_menu(hwnd);
            }
            0
        }
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as u32;
            if id == ID_EXIT {
                log_stamped("exit by command");
                PostQuitMessage(0);
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// 从内嵌的 ico 里挑一个最接近托盘尺寸的图
fn load_icon() -> isize {
    let data: &[u8] = include_bytes!("../icon.ico");
    if data.len() < 6 {
        return unsafe { LoadIconW(0, 32512 as *const u16) };
    }
    let target = unsafe { GetSystemMetrics(49) }; // SM_CXSMICON
    let count = u16::from_le_bytes([data[4], data[5]]) as usize;
    let mut best: Option<(i32, usize, usize, u32, u32)> = None;
    for i in 0..count {
        let base = 6 + i * 16;
        if base + 16 > data.len() {
            break;
        }
        let w = if data[base] == 0 { 256 } else { data[base] as i32 };
        let h = if data[base + 1] == 0 { 256 } else { data[base + 1] as i32 };
        let len = u32::from_le_bytes([
            data[base + 8],
            data[base + 9],
            data[base + 10],
            data[base + 11],
        ]) as usize;
        let off = u32::from_le_bytes([
            data[base + 12],
            data[base + 13],
            data[base + 14],
            data[base + 15],
        ]) as usize;
        if off + len > data.len() {
            continue;
        }
        let score = (w - target).abs();
        if best.map(|b| score < b.0).unwrap_or(true) {
            best = Some((score, off, len, w as u32, h as u32));
        }
    }
    if let Some((_, off, len, w, h)) = best {
        unsafe {
            let hicon = CreateIconFromResourceEx(
                data.as_ptr().add(off),
                len as u32,
                1,
                0x0003_0000,
                w as i32,
                h as i32,
                0,
            );
            if hicon != 0 {
                log_stamped(&format!("icon {}x{} ok", w, h));
                return hicon;
            }
            log_stamped(&format!("icon {}x{} failed, fallback", w, h));
        }
    }
    unsafe { LoadIconW(0, 32512 as *const u16) }
}

fn add_tray(hwnd: Hwnd, hicon: isize) -> Bool {
    unsafe {
        let mut data: NotifyIconDataW = std::mem::zeroed();
        data.cb_size = std::mem::size_of::<NotifyIconDataW>() as u32;
        data.h_wnd = hwnd;
        data.u_id = 1;
        data.u_flags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.u_callback_message = WM_TRAY;
        data.h_icon = hicon;
        let tip = wide_fixed("KeyMacroBlocker", 128);
        data.sz_tip.copy_from_slice(&tip);
        let ok = Shell_NotifyIconW(NIM_ADD, &data);
        if ok == 0 {
            log_stamped(&format!("tray add failed err={}", GetLastError()));
        }
        ok
    }
}

fn remove_tray(hwnd: Hwnd) {
    unsafe {
        let mut data: NotifyIconDataW = std::mem::zeroed();
        data.cb_size = std::mem::size_of::<NotifyIconDataW>() as u32;
        data.h_wnd = hwnd;
        data.u_id = 1;
        Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

// ---------- 启动 ----------

fn load_patterns() {
    let mut subs: Vec<String> = Vec::new();
    let mut vks: Vec<(u32, bool)> = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let cfg = dir.join("patterns.txt");
            if let Ok(content) = std::fs::read_to_string(&cfg) {
                for raw in content.lines() {
                    let line = raw.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    let lower = line.to_lowercase();
                    if let Some(rest) = lower.strip_prefix("vk=") {
                        // 形如 vk=0x2C 或 vk=0x2C:snip
                        let mut parts = rest.splitn(2, ':');
                        let key = parts.next().unwrap_or("").trim();
                        let action = parts.next().unwrap_or("").trim();
                        let hex = key.trim_start_matches("0x");
                        if let Ok(v) = u32::from_str_radix(hex, 16) {
                            vks.push((v, action == "snip"));
                        }
                        continue;
                    }
                    subs.push(line.to_string());
                }
            }
        }
    }

    if subs.is_empty() {
        subs = vec!["oneclick".to_string(), "lenovo".to_string()];
    }
    PATTERNS.set(subs).ok();
    BLOCKED_VKS.set(vks).ok();
}

fn main() {
    let name = wide("KeyMacroBlocker.Mutex");
    unsafe {
        CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
    }

    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(base).join("KeyMacroBlocker");
    let _ = std::fs::create_dir_all(&dir);
    let log_path = dir.join("blocker.log");
    let _ = std::fs::remove_file(&log_path);
    LOG_PATH.set(log_path).ok();

    if let Ok(exe) = std::env::current_exe() {
        EXE_PATH.set(exe.to_string_lossy().to_string()).ok();
    }

    if let Ok(v) = std::env::var("KMB_TRACE_VK") {
        let hex = v.trim().trim_start_matches("0x").trim_start_matches("0X");
        if let Ok(n) = i64::from_str_radix(hex, 16) {
            TRACE_VK.store(n, Ordering::Relaxed);
        }
    }

    DEBUG_INJECTED.store(
        std::env::var("KMB_DEBUG_INJECTED").map(|v| v == "1").unwrap_or(false),
        Ordering::Relaxed,
    );

    load_patterns();

    let pats = PATTERNS.get().map(|p| p.join(",")).unwrap_or_default();
    let vks = BLOCKED_VKS
        .get()
        .map(|v| {
            v.iter()
                .map(|(x, snip)| {
                    if *snip {
                        format!("0x{:02X}:snip", x)
                    } else {
                        format!("0x{:02X}", x)
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    log_stamped(&format!("start patterns={} vk={}", pats, vks));

    // 首次运行自动写开机自启
    if !autostart_enabled() {
        set_autostart(true);
    }

    thread::spawn(watchdog);

    unsafe {
        let hmod = GetModuleHandleW(std::ptr::null());
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), hmod, 0);
        if hook == 0 {
            log_stamped("hook fail");
            return;
        }
        log_stamped("hook ok");

        let hicon = load_icon();
        let cls_name = wide("KeyMacroBlockerWnd");
        let wc = WndClassW {
            style: 0,
            lpfn_wnd_proc: Some(wnd_proc),
            cb_cls_extra: 0,
            cb_wnd_extra: 0,
            h_instance: hmod,
            h_icon: hicon,
            h_cursor: LoadCursorW(0, 32512 as *const u16),
            hbr_background: 0,
            lpsz_menu_name: std::ptr::null(),
            lpsz_class_name: cls_name.as_ptr(),
        };
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            0,
            cls_name.as_ptr(),
            cls_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            hmod,
            std::ptr::null(),
        );
        if hwnd == 0 {
            log_stamped("window fail");
        } else {
            add_tray(hwnd, hicon);
        }

        let mut msg: Msg = std::mem::zeroed();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if hwnd != 0 {
            remove_tray(hwnd);
            DestroyWindow(hwnd);
        }
        UnhookWindowsHookEx(hook);
        log_stamped("exit");
    }
}
