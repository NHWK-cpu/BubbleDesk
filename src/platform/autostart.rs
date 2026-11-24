//! Mulai bersama Windows lewat registry `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
//! (spec §15, M5). Menulis HANYA di hive pengguna; tidak butuh hak admin.

#![cfg(windows)]

use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    REG_VALUE_TYPE,
};

/// Subkey Run milik pengguna.
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Nama value entri BubbleDesk.
const VALUE_NAME: &str = "BubbleDesk";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Format baris perintah autostart: path exe yang dikutip.
fn command_line_for(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// Mengaktifkan/menonaktifkan autostart agar sesuai konfigurasi.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let name = wide(VALUE_NAME);
    let mut key = HKEY::default();
    // SAFETY: subkey adalah string statis yang valid; key ditutup sebelum kembali.
    let sub = wide(RUN_KEY);
    let rc = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            None,
            PCWSTR(std::ptr::null()),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    };
    if rc != ERROR_SUCCESS {
        return Err(format!("RegCreateKeyExW gagal: {rc:?}"));
    }

    let result = if enabled {
        let Ok(exe) = std::env::current_exe() else {
            // SAFETY: key dibuka sendiri dengan KEY_SET_VALUE.
            let _ = unsafe { RegCloseKey(key) };
            return Err("tidak dapat menentukan path executable".to_owned());
        };
        let value = wide(&command_line_for(&exe));
        let bytes = bytemuck::cast_slice::<u16, u8>(&value);
        // SAFETY: key valid dengan KEY_SET_VALUE; bytes menunjuk value UTF-16 + null.
        let rc = unsafe { RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(bytes)) };
        if rc == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("RegSetValueExW gagal: {rc:?}"))
        }
    } else {
        // SAFETY: key valid dengan KEY_SET_VALUE; menghapus value yang mungkin tidak ada.
        let rc = unsafe { RegDeleteValueW(key, PCWSTR(name.as_ptr())) };
        if rc == ERROR_SUCCESS || rc == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(format!("RegDeleteValueW gagal: {rc:?}"))
        }
    };

    // SAFETY: key dibuka di atas dan belum ditutup.
    let _ = unsafe { RegCloseKey(key) };
    result
}

/// Menanyakan apakah entri autostart BubbleDesk ada di registry.
pub fn is_enabled() -> bool {
    let name = wide(VALUE_NAME);
    let sub = wide(RUN_KEY);
    let mut key = HKEY::default();
    // SAFETY: subkey valid; key ditutup sebelum kembali.
    let rc = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            None,
            PCWSTR(std::ptr::null()),
            REG_OPTION_NON_VOLATILE,
            KEY_QUERY_VALUE,
            None,
            &mut key,
            None,
        )
    };
    if rc != ERROR_SUCCESS {
        return false;
    }
    let mut ty = REG_VALUE_TYPE::default();
    // SAFETY: key valid dengan KEY_QUERY_VALUE; hanya menanyakan keberadaan value.
    let rc =
        unsafe { RegQueryValueExW(key, PCWSTR(name.as_ptr()), None, Some(&mut ty), None, None) };
    // SAFETY: key dibuka di atas.
    let _ = unsafe { RegCloseKey(key) };
    rc == ERROR_SUCCESS
}

/// Menyelaraskan entri autostart dengan konfigurasi, lalu mencatat hasilnya.
pub fn sync_with_config(enabled: bool) {
    match set_enabled(enabled) {
        Ok(()) => log_info!("autostart (registry Run) disetel: enabled={enabled}"),
        Err(e) => log_warn!("gagal menyetel autostart: {e}"),
    }
    log_info!("status autostart registry: {}", is_enabled());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_mengutip_path() {
        let s = command_line_for(Path::new(r"C:\Program Files\BubbleDesk\bubbledesk.exe"));
        assert_eq!(s, "\"C:\\Program Files\\BubbleDesk\\bubbledesk.exe\"");
    }
}
