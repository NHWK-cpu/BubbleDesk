//! Menu konteks Win32 (`TrackPopupMenu`, dipicu Ctrl + klik kiri) dan dialog ganti
//! nama item ringan (spec §12, AGENTS §9).

#![cfg(windows)]

use crate::platform::Hwnd;
use crate::strings;
use std::cell::RefCell;
use std::mem::size_of;
use std::sync::OnceLock;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetDC, GetDeviceCaps, GetStockObject, ReleaseDC, DEFAULT_GUI_FONT, LOGPIXELSX,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetActiveWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, BringWindowToTop, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DestroyWindow, DispatchMessageW, GetForegroundWindow, GetMessageW, GetSystemMetrics,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsDialogMessageW, PostMessageW,
    PostQuitMessage, RegisterClassExW, SendMessageW, SetForegroundWindow,
    SetLayeredWindowAttributes, SetWindowPos, ShowWindow, TrackPopupMenu, TranslateMessage,
    CW_USEDEFAULT, HMENU, HWND_TOPMOST, LWA_ALPHA, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG,
    SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOSIZE, SW_HIDE, SW_SHOW, TPM_LEFTALIGN,
    TPM_LEFTBUTTON, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TPM_TOPALIGN, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_INITDIALOG, WM_NULL, WM_SETFONT, WNDCLASSEXW, WS_BORDER,
    WS_CAPTION, WS_CHILD, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};

// ID item menu konteks.
const CMD_OPEN: usize = 1001;
const CMD_OPEN_LOCATION: usize = 1002;
const CMD_COPY_PATH: usize = 1003;
const CMD_RENAME_LABEL: usize = 1004;
const CMD_RESET_SCORE: usize = 1005;
const CMD_UNPIN: usize = 1006;
const CMD_HIDE: usize = 1007;

// ID kontrol dialog ganti label.
const ID_EDIT_LABEL: usize = 2001;
const ID_BTN_OK: usize = 1; // IDOK
const ID_BTN_CANCEL: usize = 2; // IDCANCEL
const ES_AUTOHSCROLL_STYLE: u32 = 0x0080;
const BS_DEFPUSHBUTTON_STYLE: u32 = 0x0001;
const BS_PUSHBUTTON_STYLE: u32 = 0x0000;
const EM_SETSEL_MSG: u32 = 0x00B1;

/// Aksi yang dipilih pengguna dari menu konteks bubble (Ctrl + klik kiri).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextAction {
    Open,
    OpenLocation,
    CopyPath,
    RenameLabel,
    ResetScore,
    Unpin,
    Hide,
}

/// Jendela helper tersembunyi sebagai owner `TrackPopupMenu` (spec §12, AGENTS §9).
pub struct MenuHelper {
    hwnd: HWND,
}

impl MenuHelper {
    pub fn new() -> Result<Self, String> {
        static CLASS_REG: OnceLock<()> = OnceLock::new();
        // SAFETY: pendaftaran kelas jendela dan pembuatan jendela helper 0x0 tak terlihat.
        unsafe {
            let hinst =
                GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW gagal: {e}"))?;
            let hinst_val = HINSTANCE(hinst.0);

            CLASS_REG.get_or_init(|| {
                let class = WNDCLASSEXW {
                    cbSize: size_of::<WNDCLASSEXW>() as u32,
                    lpfnWndProc: Some(helper_wndproc),
                    hInstance: hinst_val,
                    lpszClassName: w!("BubbleDeskMenuHelper"),
                    ..Default::default()
                };
                let _ = RegisterClassExW(&class);
            });

            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT,
                w!("BubbleDeskMenuHelper"),
                w!("BubbleDeskMenu"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(hinst_val),
                None,
            )
            .map_err(|e| format!("CreateWindowExW helper gagal: {e}"))?;

            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
            let _ = ShowWindow(hwnd, SW_HIDE);
            Ok(Self { hwnd })
        }
    }

    pub fn hwnd(&self) -> Hwnd {
        Hwnd(self.hwnd.0 as isize)
    }

