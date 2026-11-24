//! Ekstraksi ikon, nama tampilan, dan folder Desktop dari Windows Shell (spec §4, §13).
//!
//! Menggunakan `IShellItemImageFactory::GetImage` (96×96) sebagai jalur utama,
//! dengan fallback ke `SHGetFileInfoW` + `GetIconInfo` + `GetDIBits`.
//! Semua kode COM/GDI berjalan di worker thread (`CoInitializeEx` apartment-threaded).
//! Alpha: straight (tidak premultiplied) — konsisten dari sini sampai shader.

#![cfg(windows)]

use image::imageops::FilterType;
use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_PublicDesktop, IShellItemImageFactory, SHCreateItemFromParsingName,
    SHGetFileInfoW, SHGetKnownFolderPath, KNOWN_FOLDER_FLAG, SHFILEINFOW, SHGFI_DISPLAYNAME,
    SHGFI_FLAGS, SHGFI_ICON, SHGFI_LARGEICON, SIIGBF_RESIZETOFIT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, EnumWindows, FindWindowExW, GetIconInfo, IsWindowVisible, SendMessageW, ICONINFO,
    WM_COMMAND,
};

/// Ukuran slot ikon dalam piksel (spec §11).
pub const ICON_SIZE: u32 = crate::render::atlas::ICON_SLOT_SIZE;

/// Ikon yang sudah diekstrak: piksel RGBA 96×96, straight alpha.
pub struct ExtractedIcon {
    /// Lebar dan tinggi selalu `ICON_SIZE × ICON_SIZE × 4` byte.
    pub rgba: Vec<u8>,
}

/// Menginisialisasi COM untuk thread saat ini (apartment-threaded).
/// Kembalikan `ComGuard` yang memanggil `CoUninitialize` saat di-drop.
pub struct ComGuard;

impl ComGuard {
    pub fn init() -> Result<Self, String> {
        // SAFETY: CoInitializeEx aman dipanggil di awal thread worker;
        // S_FALSE (0x00000001) berarti sudah diinisialisasi sebelumnya (OK).
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr.is_ok() || hr.0 == 1 {
            Ok(ComGuard)
        } else {
            Err(format!("CoInitializeEx gagal: {hr:?}"))
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        // SAFETY: dipasangkan satu-satu dengan CoInitializeEx di thread yang sama.
        unsafe { CoUninitialize() };
    }
}

/// Mengambil path folder Desktop pengguna lewat `SHGetKnownFolderPath(FOLDERID_Desktop)`,
/// sehingga tetap akurat bila Desktop dialihkan ke OneDrive (FR-1).
pub fn desktop_dir() -> Option<PathBuf> {
    known_folder_path(&FOLDERID_Desktop).or_else(|| {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .map(|p| p.join("Desktop"))
    })
}

/// Mengambil path folder Public Desktop lewat `SHGetKnownFolderPath(FOLDERID_PublicDesktop)`.
pub fn public_desktop_dir() -> Option<PathBuf> {
    known_folder_path(&FOLDERID_PublicDesktop).or_else(|| {
        std::env::var_os("PUBLIC")
            .map(PathBuf::from)
            .map(|p| p.join("Desktop"))
    })
}

fn known_folder_path(folder_id: &windows::core::GUID) -> Option<PathBuf> {
    // SAFETY: folder_id adalah GUID statis yang valid; pointer hasil dibebaskan dengan CoTaskMemFree.
    let pwstr = unsafe { SHGetKnownFolderPath(folder_id, KNOWN_FOLDER_FLAG(0), None).ok()? };
    // SAFETY: pwstr adalah string UTF-16 null-terminated yang dialokasikan Shell.
    let path = unsafe { pwstr.to_string().ok() };
    // SAFETY: membebaskan memori yang dialokasikan oleh SHGetKnownFolderPath.
    unsafe { CoTaskMemFree(Some(pwstr.as_ptr().cast())) };
    path.map(PathBuf::from)
}

/// Mengambil nama tampilan Explorer untuk `path` via `SHGetFileInfoW(SHGFI_DISPLAYNAME)` (spec §13).
/// Mengikuti pengaturan Windows (mis. menyembunyikan ekstensi `.lnk` atau ekstensi umum).
pub fn shell_display_name(path: &Path) -> Option<String> {
    let wide: Vec<u16> = path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let mut fi = SHFILEINFOW::default();
    // SAFETY: wide null-terminated valid; fi dialokasikan di stack.
    let ret = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut fi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_DISPLAYNAME,
        )
    };
    if ret == 0 {
        return None;
    }
    let len = fi
        .szDisplayName
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(fi.szDisplayName.len());
    let name = String::from_utf16_lossy(&fi.szDisplayName[..len]);
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Mengekstrak ikon untuk `path` sebagai RGBA 96×96 (straight alpha).
/// Harus dipanggil di thread yang sudah `CoInitializeEx` (gunakan `ComGuard`).
pub fn extract_icon(path: &Path) -> Result<ExtractedIcon, String> {
    match extract_via_shell_item(path) {
        Ok(icon) => return Ok(icon),
        Err(e) => {
            log_warn!(
                "IShellItemImageFactory gagal untuk {}: {e}; mencoba fallback",
                path.display()
            );
        }
    }
    extract_via_shgetfileinfo(path)
}

