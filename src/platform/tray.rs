//! Ikon tray Windows (`Shell_NotifyIconW`) beserta menu klik-kanan (spec §7.4, M5).

#![cfg(windows)]

use crate::platform::SystemEvent;
use crate::strings;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    GetCursorPos, LoadIconW, PostMessageW, RegisterClassExW, SetForegroundWindow, TrackPopupMenu,
    HICON, HMENU, IDI_APPLICATION, MF_SEPARATOR, MF_STRING, TPM_LEFTALIGN, TPM_NONOTIFY,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TPM_TOPALIGN, WM_APP, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP,
    WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};
use winit::event_loop::EventLoopProxy;

const TRAY_ICON_ID: u32 = 1;
const TRAY_CALLBACK_MSG: u32 = WM_APP + 1;

const CMD_TOGGLE_ICONS: usize = 101;
const CMD_TOGGLE_PAUSE: usize = 102;
const CMD_TOGGLE_OVERLAY: usize = 103;
const CMD_OPEN_DATA: usize = 104;
const CMD_QUIT: usize = 105;

/// Aksi yang dipilih pengguna dari menu ikon tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    ToggleNativeIcons,
    TogglePause,
    ToggleOverlay,
    OpenDataFolder,
    Quit,
}

static PROXY: OnceLock<EventLoopProxy<SystemEvent>> = OnceLock::new();
static STATE_PAUSED: AtomicBool = AtomicBool::new(false);
static STATE_ICONS_HIDDEN: AtomicBool = AtomicBool::new(false);
static STATE_OVERLAY: AtomicBool = AtomicBool::new(false);

/// Pengelola ikon tray. Menghapus ikon tray & jendelanya saat di-drop.
pub struct Tray {
    hwnd: HWND,
}