    /// Menampilkan menu konteks di koordinat layar fisik `(screen_x, screen_y)`.
    pub fn show_context_menu(
        &self,
        screen_x: i32,
        screen_y: i32,
        is_pinned: bool,
    ) -> Option<ContextAction> {
        // SAFETY: self.hwnd valid; seluruh handle menu dibersihkan lewat DestroyMenu.
        unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                return None;
            };

            append_str(menu, CMD_OPEN, strings::MENU_OPEN, true);
            append_str(menu, CMD_OPEN_LOCATION, strings::MENU_OPEN_LOCATION, true);
            append_str(menu, CMD_COPY_PATH, strings::MENU_COPY_PATH, true);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            append_str(menu, CMD_RENAME_LABEL, strings::MENU_RENAME_LABEL, true);
            append_str(menu, CMD_RESET_SCORE, strings::MENU_RESET_SCORE, true);
            append_str(menu, CMD_UNPIN, strings::MENU_UNPIN, is_pinned);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            append_str(menu, CMD_HIDE, strings::MENU_HIDE, true);

            // Posisikan jendela helper 1x1 transparan agar dapat menjadi foreground owner
            // yang sah bagi TrackPopupMenu (menjamin menu tertutup saat klik di luar).
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                screen_x,
                screen_y,
                1,
                1,
                SWP_NOSIZE | SWP_NOACTIVATE,
            );
            let _ = ShowWindow(self.hwnd, SW_SHOW);

            let fg = GetForegroundWindow();
            let cur_tid = GetCurrentThreadId();
            let fg_tid = if !fg.0.is_null() && fg != self.hwnd {
                GetWindowThreadProcessId(fg, None)
            } else {
                0
            };
            let attached = if fg_tid != 0 && fg_tid != cur_tid {
                AttachThreadInput(cur_tid, fg_tid, true).as_bool()
            } else {
                false
            };

            // AGENTS §9: wajib SetForegroundWindow sebelum TrackPopupMenu & WM_NULL sesudahnya.
            let _ = SetForegroundWindow(self.hwnd);
            let _ = BringWindowToTop(self.hwnd);
            let _ = SetActiveWindow(self.hwnd);

            if attached {
                let _ = AttachThreadInput(cur_tid, fg_tid, false);
            }

            let cmd = TrackPopupMenu(
                menu,
                TPM_RETURNCMD
                    | TPM_LEFTBUTTON
                    | TPM_RIGHTBUTTON
                    | TPM_LEFTALIGN
                    | TPM_TOPALIGN
                    | TPM_NONOTIFY,
                screen_x,
                screen_y,
                Some(0),
                self.hwnd,
                None,
            );
            let _ = PostMessageW(Some(self.hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            let _ = DestroyMenu(menu);

            match cmd.0 as usize {
                CMD_OPEN => Some(ContextAction::Open),
                CMD_OPEN_LOCATION => Some(ContextAction::OpenLocation),
                CMD_COPY_PATH => Some(ContextAction::CopyPath),
                CMD_RENAME_LABEL => Some(ContextAction::RenameLabel),
                CMD_RESET_SCORE => Some(ContextAction::ResetScore),
                CMD_UNPIN => Some(ContextAction::Unpin),
                CMD_HIDE => Some(ContextAction::Hide),
                _ => None,
            }
        }
    }
}

