// 把 icon.ico 编译成 Win32 资源并链接进 exe，让资源管理器显示自定义图标。
// 找不到资源编译器时只告警、不失败。
use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=icon.ico");
    println!("cargo:rerun-if-changed=app.rc");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let toolchain_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    if toolchain_env == "msvc" {
        let res = out.join("app.res");
        match find_rc() {
            Some(rc) => {
                let ok = Command::new(&rc)
                    .arg("/nologo")
                    .arg("/fo")
                    .arg(&res)
                    .arg("app.rc")
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                if ok {
                    println!("cargo:rustc-link-arg={}", res.display());
                    println!("cargo:warning=exe 图标：用 rc.exe 嵌入成功");
                    mark_embedded(&out);
                    return;
                }
                println!("cargo:warning=exe 图标：rc.exe 执行失败，跳过");
            }
            None => println!("cargo:warning=exe 图标：找不到 rc.exe（装 Windows SDK 后自动可用），跳过"),
        }
    } else {
        let obj = out.join("app.o");
        match find_tool("windres") {
            Some(windres) => {
                let ok = Command::new(&windres)
                    .arg("-O")
                    .arg("coff")
                    .arg("-i")
                    .arg("app.rc")
                    .arg("-o")
                    .arg(&obj)
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                if ok {
                    println!("cargo:rustc-link-arg={}", obj.display());
                    println!("cargo:warning=exe 图标：用 windres 嵌入成功");
                    mark_embedded(&out);
                    return;
                }
                println!("cargo:warning=exe 图标：windres 执行失败，跳过");
            }
            None => println!("cargo:warning=exe 图标：找不到 windres，跳过"),
        }
    }
}

// 留给 CI 校验：资源真的编出来了
fn mark_embedded(out: &std::path::Path) {
    let _ = std::fs::write(out.join("icon-embedded.ok"), b"ok");
}

fn find_tool(name: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    for dir in env::split_paths(&paths) {
        for candidate in [format!("{}.exe", name), name.to_string()] {
            let p = dir.join(candidate);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn find_rc() -> Option<PathBuf> {
    if let Some(p) = find_tool("rc") {
        return Some(p);
    }
    for root in [
        r"C:\Program Files (x86)\Windows Kits\10\bin",
        r"C:\Program Files\Windows Kits\10\bin",
    ] {
        let root = PathBuf::from(root);
        if !root.is_dir() {
            continue;
        }
        let mut versions: Vec<PathBuf> = match std::fs::read_dir(&root) {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect(),
            Err(_) => continue,
        };
        versions.sort();
        for v in versions.iter().rev() {
            for arch in ["x64", "x86"] {
                let c = v.join(arch).join("rc.exe");
                if c.is_file() {
                    return Some(c);
                }
            }
        }
    }
    None
}
