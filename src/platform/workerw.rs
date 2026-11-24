//! Integrasi desktop Windows: attach jendela ke `WorkerW` (spec §7).
//!
//! Teknik ini tidak terdokumentasi resmi, jadi setiap langkah bersifat defensif:
//! deteksi struktur saat runtime, logging jelas, dan fallback (FR-15).
//! Layout B (Windows 11 24H2+) baru dideteksi untuk memberi peringatan; dukungan
//! penuh menyusul setelah M5.

use crate::platform::{Hwnd, SystemEvent};
use std::mem::size_of;
use std::sync::OnceLock;
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, FillRect, GetDC, GetMonitorInfoW, MonitorFromPoint, ReleaseDC,
    HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, EnumWindows, FindWindowExW, FindWindowW, GetClassNameW,
    GetClientRect, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, GetWindowTextW, IsWindow,
    IsWindowVisible, RegisterClassExW, RegisterWindowMessageW, SendMessageTimeoutW, SetParent,
    SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, GWL_STYLE, HWND_BOTTOM, SMTO_NORMAL, SM_CXSCREEN,
    SM_CYSCREEN, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WNDCLASSEXW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_CLIPSIBLINGS, WS_DLGFRAME,
    WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX,
    WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME, WS_VISIBLE,
};
use winit::event_loop::EventLoopProxy;

/// Pesan tak berdokumentasi yang membuat Explorer membuat jendela `WorkerW` (§7.2).
const SPAWN_WORKERW_MESSAGE: u32 = 0x052C;
/// Kedalaman maksimum saat mencetak pohon jendela.
const DUMP_MAX_DEPTH: usize = 8;

/// Proxy loop event yang dipakai wndproc helper (diisi sekali per proses).
static PROXY: OnceLock<EventLoopProxy<SystemEvent>> = OnceLock::new();
/// ID pesan `TaskbarCreated` yang di-register sekali.
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

/// Hasil percobaan attach ke desktop.
pub struct AttachResult {
    /// `true` bila jendela berhasil menjadi child `WorkerW`.
    pub attached: bool,
    /// Keterangan singkat untuk log.
    pub detail: String,
}

/// Cek apakah handle jendela masih hidup.
pub fn is_alive(hwnd: Hwnd) -> bool {
    // SAFETY: IsWindow hanya memeriksa validitas handle, tidak menyentuh memori kita.
    unsafe { IsWindow(Some(hwnd.handle())).as_bool() }
}

/// Menempatkan jendela menutupi monitor primer (piksel fisik).
pub fn place_primary_monitor(hwnd: Hwnd) {
    let rect = primary_monitor_rect();
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    // SAFETY: hwnd valid milik proses ini; koordinat relatif terhadap parent.
    let _ = unsafe {
        SetWindowPos(
            hwnd.handle(),
            None,
            rect.left,
            rect.top,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
        )
    };
}

/// Mengisi seluruh klien jendela dengan satu warna (`colorref` = 0x00BBGGRR).
// ponytail: fallback GDI solid fill dari M1, dipertahankan bila pengujian tanpa GPU diperlukan.
#[allow(dead_code)]
pub fn fill_solid(hwnd: Hwnd, colorref: u32) {
    let raw = hwnd.handle();
    // SAFETY: penanganan GDI lokal; semua handle dilepas pada akhir fungsi.
    unsafe {
        let hdc = GetDC(Some(raw));
        if hdc.is_invalid() {
            return;
        }
        let mut rc = RECT::default();
        let _ = GetClientRect(raw, &mut rc);
        let brush = CreateSolidBrush(COLORREF(colorref));
        if !brush.is_invalid() {
            FillRect(hdc, &rc, brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }
        ReleaseDC(Some(raw), hdc);
    }
}

/// Mencoba menjadikan jendela sebagai child `WorkerW` (Layout A).
pub fn attach(hwnd: Hwnd) -> AttachResult {
    let raw = hwnd.handle();
    let Some(workerw) = find_workerw() else {
        return AttachResult {
            attached: false,
            detail: "WorkerW tidak ditemukan".to_owned(),
        };
    };

    if has_foreign_child(workerw, raw) {
        log_warn!("WorkerW sudah berisi jendela lain (aplikasi wallpaper lain); tidak ditimpa");
        return AttachResult {
            attached: false,
            detail: "WorkerW milik aplikasi lain".to_owned(),
        };
    }

    // Larang aktivasi SEBELUM SetParent: tanpa ini SetParent menjadikan jendela kita
    // sebagai jendela foreground (mencuri fokus sekaligus memicu border jendela aktif).
    set_no_activate(raw);
    // SAFETY: raw valid; workerw adalah jendela yang ditemukan lewat enumerasi.
    if unsafe { SetParent(raw, Some(workerw)) }.is_err() {
        return AttachResult {
            attached: false,
            detail: "SetParent gagal".to_owned(),
        };
    }
    apply_child_style(raw);

    AttachResult {
        attached: true,
        detail: format!(
            "parent={} hwnd={:#x}",
            class_name(workerw),
            workerw.0 as isize
        ),
    }
}

/// Mode kompatibilitas (FR-15): jendela top-level borderless, didorong ke bawah
/// semua jendela. FR-15 menduga mode ini tersembunyi saat Win+D, tetapi pada
/// Windows 10 22H2 dengan gaya ini jendela justru tetap tampil
/// (lihat `docs/desktop-tree-notes.md`).
pub fn attach_compat(hwnd: Hwnd) {
    let raw = hwnd.handle();
    // SAFETY: raw valid; melepas ke top-level lalu mengubah style.
    unsafe {
        let _ = SetParent(raw, None);
        let style = GetWindowLongPtrW(raw, GWL_STYLE) as u32;
        let new_style = (style & !WS_CHILD.0) | WS_POPUP.0 | WS_VISIBLE.0;
        SetWindowLongPtrW(raw, GWL_STYLE, new_style as isize);
        let ex = GetWindowLongPtrW(raw, GWL_EXSTYLE) as u32;
        SetWindowLongPtrW(
            raw,
            GWL_EXSTYLE,
            (ex | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) as isize,
        );
        let rect = primary_monitor_rect();
        let _ = SetWindowPos(
            raw,
            Some(HWND_BOTTOM),
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
        );
    }
}

/// Memasang jendela helper tersembunyi untuk menerima broadcast `TaskbarCreated`.
pub fn install_taskbar_listener(proxy: EventLoopProxy<SystemEvent>) -> windows::core::Result<()> {
    let _ = PROXY.set(proxy);
    // SAFETY: pendaftaran kelas dan pembuatan satu jendela helper; handle-nya
    // sengaja dibiarkan hidup selama proses berjalan.
    unsafe {
        let hinstance = GetModuleHandleW(None)?;
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(listener_proc),
            hInstance: HINSTANCE(hinstance.0),
            lpszClassName: w!("BubbleDeskListener"),
            ..Default::default()
        };
        let _ = RegisterClassExW(&class);
        let _listener = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("BubbleDeskListener"),
            w!("BubbleDesk"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(HINSTANCE(hinstance.0)),
            None,
        )?;
    }
    Ok(())
}

