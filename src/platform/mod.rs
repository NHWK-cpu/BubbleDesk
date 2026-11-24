//! Pembungkus API Windows.
//!
//! Semua kode `unsafe` dan panggilan Win32 proyek ini hidup di modul ini (atau
//! submodulnya) dan dibungkus fungsi aman, sesuai §6 AGENTS.md.

#[cfg(windows)]
pub mod autostart;
#[cfg(windows)]
pub mod icons;
#[cfg(windows)]
pub mod input;
#[cfg(windows)]
pub mod launch;
#[cfg(windows)]
pub mod menu;
#[cfg(windows)]
pub mod power;
#[cfg(windows)]
mod singleton;
#[cfg(windows)]
pub mod tray;
pub mod wallpaper;
#[cfg(windows)]
pub mod workerw;

#[cfg(windows)]
pub use singleton::acquire_single_instance;

use std::path::PathBuf;

/// Peristiwa sistem yang dikirim dari wndproc helper ke loop event.
#[derive(Debug, Clone, Copy)]
pub enum SystemEvent {
    /// Shell (Explorer) baru saja dimulai ulang — pesan `TaskbarCreated`.
    TaskbarCreated,
    /// Bangunkan loop event ketika thread latar (watcher/ikon) mengirim data.
    WakeUp,
    /// Perintah dari menu ikon tray.
    Tray(tray::TrayAction),
    /// Status daya/sesi berubah (lock/unlock, layar mati) — evaluasi ulang jeda.
    PowerChanged,
    /// `config.toml` berubah di disk — muat ulang konfigurasi.
    ConfigChanged,
}

/// Handle jendela Win32 sebagai nilai opaque, agar modul lain tidak perlu
/// menyentuh tipe FFI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hwnd(isize);

impl Hwnd {
    /// Mengambil handle jendela dari jendela `winit`.
    pub fn from_winit(window: &winit::window::Window) -> Option<Self> {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        match window.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(handle) => Some(Hwnd(handle.hwnd.get())),
            _ => None,
        }
    }

    /// Konversi ke tipe FFI. Hanya dipakai di dalam modul `platform`.
    pub(crate) fn handle(self) -> windows::Win32::Foundation::HWND {
        windows::Win32::Foundation::HWND(self.0 as *mut core::ffi::c_void)
    }
}

/// Direktori cache ikon lokal: `%LOCALAPPDATA%\BubbleDesk`.
// ponytail: dipakai M4 untuk cache ikon disk
#[allow(dead_code)]
pub fn local_app_data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("BubbleDesk")
}

/// Direktori data aplikasi: `%APPDATA%\BubbleDesk`.
///
/// Variabel `BUBBLEDESK_DATA_DIR` dapat menimpanya; dipakai tes agar tidak
/// menyentuh data pengguna.
pub fn app_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("BUBBLEDESK_DATA_DIR") {
        return PathBuf::from(dir);
    }
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("BubbleDesk")
}

/// Waktu lokal saat ini sebagai `YYYY-MM-DD HH:MM:SS.mmm`, untuk baris log.
#[cfg(windows)]
pub fn now_local_string() -> String {
    use windows::Win32::System::SystemInformation::GetLocalTime;

    // SAFETY: GetLocalTime tidak menerima pointer dan mengembalikan nilai.
    let time = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        time.wYear,
        time.wMonth,
        time.wDay,
        time.wHour,
        time.wMinute,
        time.wSecond,
        time.wMilliseconds
    )
}

/// Informasi penggunaan memori proses (spec §14).
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessMemory {
    pub working_set_bytes: u64,
    pub private_bytes: u64,
}

/// Mengambil working set dan private bytes proses saat ini.
#[cfg(windows)]
pub fn current_process_memory() -> ProcessMemory {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::GetCurrentProcess;

    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    // SAFETY: GetCurrentProcess valid untuk proses berjalan, counters dialokasikan di stack.
    let ok = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    if ok.is_ok() {
        ProcessMemory {
            working_set_bytes: counters.WorkingSetSize as u64,
            private_bytes: counters.PagefileUsage as u64,
        }
    } else {
        ProcessMemory::default()
    }
}

#[cfg(not(windows))]
pub fn current_process_memory() -> ProcessMemory {
    ProcessMemory::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn process_memory_returns_nonzero_working_set() {
        let mem = current_process_memory();
        assert!(
            mem.working_set_bytes > 0,
            "working set harus lebih dari 0 pada proses berjalan"
        );
    }
}
