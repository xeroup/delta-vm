// links a .o file into a native executable using the system linker

use crate::{CraneliftError, Result};

pub fn link_exe(obj: &[u8], no_console: bool) -> Result<Vec<u8>> {
    let obj_path = tmp_path("_delta_link.o");
    let exe_path = tmp_path("_delta_link_out");
    std::fs::write(&obj_path, obj).map_err(|e| CraneliftError(e.to_string()))?;

    link_native(&obj_path, &exe_path, no_console).map_err(CraneliftError)?;

    #[cfg(target_os = "windows")]
    let read_path = format!("{exe_path}.exe");
    #[cfg(not(target_os = "windows"))]
    let read_path = exe_path.clone();

    let bytes = std::fs::read(&read_path).map_err(|e| CraneliftError(e.to_string()))?;

    // cleanup
    let _ = std::fs::remove_file(&obj_path);
    let _ = std::fs::remove_file(&read_path);

    Ok(bytes)
}

fn tmp_path(name: &str) -> String {
    #[cfg(target_os = "windows")]
    { format!("{}\\{name}", std::env::var("TEMP").unwrap_or_else(|_| "C:\\Temp".into())) }
    #[cfg(not(target_os = "windows"))]
    { format!("/tmp/{name}") }
}

#[cfg(target_os = "linux")]
fn link_native(obj: &str, exe: &str, _no_console: bool) -> std::result::Result<(), String> {
    // find system C runtime
    let crt_dirs = [
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
        "/usr/lib",
    ];
    let crt_dir = crt_dirs.iter()
        .find(|d| std::path::Path::new(&format!("{d}/crt1.o")).exists())
        .copied()
        .unwrap_or("/usr/lib/x86_64-linux-gnu");

    // try cc first (simplest, handles everything automatically)
    if let Ok(status) = std::process::Command::new("cc")
        .args([obj, "-o", exe, "-lm", "-no-pie"])
        .status()
    {
        if status.success() { return Ok(()); }
    }

    // fallback: invoke ld directly
    let dynlinker = if std::path::Path::new("/lib64/ld-linux-x86-64.so.2").exists() {
        "/lib64/ld-linux-x86-64.so.2"
    } else if std::path::Path::new("/lib/ld-linux-aarch64.so.1").exists() {
        "/lib/ld-linux-aarch64.so.1"
    } else {
        "/lib64/ld-linux-x86-64.so.2"
    };

    let crt1 = format!("{crt_dir}/crt1.o");
    let crti = format!("{crt_dir}/crti.o");
    let crtn = format!("{crt_dir}/crtn.o");
    let has_crt = std::path::Path::new(&crt1).exists();

    let mut cmd = std::process::Command::new("ld");
    cmd.arg(obj).arg("-o").arg(exe)
        .arg("-no-pie")
        .arg("-dynamic-linker").arg(dynlinker)
        .arg("-L/usr/lib/x86_64-linux-gnu")
        .arg("-L/usr/lib/aarch64-linux-gnu")
        .arg("-L/usr/lib");
    if has_crt { cmd.arg(&crt1).arg(&crti); }
    cmd.arg("-lc").arg("-lm");
    if has_crt { cmd.arg(&crtn); }

    let status = cmd.status().map_err(|e| format!("ld failed: {e}"))?;
    if status.success() { Ok(()) } else { Err("ld returned non-zero".into()) }
}

#[cfg(target_os = "macos")]
fn link_native(obj: &str, exe: &str, _no_console: bool) -> std::result::Result<(), String> {
    // cc is always available on macOS (Xcode CLT)
    let status = std::process::Command::new("cc")
        .args([obj, "-o", exe])
        .status()
        .map_err(|e| format!("cc failed: {e}"))?;
    if status.success() { Ok(()) } else { Err("cc returned non-zero".into()) }
}

#[cfg(target_os = "windows")]
fn link_native(obj: &str, exe: &str, no_console: bool) -> std::result::Result<(), String> {
    let subsystem = if no_console { "WINDOWS" } else { "CONSOLE" };
    for linker in &["link", "gcc", "cc"] {
        let mut cmd = std::process::Command::new(linker);
        if *linker == "link" {
            cmd.args([obj, &format!("/OUT:{exe}.exe"), &format!("/SUBSYSTEM:{subsystem}"), "/DEFAULTLIB:msvcrt"]);
        } else {
            let mut args = vec![obj, "-o", exe];
            if no_console { args.push("-mwindows"); }
            cmd.args(args);
        }
        if let Ok(s) = cmd.status() {
            if s.success() { return Ok(()); }
        }
    }
    Err("no linker found; install MSVC or MinGW and add to PATH".into())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn link_native(_obj: &str, _exe: &str, _no_console: bool) -> std::result::Result<(), String> {
    Err("--emit exe is not supported on this platform".into())
}