/// Mencetak pohon jendela desktop ke stdout (untuk `--dump-desktop-tree`).
pub fn dump_desktop_tree() {
    println!("== Pohon jendela desktop (top-level dan turunannya) ==");
    // SAFETY: callback hanya membaca properti jendela dan mencetak.
    let _ = unsafe { EnumWindows(Some(dump_top_level), LPARAM(0)) };
}

fn find_workerw() -> Option<HWND> {
    // SAFETY: seluruh blok hanya memanggil Win32 dengan handle yang valid.
    unsafe {
        let progman = FindWindowW(w!("Progman"), PCWSTR::null()).ok()?;

        // Minta Explorer membuat WorkerW di belakang ikon.
        let sent = SendMessageTimeoutW(
            progman,
            SPAWN_WORKERW_MESSAGE,
            WPARAM(0xD),
            LPARAM(0x1),
            SMTO_NORMAL,
            1000,
            None,
        );
        if sent.0 == 0 {
            let _ = SendMessageTimeoutW(
                progman,
                SPAWN_WORKERW_MESSAGE,
                WPARAM(0xD),
                LPARAM(0x0),
                SMTO_NORMAL,
                1000,
                None,
            );
        }

        if let Some(workerw) = find_workerw_layout_a() {
            return Some(workerw);
        }

        // Layout B (Windows 11 24H2+): WorkerW menjadi child Progman. Belum didukung.
        if FindWindowExW(Some(progman), None, w!("WorkerW"), PCWSTR::null()).is_ok() {
            log_warn!(
                "Layout B (WorkerW child Progman) terdeteksi; belum didukung sampai setelah M5"
            );
        }
        None
    }
}

fn find_workerw_layout_a() -> Option<HWND> {
    let mut ctx = LayoutACtx { found: None };
    // SAFETY: lparam menunjuk ke ctx lokal yang hidup selama pemanggilan.
    let _ = unsafe {
        EnumWindows(
            Some(layout_a_proc),
            LPARAM(&mut ctx as *mut LayoutACtx as isize),
        )
    };
    ctx.found
}

struct LayoutACtx {
    found: Option<HWND>,
}

unsafe extern "system" fn layout_a_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = unsafe { &mut *(lparam.0 as *mut LayoutACtx) };
    // Cari top-level yang memuat SHELLDLL_DefView, lalu WorkerW sesudahnya.
    let has_defview =
        unsafe { FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), PCWSTR::null()) }.is_ok();
    if has_defview {
        if let Ok(workerw) =
            unsafe { FindWindowExW(None, Some(hwnd), w!("WorkerW"), PCWSTR::null()) }
        {
            ctx.found = Some(workerw);
            return BOOL(0);
        }
    }
    BOOL(1)
}

unsafe extern "system" fn dump_top_level(hwnd: HWND, _lparam: LPARAM) -> BOOL {
    print_window(hwnd, 0);
    BOOL(1)
}

