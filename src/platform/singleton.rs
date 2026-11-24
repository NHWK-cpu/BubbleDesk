//! Penguncian satu instance lewat named mutex (FR-12).
//!
//! Instance kedua tidak perlu memberi tahu yang pertama untuk versi ini; itu
//! ("bila mungkin" pada FR-12) menyusul saat ada tray/loop event (M5).

use windows::core::{w, Result};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;

/// Kepemilikan instance tunggal. Menutup handle mutex saat di-drop.
pub struct InstanceGuard(HANDLE);

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // SAFETY: handle milik guard ini dan belum pernah ditutup.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Mengakuisisi instance tunggal.
///
/// Mengembalikan `Ok(Some(guard))` bila menjadi instance utama, `Ok(None)` bila
/// instance lain sudah berjalan, dan `Err` bila panggilan Win32 gagal.
pub fn acquire_single_instance() -> Result<Option<InstanceGuard>> {
    // SAFETY: nama statis, tanpa atribut keamanan, tanpa memori yang dibagikan.
    let handle = unsafe { CreateMutexW(None, false, w!("Local\\BubbleDesk.SingleInstance")) }?;

    // SAFETY: GetLastError dibaca tepat setelah CreateMutexW pada thread yang sama.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // SAFETY: handle valid; kita bukan pemilik instance, jadi handle ditutup.
        let _ = unsafe { CloseHandle(handle) };
        return Ok(None);
    }
    Ok(Some(InstanceGuard(handle)))
}