impl Drop for MenuHelper {
    fn drop(&mut self) {
        // SAFETY: self.hwnd dibuat oleh MenuHelper dan dihancurkan sekali saat drop.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn helper_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: penerusan standar pesan Win32.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

unsafe fn append_str(menu: HMENU, id: usize, text: &str, enabled: bool) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let flags = if enabled {
        MF_STRING
    } else {
        MF_STRING | MF_GRAYED
    };
    // SAFETY: menu valid dan wide diakhiri null sepanjang pemanggilan.
    let _ = unsafe { AppendMenuW(menu, flags, id, PCWSTR(wide.as_ptr())) };
}

// ─── Dialog ganti nama item ringan ────────────────────────────────────────────

struct RenameState {
    edit_hwnd: HWND,
    accepted: bool,
    result_text: String,
}

thread_local! {
    static RENAME_STATE: RefCell<Option<RenameState>> = const { RefCell::new(None) };
}

/// Membuka dialog input ringan untuk mengganti nama file/folder bubble.
/// Mengembalikan:
/// - `None` bila dibatalkan,
/// - `Some(None)` bila dikosongkan (pemanggil mengabaikannya karena nama file tidak boleh kosong),
/// - `Some(Some(name))` bila pengguna menyimpan nama baru.
pub fn prompt_rename_label(
    owner: Option<Hwnd>,
    current_label: &str,
    screen_x: i32,
    screen_y: i32,
) -> Option<Option<String>> {
    static DLG_CLASS_REG: OnceLock<()> = OnceLock::new();

    // SAFETY: seluruh pembuatan jendela, kontrol anak, dan loop pesan modal berjalan di thread UI.
    unsafe {
        let hinst = GetModuleHandleW(None).ok()?;
        let hinst_val = HINSTANCE(hinst.0);

        DLG_CLASS_REG.get_or_init(|| {
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(rename_wndproc),
                hInstance: hinst_val,
                lpszClassName: w!("BubbleDeskRenameDlg"),
                hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(
                    (windows::Win32::Graphics::Gdi::COLOR_BTNFACE.0 + 1) as isize
                        as *mut core::ffi::c_void,
                ),
                ..Default::default()
            };
            let _ = RegisterClassExW(&class);
        });

        let scale = system_dpi_scale();
        let dlg_w = (360.0 * scale) as i32;
        let dlg_h = (145.0 * scale) as i32;
        let scr_w = GetSystemMetrics(SM_CXSCREEN).max(dlg_w);
        let scr_h = GetSystemMetrics(SM_CYSCREEN).max(dlg_h);
        let pos_x = (screen_x - dlg_w / 2).clamp(16, (scr_w - dlg_w - 16).max(16));
        let pos_y = (screen_y - dlg_h / 2).clamp(16, (scr_h - dlg_h - 16).max(16));

        let title_w = to_wide(strings::RENAME_DIALOG_TITLE);
        let dlg = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("BubbleDeskRenameDlg"),
            PCWSTR(title_w.as_ptr()),
            WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
            pos_x,
            pos_y,
            dlg_w,
            dlg_h,
            owner.map(|h| h.handle()),
            None,
            Some(hinst_val),
            None,
        )
        .ok()?;

        let font = GetStockObject(DEFAULT_GUI_FONT);
        let pad = (12.0 * scale) as i32;
        let client_w = dlg_w - (16.0 * scale) as i32;