/// Jalur utama: `IShellItemImageFactory::GetImage` → `HBITMAP` → RGBA.
fn extract_via_shell_item(path: &Path) -> Result<ExtractedIcon, String> {
    let wide: Vec<u16> = path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: wide adalah string UTF-16 null-terminated valid.
    let shell_item: IShellItemImageFactory = unsafe {
        SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)
            .map_err(|e| format!("SHCreateItemFromParsingName: {e}"))?
    };

    let size = SIZE {
        cx: ICON_SIZE as i32,
        cy: ICON_SIZE as i32,
    };

    // SAFETY: shell_item valid COM interface; HBITMAP dikembalikan dan harus kita hapus.
    let hbitmap: HBITMAP = unsafe {
        shell_item
            .GetImage(size, SIIGBF_RESIZETOFIT)
            .map_err(|e| format!("GetImage: {e}"))?
    };

    let result = hbitmap_to_rgba(hbitmap, None);

    // SAFETY: hbitmap adalah milik kita setelah GetImage.
    unsafe {
        let _ = DeleteObject(hbitmap.into());
    }

    result
}

/// Fallback: `SHGetFileInfoW` → `HICON` → `GetIconInfo` → `GetDIBits` → RGBA.
fn extract_via_shgetfileinfo(path: &Path) -> Result<ExtractedIcon, String> {
    let wide: Vec<u16> = path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let mut fi = SHFILEINFOW::default();
    // SAFETY: wide valid, fi dialokasikan di stack.
    let ret = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut fi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_FLAGS(SHGFI_ICON.0 | SHGFI_LARGEICON.0),
        )
    };
    if ret == 0 || fi.hIcon.is_invalid() {
        return Err("SHGetFileInfoW tidak mengembalikan hIcon valid".to_owned());
    }

    let mut icon_info = ICONINFO::default();
    // SAFETY: fi.hIcon valid karena ret != 0 dan !is_invalid().
    let ok = unsafe { GetIconInfo(fi.hIcon, &mut icon_info) };
    if ok.is_err() {
        // SAFETY: fi.hIcon valid.
        unsafe {
            let _ = DestroyIcon(fi.hIcon);
        }
        return Err("GetIconInfo gagal".to_owned());
    }

    let mask = if !icon_info.hbmMask.is_invalid() {
        Some(icon_info.hbmMask)
    } else {
        None
    };

    let result = if !icon_info.hbmColor.is_invalid() {
        hbitmap_to_rgba(icon_info.hbmColor, mask)
    } else if let Some(m) = mask {
        hbitmap_to_rgba(m, None)
    } else {
        Err("ICONINFO tidak memiliki hbmColor maupun hbmMask".to_owned())
    };

    // SAFETY: hbitmap dan hIcon dari GetIconInfo/SHGetFileInfoW menjadi milik kita untuk dibebaskan.
    unsafe {
        if !icon_info.hbmColor.is_invalid() {
            let _ = DeleteObject(icon_info.hbmColor.into());
        }
        if !icon_info.hbmMask.is_invalid() {
            let _ = DeleteObject(icon_info.hbmMask.into());
        }
        let _ = DestroyIcon(fi.hIcon);
    }

    result
}

