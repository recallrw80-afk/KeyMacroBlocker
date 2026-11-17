# KeyMacroBlocker

键盘上那颗宏键按下时不产生任何键码，而是由固件自己发 Win+R、把
https://oneclick.lenovo.com.cn/ 逐字敲进「运行」框再回车，打开厂商推广页。
因为没有键码，注册表 Scancode Map 对它无效。

> **作者特别讨厌这把键盘。** 写这个东西没有别的原因，就是每次误触都得手动关掉运行框和浏览器，烦到极点。

唯一能拦的地方是按键流：每 10 ms 看一眼「运行」框，里面有那个网址就关掉它。
顺带支持按虚拟键码直接吞按键（默认吞掉 `PrintScreen`，也就是那颗"截图"键）。

Rust 实现，纯 `std` + 直接 bind `user32`，**零第三方 crate**，单文件 exe，离线可编。
常驻托盘，自带开机自启。

## 实测时间线

在一把 `VID_32C2 / PID_0018` 的 2.4G 无线键盘上抓到的：

| 时间 | 事件 |
|---|---|
| +0 ms | 运行框出现 |
| +459 ms | 开始输入第一个字符 |
| +675 ms | `https://` |
| +877 ms | `https://oneclick` |
| +1237 ms | `https://oneclick.lenovo.com.cn/` 输完 |
| +1347 ms | 回车，对话框消失，浏览器打开 |

单字符间隔 16~32 ms，全程约 780 ms，回车只隔 110 ms。
浏览器最终会跳到 `xbb.lenovo.com.cn/?sourceType=2`。

## 原理

1. 每 **10 ms** 用 `EnumWindows` 找一次「运行」对话框（类 `#32770`，标题 `运行`/`Run`）。
2. 用 `WM_GETTEXT` 读它输入框的内容——**不能用 `GetWindowText`**，那个读不到别的进程里的控件，这是第一版失败的原因。
3. 内容命中关键词（默认 `oneclick` / `lenovo`）就 `WM_CLOSE` 关掉对话框。
4. 同时装一个低级键盘钩子：
   - 把这一串按键剩下的部分（连同回车）吞掉，避免字符漏到别的窗口里；
   - 把配置里列出的虚拟键码直接吞掉。

吞的是"按下"，"抬起"一律放行，不会出现卡住的修饰键。

因为 `oneclick` 这个词在回车前约 470 ms 就完整了，拦截有充裕余量。

## 行为

- **正常 `Win+R` 完全不受影响**，没有延迟、没有吞键。
- 按那个宏键：运行框闪一下（约 0.9 秒）然后自己消失，不开浏览器。
- 按那个"截图"键（直接发 `PrintScreen`）：什么都不发生。
- 其他任何 Run 输入（`notepad`、`cmd`……）照常执行。

> 运行框没法阻止它弹出——除非把 `Win+R` 整个废掉，那样你自己的 Win+R 也就没了。

## 托盘

任务栏右下角有个图标，右键菜单：

| 菜单项 | 作用 |
|---|---|
| 暂停拦截 / 恢复拦截 | 临时全部放行（含 `vk=` 规则） |
| 开机自启 | 勾选状态，切换写/删 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 下的 `KeyMacroBlocker` |
| 打开日志 | 用默认程序打开 `blocker.log` |
| 退出 | 移除托盘图标并结束进程 |

**开机自启**首次运行会自动写入（指向当前 exe 的绝对路径），不需要手动放快捷方式。

**图标**是 `icon.ico`（9 个尺寸 16~256，自制，MIT），编译时用 `include_bytes!` 打进 exe，
运行时按 `SM_CXSMICON` 挑最合适的那一档用 `CreateIconFromResourceEx` 转成 `HICON`。
所以发布时**只发 exe 就行**，不用带图标文件。

> Windows 11 会把新出现的托盘图标默认收进 `^` 折叠区，拖出来固定一下就好。

想彻底停掉：托盘 → 退出；想连自启一起清掉：先取消"开机自启"再退出。

## 构建

需要 Rust（`rustup`）。msvc 和 gnu 两种工具链都行；gnu 工具链自带链接器，
不用装 Visual Studio：

```powershell
rustup default stable-x86_64-pc-windows-gnu
cargo build --release
```

或者直接跑 `.uild.ps1`，它会把产物拷到 `bin`。

产物 `targeteleaseKeyMacroBlocker.exe`，约 320 KB，静态链接 CRT，无运行库依赖。

## 安装

把 `KeyMacroBlocker.exe` 和 `patterns.txt` 放到同一个目录，双击运行即可。
首次运行会自己写开机自启。

> 已经在运行再双击不会有任何反应——靠命名互斥体做了单实例。

## 配置

`patterns.txt` 与 exe 同目录，改完重启程序生效。

```
# 子串匹配：运行框内容命中任意一行就关掉它（不区分大小写）
oneclick
lenovo

# 直接吞掉的按键：十六进制虚拟键码
# 0x2C = PrintScreen
vk=0x2C
```

- 不带前缀的行 = 运行框内容的匹配串。
- `vk=` 开头的行 = 要吞掉的虚拟键码（`0x` 前缀可写可不写）。
- `#` 开头是注释。
- 文件不存在时用内置的 `oneclick` / `lenovo`，不吞任何按键。

**注意**：`vk=0x2C` 会把**整个 PrintScreen 键**吃掉，包括键盘上正常的 PrintScreen——
宏键和普通 PrintScreen 发的是同一个键码，分不出来。不想失去截图就删掉那一行，改用 `Win+Shift+S`。

换键盘型号、换推广域名，改这个文件就行。

## 日志

`%LOCALAPPDATA%\KeyMacroBlocker\blocker.log`，只记状态，不记你打的字：

```
11:43:43.879 start patterns=oneclick,lenovo vk=0x2C
11:43:43.879 autostart -> on rc=0 path="C:\\...\\KeyMacroBlocker.exe"
11:43:43.880 hook ok
11:43:47.133 blocked run: https://oneclick.lenovo.com.cn/
11:37:13.179 blocked vk 0x2C
```

## 调试

设环境变量 `KMB_DEBUG_INJECTED=1` 后启动，程序会把 `SendInput` 合出来的按键
也当成真实按键处理，方便用脚本复现测试。日常不要开。

## 已知限制

- 只处理「运行」对话框这一条路径。如果某款键盘是直接 `ShellExecute` 一个网址（不经过 Run），
  这个程序拦不到，需要换思路。
- 关键词匹配是子串匹配。真有人手动在运行框里敲 `lenovo` 也会被关掉——但那种情况基本不存在。
- 宏键在"运行框出现 → 被关掉"之间会闪一下，无法消除。
- 不是 Windows 服务：键盘钩子必须在用户会话里才拿得到按键，Session 0 的服务钩不到。

## 许可证

MIT（见 [LICENSE](LICENSE)）
