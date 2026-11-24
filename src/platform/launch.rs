//! Peluncuran item desktop (`ShellExecuteExW`), buka lokasi di Explorer,
//! dan salin path ke clipboard (spec §4 FR-3, §12, §13).

#![cfg(windows)]

use crate::platform::icons::ComGuard;
use crate::platform::{Hwnd, SystemEvent};
use crate::strings;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, GlobalFree, ERROR_ACCESS_DENIED, ERROR_CANCELLED, ERROR_FILE_NOT_FOUND,
    ERROR_PATH_NOT_FOUND, HANDLE, HGLOBAL, HWND, MAX_PATH,
};
use windows::Win32::System::Com::{
    CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Shell::{
    IShellLinkW, ShellExecuteExW, ShellLink, SEE_MASK_FLAG_NO_UI, SHELLEXECUTEINFOW, SLGP_RAWPATH,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use winit::event_loop::EventLoopProxy;

/// CF_UNICODETEXT = 13 (Win32 clipboard format).
const CF_UNICODETEXT_FMT: u32 = 13;

/// Permintaan peluncuran yang dikirim ke thread worker.
struct LaunchRequest {
    id: String,
    path: PathBuf,
}

/// Hasil peluncuran item yang dikembalikan ke main thread.
#[derive(Debug, Clone)]
pub struct LaunchResult {
    pub id: String,
    pub ok: bool,
    pub error_msg: Option<String>,
    pub cancelled: bool,
}

/// Worker peluncur item di latar belakang (thread COM STA tunggal).
pub struct Launcher {
    tx: Sender<LaunchRequest>,
    pub rx: Receiver<LaunchResult>,
}

impl Launcher {
    pub fn start(proxy: EventLoopProxy<SystemEvent>) -> Self {
        let (req_tx, req_rx) = mpsc::channel::<LaunchRequest>();
        let (res_tx, res_rx) = mpsc::channel::<LaunchResult>();

        let _ = std::thread::Builder::new()
            .name("BubbleDeskLauncher".to_owned())
            .spawn(move || {
                let _com = match ComGuard::init() {
                    Ok(g) => Some(g),
                    Err(e) => {
                        log_warn!("COM init gagal di thread launcher: {e}");
                        None
                    }
                };
                while let Ok(req) = req_rx.recv() {
                    let result = execute_item(&req.id, &req.path);
                    if res_tx.send(result).is_err() {
                        break;
                    }
                    let _ = proxy.send_event(SystemEvent::WakeUp);
                }
            });

        Self {
            tx: req_tx,
            rx: res_rx,
        }
    }

    /// Meminta peluncuran satu item secara non-blocking.
    pub fn launch(&self, id: String, path: PathBuf) {
        if let Err(e) = self.tx.send(LaunchRequest { id, path }) {
            log_warn!("gagal mengirim permintaan peluncuran: {e}");
        }
    }
}

fn execute_item(id: &str, path: &Path) -> LaunchResult {
    if !path.exists() {
        return LaunchResult {
            id: id.to_owned(),
            ok: false,
            error_msg: Some(strings::ERR_FILE_NOT_FOUND.to_owned()),
            cancelled: false,
        };
    }

    // Jika shortcut (.lnk) menunjuk ke target filesystem yang sudah dihapus,
    // deteksi lebih awal karena ShellExecuteExW sering gagal diam-diam / ERROR_CANCELLED.
    if is_lnk_with_missing_target(path) {
        log_warn!("target shortcut .lnk tidak ditemukan: {}", path.display());
        return LaunchResult {
            id: id.to_owned(),
            ok: false,
            error_msg: Some(strings::ERR_PATH_NOT_FOUND.to_owned()),
            cancelled: false,
        };
    }

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut sei = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_FLAG_NO_UI,
        lpVerb: PCWSTR::null(),
        lpFile: PCWSTR(wide.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };

    // SAFETY: wide hidup selama pemanggilan ShellExecuteExW; cbSize terisi benar.
    let res = unsafe { ShellExecuteExW(&mut sei) };
    match res {
        Ok(()) => LaunchResult {
            id: id.to_owned(),
            ok: true,
            error_msg: None,
            cancelled: false,
        },
        Err(e) => {
            // SAFETY: GetLastError membaca kode error thread saat ini.
            let win_err = unsafe { GetLastError() };
            let code = if win_err.0 != 0 {
                win_err.0
            } else {
                (e.code().0 as u32) & 0xFFFF
            };
            if code == ERROR_CANCELLED.0 {
                return LaunchResult {
                    id: id.to_owned(),
                    ok: false,
                    error_msg: None,
                    cancelled: true,
                };
            }
            let msg = match code {
                c if c == ERROR_FILE_NOT_FOUND.0 => strings::ERR_FILE_NOT_FOUND.to_owned(),
                c if c == ERROR_PATH_NOT_FOUND.0 => strings::ERR_PATH_NOT_FOUND.to_owned(),
                c if c == ERROR_ACCESS_DENIED.0 => strings::ERR_ACCESS_DENIED.to_owned(),
                c => strings::format_launch_error_code(c),
            };
            log_warn!(
                "ShellExecuteExW gagal ({}): {} [kode={code}]",
                path.display(),
                msg
            );
            LaunchResult {
                id: id.to_owned(),
                ok: false,
                error_msg: Some(msg),
                cancelled: false,
            }
        }
    }
}

/// Membuka Windows Explorer dan memilih berkas target (`explorer.exe /select,"<path>"`).
pub fn open_file_location(path: &Path) -> Result<(), String> {
    let arg = format!("/select,\"{}\"", path.display());
    std::process::Command::new("explorer.exe")
        .raw_arg(arg)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("gagal membuka Explorer: {e}"))
}

/// Membuka direktori data BubbleDesk di Windows Explorer.
pub fn open_folder(dir: &Path) -> Result<(), String> {
    std::process::Command::new("explorer.exe")
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("gagal membuka folder: {e}"))
}