/// Membaca dimensi asli `HBITMAP` via `GetObjectW`, mengambil piksel via `GetDIBits`,
/// mengonversi BGRA ke straight RGBA, dan menyesuaikan ukurannya ke `ICON_SIZE × ICON_SIZE`.
fn hbitmap_to_rgba(hbitmap: HBITMAP, mask_bmp: Option<HBITMAP>) -> Result<ExtractedIcon, String> {
    let mut bmp = BITMAP::default();
    // SAFETY: GetObjectW membaca header BITMAP ke struct stack yang ukurannya tepat.
    let copied = unsafe {
        GetObjectW(
            hbitmap.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some((&mut bmp as *mut BITMAP).cast()),
        )
    };
    if copied == 0 || bmp.bmWidth <= 0 || bmp.bmHeight == 0 {
        return Err("GetObjectW gagal membaca dimensi bitmap".to_owned());
    }

    let src_w = bmp.bmWidth as u32;
    let src_h = bmp.bmHeight.unsigned_abs();
    let pixel_count = (src_w * src_h) as usize;

    // SAFETY: CreateCompatibleDC(None) membuat memory DC; hbitmap TIDAK boleh di-SelectObject
    // ke dalam DC ini sesuai syarat MSDN GetDIBits.
    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.is_invalid() {
        return Err("CreateCompatibleDC gagal".to_owned());
    }

    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: src_w as i32,
            biHeight: -(src_h as i32), // negatif = top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0, // BI_RGB
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bgra = vec![0u8; pixel_count * 4];
    // SAFETY: hdc valid, hbitmap tidak terseleksi ke DC mana pun, buffer bgra berukuran tepat.
    let lines = unsafe {
        GetDIBits(
            hdc,
            hbitmap,
            0,
            src_h,
            Some(bgra.as_mut_ptr().cast()),
            &mut bmi,
            DIB_RGB_COLORS,
        )
    };

    let mask_bgra = if let Some(mask) = mask_bmp {
        let mut m_buf = vec![0u8; pixel_count * 4];
        let mut m_bmi = bmi;
        // SAFETY: hdc valid dan ukuran mask untuk HICON warna sama dengan hbmColor.
        let m_lines = unsafe {
            GetDIBits(
                hdc,
                mask,
                0,
                src_h,
                Some(m_buf.as_mut_ptr().cast()),
                &mut m_bmi,
                DIB_RGB_COLORS,
            )
        };
        if m_lines > 0 {
            Some(m_buf)
        } else {
            None
        }
    } else {
        None
    };

    // SAFETY: hdc dibuat di atas dan tidak lagi dipakai.
    unsafe {
        let _ = DeleteDC(hdc);
    }

    if lines == 0 {
        return Err("GetDIBits mengembalikan 0 baris".to_owned());
    }

    // Periksa apakah bitmap memiliki saluran alpha sendiri atau 0 semua (ikon lawas).
    let has_any_alpha = bgra.as_chunks::<4>().0.iter().any(|px| px[3] != 0);
    // Deteksi apakah piksel merupakan premultiplied alpha (PARGB32): pada PARGB32 semua kanal <= alpha.
    let looks_premultiplied = has_any_alpha
        && bgra
            .as_chunks::<4>()
            .0
            .iter()
            .all(|px| px[0] <= px[3] && px[1] <= px[3] && px[2] <= px[3]);

    let mut rgba = vec![0u8; pixel_count * 4];
    for i in 0..pixel_count {
        let b = bgra[i * 4];
        let g = bgra[i * 4 + 1];
        let r = bgra[i * 4 + 2];
        let a = if has_any_alpha {
            bgra[i * 4 + 3]
        } else if let Some(ref mb) = mask_bgra {
            // Di mask HICON: piksel hitam (0) = opak, putih (255) = transparan.
            if mb[i * 4] == 0 {
                255
            } else {
                0
            }
        } else if r != 0 || g != 0 || b != 0 {
            255
        } else {
            0
        };

        // Un-premultiply ke straight alpha agar konsisten di seluruh pipeline.
        let (sr, sg, sb) = if looks_premultiplied && a > 0 && a < 255 {
            let a16 = u16::from(a);
            (
                ((u16::from(r) * 255) / a16).min(255) as u8,
                ((u16::from(g) * 255) / a16).min(255) as u8,
                ((u16::from(b) * 255) / a16).min(255) as u8,
            )
        } else {
            (r, g, b)
        };

        rgba[i * 4] = sr;
        rgba[i * 4 + 1] = sg;
        rgba[i * 4 + 2] = sb;
        rgba[i * 4 + 3] = a;
    }

    if src_w == ICON_SIZE && src_h == ICON_SIZE {
        return Ok(ExtractedIcon { rgba });
    }

    // Sesuaikan ke kanvas ICON_SIZE × ICON_SIZE sambil menjaga rasio aspek (letterbox transparan).
    let Some(img) = RgbaImage::from_raw(src_w, src_h, rgba) else {
        return Err("gagal membangun RgbaImage dari buffer bitmap".to_owned());
    };
    let scale = (ICON_SIZE as f32 / src_w as f32).min(ICON_SIZE as f32 / src_h as f32);
    let dst_w = ((src_w as f32 * scale).round() as u32).clamp(1, ICON_SIZE);
    let dst_h = ((src_h as f32 * scale).round() as u32).clamp(1, ICON_SIZE);
    let resized = image::imageops::resize(&img, dst_w, dst_h, FilterType::Triangle);

    let mut canvas = vec![0u8; (ICON_SIZE * ICON_SIZE * 4) as usize];
    let ox = ((ICON_SIZE - dst_w) / 2) as usize;
    let oy = ((ICON_SIZE - dst_h) / 2) as usize;
    let raw = resized.as_raw();
    for y in 0..dst_h as usize {
        let src_row = y * dst_w as usize * 4;
        let dst_row = ((oy + y) * ICON_SIZE as usize + ox) * 4;
        let len = dst_w as usize * 4;
        canvas[dst_row..dst_row + len].copy_from_slice(&raw[src_row..src_row + len]);
    }

    Ok(ExtractedIcon { rgba: canvas })
}