        let prompt_w = to_wide(strings::RENAME_DIALOG_PROMPT);
        let lbl_hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            PCWSTR(prompt_w.as_ptr()),
            WS_CHILD | WS_VISIBLE,
            pad,
            pad,
            client_w - pad * 2,
            (20.0 * scale) as i32,
            Some(dlg),
            None,
            Some(hinst_val),
            None,
        )
        .ok();
        if let Some(h) = lbl_hwnd {
            let _ = SendMessageW(
                h,
                WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(1)),
            );
        }

        let cur_w = to_wide(current_label);
        let edit_hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EDIT"),
            PCWSTR(cur_w.as_ptr()),
            WINDOW_STYLE(
                WS_CHILD.0 | WS_VISIBLE.0 | WS_BORDER.0 | WS_TABSTOP.0 | ES_AUTOHSCROLL_STYLE,
            ),
            pad,
            (36.0 * scale) as i32,
            client_w - pad * 2,
            (24.0 * scale) as i32,
            Some(dlg),
            Some(HMENU(ID_EDIT_LABEL as *mut core::ffi::c_void)),
            Some(hinst_val),
            None,
        )
        .ok()?;
        let _ = SendMessageW(
            edit_hwnd,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );

        let btn_w = (84.0 * scale) as i32;
        let btn_h = (28.0 * scale) as i32;
        let btn_y = (72.0 * scale) as i32;
        let cancel_x = client_w - pad - btn_w;
        let ok_x = cancel_x - (8.0 * scale) as i32 - btn_w;

        let ok_w = to_wide(strings::RENAME_DIALOG_OK);
        if let Ok(ok_btn) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("BUTTON"),
            PCWSTR(ok_w.as_ptr()),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | BS_DEFPUSHBUTTON_STYLE),
            ok_x,
            btn_y,
            btn_w,
            btn_h,
            Some(dlg),
            Some(HMENU(ID_BTN_OK as *mut core::ffi::c_void)),
            Some(hinst_val),
            None,
        ) {
            let _ = SendMessageW(
                ok_btn,
                WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(1)),
            );
        }

        let cancel_w = to_wide(strings::RENAME_DIALOG_CANCEL);
        if let Ok(cancel_btn) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("BUTTON"),
            PCWSTR(cancel_w.as_ptr()),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | BS_PUSHBUTTON_STYLE),
            cancel_x,
            btn_y,
            btn_w,
            btn_h,
            Some(dlg),
            Some(HMENU(ID_BTN_CANCEL as *mut core::ffi::c_void)),
            Some(hinst_val),
            None,
        ) {
            let _ = SendMessageW(
                cancel_btn,
                WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(1)),
            );
        }

        RENAME_STATE.with(|s| {
            *s.borrow_mut() = Some(RenameState {
                edit_hwnd,
                accepted: false,
                result_text: String::new(),
            });
        });

        let _ = EnableWindow(dlg, true);
        let _ = ShowWindow(dlg, SW_SHOW);
        let _ = SetForegroundWindow(dlg);
        let _ = SetFocus(Some(edit_hwnd));
        let _ = SendMessageW(edit_hwnd, EM_SETSEL_MSG, Some(WPARAM(0)), Some(LPARAM(-1)));
        let _ = PostMessageW(Some(dlg), WM_INITDIALOG, WPARAM(0), LPARAM(0));

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if !IsDialogMessageW(dlg, &msg).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        let _ = DestroyWindow(dlg);

        let final_state = RENAME_STATE.with(|s| s.borrow_mut().take());
        match final_state {
            Some(st) if st.accepted => {
                let trimmed = st.result_text.trim();
                if trimmed.is_empty() {
                    Some(None)
                } else {
                    Some(Some(trimmed.to_owned()))
                }
            }
            _ => None,
        }
    }
}

unsafe extern "system" fn rename_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let cmd = wparam.0 & 0xFFFF;
            if cmd == ID_BTN_OK {
                RENAME_STATE.with(|s| {
                    if let Some(st) = s.borrow_mut().as_mut() {
                        st.accepted = true;
                        st.result_text = read_window_text(st.edit_hwnd);
                    }
                });
                // SAFETY: menutup loop modal lokal dialog.
                unsafe { PostQuitMessage(0) };
                return LRESULT(0);
            } else if cmd == ID_BTN_CANCEL {
                // SAFETY: menutup loop modal lokal dialog sebagai batal.
                unsafe { PostQuitMessage(0) };
                return LRESULT(0);
            }
        }
        WM_CLOSE => {
            // SAFETY: menutup loop modal lokal dialog sebagai batal.
            unsafe { PostQuitMessage(0) };
            return LRESULT(0);
        }
        _ => {}
    }
    // SAFETY: penerusan pesan standar Win32.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

unsafe fn read_window_text(hwnd: HWND) -> String {
    // SAFETY: GetWindowTextLengthW dan GetWindowTextW membaca buffer lokal dengan ukuran benar.
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; (len as usize) + 1];
        let copied = GetWindowTextW(hwnd, &mut buf);
        if copied <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..copied as usize])
    }
}

fn system_dpi_scale() -> f32 {
    // SAFETY: GetDC(None) mengambil DC layar untuk membaca LOGPIXELSX dan langsung dilepas.
    unsafe {
        let hdc = GetDC(None);
        if hdc.is_invalid() {
            return 1.0;
        }
        let dpi = GetDeviceCaps(Some(hdc), LOGPIXELSX);
        let _ = ReleaseDC(None, hdc);
        (dpi as f32 / 96.0).clamp(1.0, 3.0)
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[allow(dead_code)]
const _DEFAULT_CW: i32 = CW_USEDEFAULT;