/// Menyalin teks (path) ke clipboard Windows sebagai `CF_UNICODETEXT`.
pub fn copy_text_to_clipboard(owner: Option<Hwnd>, text: &str) -> Result<(), String> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let byte_len = wide.len() * std::mem::size_of::<u16>();
    let owner_hwnd: Option<HWND> = owner.map(|h| h.handle());

    // SAFETY: pengelolaan clipboard dan memori global HGLOBAL mengikuti kontrak Win32:
    // setelah SetClipboardData sukses, kepemilikan HGLOBAL berpindah ke sistem.
    unsafe {
        OpenClipboard(owner_hwnd).map_err(|e| format!("OpenClipboard gagal: {e}"))?;

        let result = (|| -> Result<(), String> {
            EmptyClipboard().map_err(|e| format!("EmptyClipboard gagal: {e}"))?;
            let hmem: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, byte_len)
                .map_err(|e| format!("GlobalAlloc gagal: {e}"))?;
            let ptr = GlobalLock(hmem) as *mut u16;
            if ptr.is_null() {
                let _ = GlobalFree(Some(hmem));
                return Err("GlobalLock gagal".to_owned());
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            let _ = GlobalUnlock(hmem);

            if let Err(e) = SetClipboardData(CF_UNICODETEXT_FMT, Some(HANDLE(hmem.0))) {
                let _ = GlobalFree(Some(hmem));
                return Err(format!("SetClipboardData gagal: {e}"));
            }
            Ok(())
        })();

        let _ = CloseClipboard();
        result
    }
}

/// Memeriksa apakah berkas `.lnk` menunjuk ke target filesystem yang tidak lagi ada.
fn is_lnk_with_missing_target(lnk_path: &Path) -> bool {
    let is_lnk = lnk_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("lnk"));
    if !is_lnk {
        return false;
    }

    let wide: Vec<u16> = lnk_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: COM STA telah diinisialisasi di thread launcher (ComGuard).
    // IShellLinkW dan IPersistFile dikelola oleh smart pointer COM windows-rs.
    unsafe {
        let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER)
        else {
            return false;
        };
        let Ok(persist) = link.cast::<IPersistFile>() else {
            return false;
        };
        if persist.Load(PCWSTR(wide.as_ptr()), STGM_READ).is_err() {
            return false;
        }
        let mut target_buf = [0u16; MAX_PATH as usize];
        if link
            .GetPath(&mut target_buf, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
            .is_err()
        {
            return false;
        }
        let end = target_buf
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(target_buf.len());
        if end == 0 {
            return false;
        }
        let target_str = String::from_utf16_lossy(&target_buf[..end]);
        let target = Path::new(&target_str);
        // Hanya verifikasi target yang jelas berupa path filesystem lokal/UNC.
        if (target.is_absolute() || target_str.starts_with(r"\\")) && !target.exists() {
            return true;
        }
    }
    false
}