// ─── Ikon desktop bawaan: sembunyikan/tampilkan (spec §7.4, M5) ───────────────

/// True bila BubbleDesk saat ini yang menyembunyikan ikon desktop bawaan.
/// Dipakai panic hook & pemulihan startup.
static ICONS_HIDDEN_BY_US: AtomicBool = AtomicBool::new(false);

/// ID perintah menu "Tampilkan ikon desktop" di `SHELLDLL_DefView`.
const TOGGLE_ICONS_CMD: usize = 0x7402;

/// Apakah BubbleDesk saat ini sedang menyembunyikan ikon desktop.
pub fn icons_hidden_flag() -> bool {
    ICONS_HIDDEN_BY_US.load(Ordering::Relaxed)
}

/// Menyembunyikan/menampilkan ikon desktop bawaan secara idempoten.
///
/// Command `0x7402` bersifat toggle, jadi status diperiksa dulu lewat
/// `IsWindowVisible(SysListView32)`. Bila `SysListView32` tidak ada (mis. ikon
/// dimatikan lewat mode tampilan), dianggap sudah sesuai.
pub fn set_native_icons(visible: bool) -> Result<(), String> {
    let Some(defview) = find_def_view() else {
        return Err("SHELLDLL_DefView tidak ditemukan".to_owned());
    };
    ICONS_HIDDEN_BY_US.store(!visible, Ordering::Relaxed);

    let Some(list) = find_desktop_list_view() else {
        return Ok(());
    };
    // SAFETY: list adalah handle jendela valid hasil FindWindowExW.
    let current = unsafe { IsWindowVisible(list).as_bool() };
    if current == visible {
        return Ok(());
    }
    // SAFETY: mengirim WM_COMMAND toggle ke SHELLDLL_DefView (setara menu ikon desktop).
    unsafe {
        let _ = SendMessageW(
            defview,
            WM_COMMAND,
            Some(WPARAM(TOGGLE_ICONS_CMD)),
            Some(LPARAM(0)),
        );
    }
    Ok(())
}

