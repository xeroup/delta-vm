// installs das and dvm binaries to the appropriate system location
// and, on windows, adds that location to the user PATH

use std::path::{Path, PathBuf};

fn main() {
    let self_dir = self_dir();

    #[cfg(target_os = "windows")]
    run(&self_dir, windows_dest(), true);

    #[cfg(not(target_os = "windows"))]
    run(&self_dir, PathBuf::from("/usr/local/bin"), false);
}

fn run(src_dir: &Path, dest: PathBuf, add_to_path: bool) {
    println!("Installing to: {}", dest.display());

    create_dir(&dest);

    for name in &["das", "dvm"] {
        let src = src_bin(src_dir, name);
        let dst = dest_bin(&dest, name);

        if !src.exists() {
            eprintln!("error: binary not found: {}", src.display());
            std::process::exit(1);
        }

        copy_file(&src, &dst);
        println!("  installed: {}", dst.display());

        #[cfg(not(target_os = "windows"))]
        set_executable(&dst);
    }

    if add_to_path {
        #[cfg(target_os = "windows")]
        windows_add_to_path(&dest);
    }

    println!("Done.");
}

// returns the directory that contains the installer binary
fn self_dir() -> PathBuf {
    std::env::current_exe()
        .expect("cannot resolve installer path")
        .parent()
        .expect("installer has no parent directory")
        .to_path_buf()
}

// platform-specific binary filename
fn src_bin(dir: &Path, name: &str) -> PathBuf {
    #[cfg(target_os = "windows")]
    return dir.join(format!("{name}.exe"));

    #[cfg(not(target_os = "windows"))]
    return dir.join(name);
}

fn dest_bin(dir: &Path, name: &str) -> PathBuf {
    #[cfg(target_os = "windows")]
    return dir.join(format!("{name}.exe"));

    #[cfg(not(target_os = "windows"))]
    return dir.join(name);
}

fn create_dir(path: &Path) {
    if let Err(e) = std::fs::create_dir_all(path) {
        eprintln!("error: cannot create directory {}: {e}", path.display());
        std::process::exit(1);
    }
}

fn copy_file(src: &Path, dst: &Path) {
    if let Err(e) = std::fs::copy(src, dst) {
        eprintln!("error: cannot copy {} -> {}: {e}", src.display(), dst.display());
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "windows"))]
fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .expect("cannot read permissions")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).expect("cannot set permissions");
}

// returns C:\Program Files\Delta-VM
#[cfg(target_os = "windows")]
fn windows_dest() -> PathBuf {
    let base = std::env::var("PROGRAMFILES")
        .unwrap_or_else(|_| r"C:\Program Files".to_string());
    PathBuf::from(base).join("Delta-VM")
}

// adds dest to the current user PATH via the registry (persists across sessions)
#[cfg(target_os = "windows")]
fn windows_add_to_path(dest: &Path) {
    let dest_str = dest.to_string_lossy().to_string();

    // read current user PATH from registry
    let output = std::process::Command::new("reg")
        .args(["query", r"HKCU\Environment", "/v", "PATH"])
        .output();

    let current = match output {
        Ok(o) if o.status.success() => {
            let raw = String::from_utf8_lossy(&o.stdout).to_string();
            // reg query output: "    PATH    REG_SZ    <value>"
            raw.lines()
                .find(|l| l.trim_start().starts_with("PATH"))
                .and_then(|l| l.splitn(3, "REG_SZ").nth(1))
                .map(|v| v.trim().to_string())
                .unwrap_or_default()
        }
        _ => String::new(),
    };

    // skip if already present
    if current.split(';').any(|p| p.trim().eq_ignore_ascii_case(&dest_str)) {
        println!("PATH already contains {dest_str}");
        return;
    }

    let new_path = if current.is_empty() {
        dest_str.clone()
    } else {
        format!("{current};{dest_str}")
    };

    let status = std::process::Command::new("reg")
        .args(["add", r"HKCU\Environment", "/v", "PATH", "/t", "REG_EXPAND_SZ", "/d", &new_path, "/f"])
        .status();

    match status {
        Ok(s) if s.success() => println!("Added to PATH: {dest_str}"),
        _ => eprintln!("warning: could not update PATH in registry - add {dest_str} manually"),
    }
}
