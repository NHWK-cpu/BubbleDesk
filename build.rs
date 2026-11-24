//! Menyematkan manifest aplikasi (DPI v2, longPathAware, asInvoker), ikon,
//! dan metadata versi ke binary.
//!
//! Memakai argumen linker MSVC dan `rc.exe` Windows SDK secara langsung
//! supaya tidak menambah dependensi build.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/manifest.xml");
    println!("cargo:rerun-if-changed=assets/app.ico");

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    let Some(dir) = std::env::var_os("CARGO_MANIFEST_DIR") else {
        return;
    };
    let manifest_dir = PathBuf::from(dir);
    let manifest = manifest_dir.join("assets").join("manifest.xml");

    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
        manifest.display()
    );

    embed_rc_resources(&manifest_dir);
}

fn embed_rc_resources(manifest_dir: &Path) {
    let ico = manifest_dir.join("assets").join("app.ico");
    if !ico.exists() {
        return;
    }
    let Some(rc_exe) = find_rc_exe() else {
        println!("cargo:warning=rc.exe tidak ditemukan; ikon & VERSIONINFO tidak disematkan");
        return;
    };
    let Some(out_dir) = std::env::var_os("OUT_DIR").map(PathBuf::from) else {
        return;
    };

    let version = env!("CARGO_PKG_VERSION");
    let parts: Vec<&str> = version.split('.').collect();
    let maj = parts.first().copied().unwrap_or("0");
    let min = parts.get(1).copied().unwrap_or("1");
    let pat = parts.get(2).copied().unwrap_or("0");
    let ico_escaped = ico.to_string_lossy().replace('\\', "\\\\");

    let rc_content = format!(
        r#"1 ICON "{ico_escaped}"

1 VERSIONINFO
FILEVERSION {maj},{min},{pat},0
PRODUCTVERSION {maj},{min},{pat},0
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "Hafizh Ammar Muflih"
            VALUE "FileDescription", "BubbleDesk - Wallpaper hidup berisi bubble"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "bubbledesk"
            VALUE "LegalCopyright", "Copyright (C) 2026 Hafizh Ammar Muflih (AGPL-3.0)"
            VALUE "OriginalFilename", "bubbledesk.exe"
            VALUE "ProductName", "BubbleDesk"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x0409, 1200
    END
END
"#
    );

    let rc_path = out_dir.join("bubbledesk.rc");
    let res_path = out_dir.join("bubbledesk.res");
    if std::fs::write(&rc_path, rc_content).is_err() {
        return;
    }

    let status = Command::new(rc_exe)
        .args(["/nologo", "/fo"])
        .arg(&res_path)
        .arg(&rc_path)
        .status();

    match status {
        Ok(s) if s.success() && res_path.exists() => {
            println!("cargo:rustc-link-arg-bins={}", res_path.display());
        }
        Ok(s) => println!("cargo:warning=rc.exe keluar dengan status {s}"),
        Err(e) => println!("cargo:warning=gagal menjalankan rc.exe: {e}"),
    }
}

fn find_rc_exe() -> Option<PathBuf> {
    if let Ok(status) = Command::new("rc.exe").arg("/?").output() {
        if status.status.success() || !status.stdout.is_empty() || !status.stderr.is_empty() {
            return Some(PathBuf::from("rc.exe"));
        }
    }

    let kits_bin = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let entries = std::fs::read_dir(kits_bin).ok()?;
    let mut versions: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|x| x.path()))
        .filter(|p| p.join("x64").join("rc.exe").is_file())
        .collect();
    versions.sort();
    versions.pop().map(|p| p.join("x64").join("rc.exe"))
}