impl Tray {
    /// Membuat jendela helper tersembunyi dan memasang ikon tray.
    pub fn new(proxy: EventLoopProxy<SystemEvent>) -> Result<Self, String> {
        let _ = PROXY.set(proxy);

        // SAFETY: pendaftaran kelas, pembuatan jendela helper, dan pendaftaran Shell_NotifyIconW.
        unsafe {
            let hinst = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW: {e}"))?;
            let hinst_val = HINSTANCE(hinst.0);
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(tray_wndproc),
                hInstance: hinst_val,
                lpszClassName: w!("BubbleDeskTray"),
                ..Default::default()
            };
            let _ = RegisterClassExW(&class);

            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("BubbleDeskTray"),
                w!("BubbleDeskTray"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(hinst_val),
                None,
            )
            .map_err(|e| format!("CreateWindowExW(tray): {e}"))?;

            let tray = Self { hwnd };
            tray.add_icon()?;
            Ok(tray)
        }
    }

    /// Memperbarui label status yang ditampilkan di menu tray.
    pub fn set_state(&self, paused: bool, icons_hidden: bool, overlay: bool) {
        STATE_PAUSED.store(paused, Ordering::Relaxed);
        STATE_ICONS_HIDDEN.store(icons_hidden, Ordering::Relaxed);
        STATE_OVERLAY.store(overlay, Ordering::Relaxed);
    }

    /// Memasang ulang ikon tray setelah Explorer restart (`TaskbarCreated`).
    pub fn recreate(&self) {
        if let Err(e) = self.add_icon() {
            log_warn!("gagal memasang ulang ikon tray: {e}");
        }
    }

    #[allow(clippy::manual_dangling_ptr)] // Resource ID integer Win32 (MAKEINTRESOURCEW(1))
    fn add_icon(&self) -> Result<(), String> {
        // SAFETY: hwnd valid; muat ikon aplikasi yang disematkan (resource ID 1),
        // fallback ke ikon standar sistem bila tidak tersedia.
        unsafe {
            let hinst = GetModuleHandleW(None).map(|h| HINSTANCE(h.0)).ok();
            let icon: HICON = hinst
                .and_then(|h| LoadIconW(Some(h), PCWSTR(1 as *const u16)).ok())
                .or_else(|| LoadIconW(None, IDI_APPLICATION).ok())
                .ok_or_else(|| "LoadIconW gagal memuat ikon tray".to_owned())?;
            let mut nid = NOTIFYICONDATAW {
                cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uID: TRAY_ICON_ID,
                uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
                uCallbackMessage: TRAY_CALLBACK_MSG,
                hIcon: icon,
                ..Default::default()
            };
            copy_tip(&mut nid.szTip, strings::TRAY_TOOLTIP);
            if !Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
                return Err("Shell_NotifyIconW(NIM_ADD) mengembalikan false".to_owned());
            }
            Ok(())
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        // SAFETY: menghapus ikon tray milik jendela ini dan menghancurkan jendelanya.
        unsafe {
            let nid = NOTIFYICONDATAW {
                cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uID: TRAY_ICON_ID,
                ..Default::default()
            };
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn copy_tip(buf: &mut [u16; 128], text: &str) {
    let mut i = 0;
    for unit in text.encode_utf16() {
        if i + 1 >= buf.len() {
            break;
        }
        buf[i] = unit;
        i += 1;
    }
    buf[i] = 0;
}

unsafe extern "system" fn tray_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == TRAY_CALLBACK_MSG {
        let ev = (lparam.0 & 0xFFFF) as u32;
        if ev == WM_RBUTTONUP || ev == WM_LBUTTONUP {
            // SAFETY: hwnd milik tray ini dan dipanggil di thread UI.
            unsafe { show_tray_menu(hwnd) };
            return LRESULT(0);
        }
    }
    // SAFETY: penerusan standar pesan Win32.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

unsafe fn show_tray_menu(hwnd: HWND) {
    // SAFETY: seluruh handle menu dibuat dan dihancurkan di dalam fungsi ini.
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };

        let icons_label = if STATE_ICONS_HIDDEN.load(Ordering::Relaxed) {
            strings::TRAY_SHOW_ICONS
        } else {
            strings::TRAY_HIDE_ICONS
        };
        let pause_label = if STATE_PAUSED.load(Ordering::Relaxed) {
            strings::TRAY_RESUME
        } else {
            strings::TRAY_PAUSE
        };
        let overlay_label = if STATE_OVERLAY.load(Ordering::Relaxed) {
            strings::TRAY_OVERLAY_HIDE
        } else {
            strings::TRAY_OVERLAY_SHOW
        };

        append_str(menu, CMD_TOGGLE_ICONS, icons_label);
        append_str(menu, CMD_TOGGLE_PAUSE, pause_label);
        append_str(menu, CMD_TOGGLE_OVERLAY, overlay_label);
        append_str(menu, CMD_OPEN_DATA, strings::TRAY_OPEN_DATA);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        append_str(menu, CMD_QUIT, strings::TRAY_QUIT);

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_LEFTALIGN | TPM_TOPALIGN | TPM_NONOTIFY,
            pt.x,
            pt.y,
            Some(0),
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);

        let action = match cmd.0 as usize {
            CMD_TOGGLE_ICONS => Some(TrayAction::ToggleNativeIcons),
            CMD_TOGGLE_PAUSE => Some(TrayAction::TogglePause),
            CMD_TOGGLE_OVERLAY => Some(TrayAction::ToggleOverlay),
            CMD_OPEN_DATA => Some(TrayAction::OpenDataFolder),
            CMD_QUIT => Some(TrayAction::Quit),
            _ => None,
        };
        if let Some(a) = action {
            if let Some(proxy) = PROXY.get() {
                let _ = proxy.send_event(SystemEvent::Tray(a));
            }
        }
    }
}

unsafe fn append_str(menu: HMENU, id: usize, text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: menu valid dan wide berakhiran null.
    let _ = unsafe { AppendMenuW(menu, MF_STRING, id, PCWSTR(wide.as_ptr())) };
}