unsafe extern "system" fn listener_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let taskbar =
        *TASKBAR_CREATED.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) });
    if taskbar != 0 && msg == taskbar {
        if let Some(proxy) = PROXY.get() {
            let _ = proxy.send_event(SystemEvent::TaskbarCreated);
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn set_no_activate(hwnd: HWND) {
    // SAFETY: hwnd valid; hanya mengubah ex-style jendela kita sendiri.
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            (ex | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize,
        );
    }
}

fn apply_child_style(hwnd: HWND) {
    // SAFETY: hwnd valid; hanya mengubah gaya jendela kita sendiri.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        // winit memasang WS_CAPTION | WS_BORDER secara default; bit bingkai/judul itu
        // harus dibuang agar tidak ada border, lalu jendela dijadikan child.
        let frames = WS_CAPTION.0
            | WS_BORDER.0
            | WS_DLGFRAME.0
            | WS_THICKFRAME.0
            | WS_SYSMENU.0
            | WS_MINIMIZEBOX.0
            | WS_MAXIMIZEBOX.0
            | WS_POPUP.0;
        let new_style = (style & !frames) | WS_CHILD.0 | WS_VISIBLE.0 | WS_CLIPSIBLINGS.0;
        SetWindowLongPtrW(hwnd, GWL_STYLE, new_style as isize);

        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        // WS_EX_WINDOWEDGE menggambar tepi 3D; WS_EX_APPWINDOW memengaruhi taskbar/
        // foreground. Keduanya dibuang, dan jendela dilarang aktif.
        let ex_frames = WS_EX_WINDOWEDGE.0 | WS_EX_APPWINDOW.0;
        let new_ex = (ex & !ex_frames) | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex as isize);
    }
}

fn has_foreign_child(parent: HWND, ours: HWND) -> bool {
    let mut after: Option<HWND> = None;
    // SAFETY: parent valid; enumerasi child langsung.
    while let Ok(child) =
        unsafe { FindWindowExW(Some(parent), after, PCWSTR::null(), PCWSTR::null()) }
    {
        if child != ours {
            return true;
        }
        after = Some(child);
    }
    false
}

/// Ukuran monitor utama (width, height) dalam piksel fisik.
pub fn primary_monitor_size() -> (u32, u32) {
    let rect = primary_monitor_rect();
    (
        (rect.right - rect.left).max(1) as u32,
        (rect.bottom - rect.top).max(1) as u32,
    )
}

/// Titik asal monitor utama `(left, top)` dalam koordinat layar virtual.
pub fn primary_monitor_origin() -> (i32, i32) {
    let rect = primary_monitor_rect();
    (rect.left, rect.top)
}

/// Batas area kerja (`rcWork`, bebas taskbar) relatif terhadap pojok kiri-atas
/// monitor utama: `(work_min, work_max)` dalam piksel fisik.
pub fn primary_work_area_bounds() -> ([f32; 2], [f32; 2]) {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: pointer ke struct lokal yang valid.
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let ox = info.rcMonitor.left;
            let oy = info.rcMonitor.top;
            let min_x = (info.rcWork.left - ox).max(0) as f32;
            let min_y = (info.rcWork.top - oy).max(0) as f32;
            let max_x = (info.rcWork.right - ox).max(1) as f32;
            let max_y = (info.rcWork.bottom - oy).max(1) as f32;
            ([min_x, min_y], [max_x, max_y])
        } else {
            let (w, h) = primary_monitor_size();
            ([0.0, 0.0], [w as f32, h as f32])
        }
    }
}

fn primary_monitor_rect() -> RECT {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: pointer ke struct lokal yang valid.
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            info.rcMonitor
        } else {
            RECT {
                left: 0,
                top: 0,
                right: GetSystemMetrics(SM_CXSCREEN),
                bottom: GetSystemMetrics(SM_CYSCREEN),
            }
        }
    }
}

fn print_window(hwnd: HWND, depth: usize) {
    let indent = "  ".repeat(depth);
    // SAFETY: pencarian properti jendela yang valid.
    let (visible, rect, title) = unsafe {
        let mut rc = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rc);
        (IsWindowVisible(hwnd).as_bool(), rc, window_title(hwnd))
    };
    println!(
        "{indent}{} hwnd={:#x} visible={visible} rect=({},{},{},{}) title={title:?}",
        class_name(hwnd),
        hwnd.0 as isize,
        rect.left,
        rect.top,
        rect.right,
        rect.bottom
    );

    if depth >= DUMP_MAX_DEPTH {
        return;
    }
    let mut after: Option<HWND> = None;
    // SAFETY: hwnd valid; enumerasi child langsung.
    while let Ok(child) =
        unsafe { FindWindowExW(Some(hwnd), after, PCWSTR::null(), PCWSTR::null()) }
    {
        print_window(child, depth + 1);
        after = Some(child);
    }
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    // SAFETY: buffer lokal valid dengan panjang yang benar.
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..len as usize])
}

fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    // SAFETY: buffer lokal valid dengan panjang yang benar.
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..len as usize])
}