/// Memulihkan ikon desktop bila BubbleDesk yang menyembunyikannya.
/// Dipanggil dari panic hook & pemulihan startup; tidak boleh panic.
pub fn force_show_native_icons() {
    if !icons_hidden_flag() {
        return;
    }
    match set_native_icons(true) {
        Ok(()) => log_info!("ikon desktop dipulihkan"),
        Err(e) => log_warn!("gagal memulihkan ikon desktop: {e}"),
    }
}

struct DefViewCtx {
    found: Option<HWND>,
}

fn find_def_view() -> Option<HWND> {
    let mut ctx = DefViewCtx { found: None };
    // SAFETY: lparam menunjuk ke ctx lokal yang hidup selama EnumWindows.
    let _ = unsafe {
        EnumWindows(
            Some(find_def_view_proc),
            LPARAM(&mut ctx as *mut DefViewCtx as isize),
        )
    };
    ctx.found
}

unsafe extern "system" fn find_def_view_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = unsafe { &mut *(lparam.0 as *mut DefViewCtx) };
    // SAFETY: hwnd dari EnumWindows valid; mencari child kelas SHELLDLL_DefView.
    if let Ok(child) =
        unsafe { FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), PCWSTR::null()) }
    {
        ctx.found = Some(child);
        return BOOL(0);
    }
    BOOL(1)
}

fn find_desktop_list_view() -> Option<HWND> {
    let defview = find_def_view()?;
    // SAFETY: defview valid; mencari child kelas SysListView32.
    unsafe { FindWindowExW(Some(defview), None, w!("SysListView32"), PCWSTR::null()) }.ok()
}

/// Membuat ikon placeholder: lingkaran berwarna (spec §11).
/// Dipakai hanya bila ekstraksi ikon gagal total.
pub fn placeholder_icon(_label: &str, ring_color: [f32; 4]) -> ExtractedIcon {
    let size = ICON_SIZE as usize;
    let mut rgba = vec![0u8; size * size * 4];
    let cx = size as f32 * 0.5;
    let cy = size as f32 * 0.5;
    let r = size as f32 * 0.42;

    let bg_r = (ring_color[0] * 180.0) as u8;
    let bg_g = (ring_color[1] * 180.0) as u8;
    let bg_b = (ring_color[2] * 180.0) as u8;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let alpha = if dist < r - 1.0 {
                255
            } else if dist < r + 1.0 {
                ((r + 1.0 - dist) * 127.5) as u8
            } else {
                0
            };
            let idx = (y * size + x) * 4;
            rgba[idx] = bg_r;
            rgba[idx + 1] = bg_g;
            rgba[idx + 2] = bg_b;
            rgba[idx + 3] = alpha;
        }
    }

    ExtractedIcon { rgba }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_icon_dan_nama_tampilan_windows() {
        let _com = ComGuard::init().expect("COM init");
        let notepad = Path::new(r"C:\Windows\System32\notepad.exe");
        if notepad.exists() {
            let icon = extract_icon(notepad).expect("extract notepad icon");
            assert_eq!(icon.rgba.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
            assert!(
                icon.rgba.iter().skip(3).step_by(4).any(|&a| a > 0),
                "ikon notepad harus memiliki piksel tidak transparan"
            );
            let name = shell_display_name(notepad);
            assert!(name.is_some(), "nama tampilan shell harus tersedia");
        }
    }

    #[test]
    fn desktop_dirs_ditemukan() {
        let user = desktop_dir();
        assert!(user.is_some(), "FOLDERID_Desktop harus mengembalikan path");
    }
}
