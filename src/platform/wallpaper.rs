//! Pembacaan dan persiapan latar belakang wallpaper desktop (spec §11).
//!
//! Mengambil path wallpaper aktif dari Windows (`SPI_GETDESKWALLPAPER`),
//! mendekode berkas gambar (PNG, JPEG, BMP), melakukan scaling dan blur
//! jika diperlukan, lalu menghasilkan buffer RGBA untuk tekstur GPU.

use std::path::{Path, PathBuf};

/// Data gambar latar dalam format RGBA 8-bit.
#[derive(Debug, Clone)]
pub struct WallpaperImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Mengambil path wallpaper saat ini dari Windows.
#[cfg(windows)]
pub fn current_wallpaper_path() -> Option<PathBuf> {
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETDESKWALLPAPER};

    let mut buf = [0u16; 1024];
    // SAFETY: buf berukuran 1024 u16, dipass pointer dan kapasitasnya.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            buf.len() as u32,
            Some(buf.as_mut_ptr() as *mut _),
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };

    if ok.is_err() {
        return None;
    }

    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    if len == 0 {
        return None;
    }
    let path_str = String::from_utf16_lossy(&buf[..len]);
    let path = PathBuf::from(path_str);
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

#[cfg(not(windows))]
pub fn current_wallpaper_path() -> Option<PathBuf> {
    None
}

/// Memuat wallpaper dari path atau menghasilkan latar gradien default bila gagal.
pub fn load_wallpaper_or_default(
    path: Option<&Path>,
    target_width: u32,
    target_height: u32,
    blur_radius: u32,
) -> WallpaperImage {
    let target_width = target_width.max(1);
    let target_height = target_height.max(1);

    if let Some(p) = path {
        if let Ok(img) = image::open(p) {
            let mut rgba = img.to_rgba8();
            if rgba.width() != target_width || rgba.height() != target_height {
                rgba = image::imageops::resize(
                    &rgba,
                    target_width,
                    target_height,
                    image::imageops::FilterType::Triangle,
                );
            }
            if blur_radius > 0 {
                // Blur sederhana di CPU (spec §11) sebelum diunggah ke GPU
                rgba = image::imageops::blur(&rgba, blur_radius as f32);
            }
            return WallpaperImage {
                width: rgba.width(),
                height: rgba.height(),
                rgba: rgba.into_raw(),
            };
        }
    }

    // Fallback: gradien vertikal halus (spec §11: "pakai gradien default")
    generate_default_gradient(target_width, target_height)
}

/// Menghasilkan gambar warna solid dari COLORREF (0x00BBGGRR).
pub fn generate_solid_color(width: u32, height: u32, colorref: u32) -> WallpaperImage {
    let r = (colorref & 0xFF) as u8;
    let g = ((colorref >> 8) & 0xFF) as u8;
    let b = ((colorref >> 16) & 0xFF) as u8;
    let pixel = [r, g, b, 255];
    let total_pixels = (width * height) as usize;
    let mut rgba = Vec::with_capacity(total_pixels * 4);
    for _ in 0..total_pixels {
        rgba.extend_from_slice(&pixel);
    }
    WallpaperImage {
        width,
        height,
        rgba,
    }
}

/// Menghasilkan gradien halus gelap (dari #1b2838 ke #0d1117) sebagai fallback.
pub fn generate_default_gradient(width: u32, height: u32) -> WallpaperImage {
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let t = y as f32 / height.max(1) as f32;
        // Interpolasi warna RGB antara (27, 40, 56) dan (13, 17, 23)
        let r = ((1.0 - t) * 27.0 + t * 13.0) as u8;
        let g = ((1.0 - t) * 40.0 + t * 17.0) as u8;
        let b = ((1.0 - t) * 56.0 + t * 23.0) as u8;
        for _ in 0..width {
            rgba.push(r);
            rgba.push(g);
            rgba.push(b);
            rgba.push(255);
        }
    }
    WallpaperImage {
        width,
        height,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_gradient_has_correct_dimensions_and_alpha() {
        let grad = generate_default_gradient(100, 50);
        assert_eq!(grad.width, 100);
        assert_eq!(grad.height, 50);
        assert_eq!(grad.rgba.len(), 100 * 50 * 4);
        for (i, &byte) in grad.rgba.iter().enumerate() {
            if i % 4 == 3 {
                assert_eq!(byte, 255);
            }
        }
    }

    #[test]
    fn solid_color_generation_maps_rgb_correctly() {
        let colorref = 0x00332211;
        let img = generate_solid_color(10, 10, colorref);
        assert_eq!(img.width, 10);
        assert_eq!(img.height, 10);
        assert_eq!(img.rgba.len(), 10 * 10 * 4);
        assert_eq!(img.rgba[0], 0x11);
        assert_eq!(img.rgba[1], 0x22);
        assert_eq!(img.rgba[2], 0x33);
        assert_eq!(img.rgba[3], 255);
    }
}
