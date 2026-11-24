//! Modul rendering BubbleDesk (spec §11).
//!
//! Menyediakan abstraksi `Renderer` agar implementasi GPU (`wgpu`)
//! atau CPU (`tiny-skia` + `softbuffer`) dapat saling menggantikan
//! sesuai evaluasi anggaran RAM (§21).

pub mod atlas;
pub mod text;
pub mod wgpu_backend;

pub use wgpu_backend::WgpuRenderer;

use crate::platform::wallpaper::WallpaperImage;

/// Jenis item bubble untuk pewarnaan cincin default (spec §11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BubbleKind {
    App,
    Folder,
    Document,
    Media,
    Other,
}

impl BubbleKind {
    /// Warna RGBA (0.0 s.d 1.0) cincin default berdasarkan jenis.
    pub fn default_ring_color(&self) -> [f32; 4] {
        match self {
            // #5B8DEF -> (91, 141, 239)
            BubbleKind::App => [0.357, 0.553, 0.937, 0.95],
            // #F2B84B -> (242, 184, 75)
            BubbleKind::Folder => [0.949, 0.722, 0.294, 0.95],
            // #6FCF97 -> (111, 207, 151)
            BubbleKind::Document => [0.435, 0.812, 0.592, 0.95],
            // #BB6BD9 -> (187, 107, 217)
            BubbleKind::Media => [0.733, 0.420, 0.851, 0.95],
            // #9AA5B1 -> (154, 165, 177)
            BubbleKind::Other => [0.604, 0.647, 0.694, 0.95],
        }
    }
}

/// Data rendering untuk satu instance bubble.
#[derive(Debug, Clone, Copy)]
pub struct BubbleRenderData {
    pub center: [f32; 2],
    pub radius: f32,
    pub visual_scale: f32,
    pub ring_color: [f32; 4],
    pub ring_width: f32,
    pub glow: f32,
    pub flags: u32,
    /// Koordinat UV slot ikon di atlas `[u_min, v_min, u_max, v_max]`.
    pub icon_uv: [f32; 4],
    /// Indeks halaman atlas (`-1` jika ikon belum siap).
    pub icon_page: i32,
}

/// Flag bit untuk instance bubble di shader (`BubbleRenderData::flags`).
pub const BUBBLE_FLAG_HOVER: u32 = 1 << 0;
pub const BUBBLE_FLAG_ERROR: u32 = 1 << 1;

/// Data rendering untuk label nama di bawah satu bubble.
#[derive(Debug, Clone)]
pub struct LabelRenderData {
    /// Titik tengah bawah bubble (x, y) dalam piksel layar.
    pub anchor: [f32; 2],
    /// Teks nama item.
    pub text: String,
    /// Lebar maksimum label sebelum dipotong elipsis.
    pub max_width: f32,
}

/// Data rendering untuk tooltip saat hover >= 400 ms atau saat peluncuran gagal (spec §12).
#[derive(Debug, Clone)]
pub struct TooltipRenderData {
    /// Koordinat X pusat bubble acuan (piksel).
    pub anchor_x: f32,
    /// Koordinat Y pusat bubble dan radius efektifnya agar penempatan di atas/bawah akurat.
    pub bubble_center_y: f32,
    pub bubble_radius: f32,
    /// Baris 1: nama item.
    pub title: String,
    /// Baris 2: statistik (`"Dibuka N× · terakhir <relatif>"`) atau pesan error.
    pub subtitle: String,
    /// Baris 3: path lengkap item.
    pub detail: String,
    /// `true` bila menampilkan error peluncuran.
    pub is_error: bool,
}

/// Data untuk panel debug overlay (spec §14).
#[derive(Debug, Clone, Copy, Default)]
pub struct OverlayRenderData {
    pub show: bool,
    pub fps: f32,
    pub frame_time_ms: f32,
    pub working_set_bytes: u64,
    pub private_bytes: u64,
    pub body_count: usize,
    pub is_asleep: bool,
}

/// Data lengkap untuk menggambar satu frame.
pub struct FrameData<'a> {
    pub screen_width: u32,
    pub screen_height: u32,
    pub background_dim: f32,
    pub time: f32,
    pub bubbles: &'a [BubbleRenderData],
    pub labels: &'a [LabelRenderData],
    pub tooltip: Option<&'a TooltipRenderData>,
    pub overlay: OverlayRenderData,
}

/// Kesalahan render.
#[derive(Debug)]
pub enum RenderError {
    SurfaceLost,
    OutOfMemory(String),
    #[allow(dead_code)]
    DeviceLost,
    Other(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SurfaceLost => write!(f, "Surface lost atau outdated"),
            Self::OutOfMemory(s) => write!(f, "Kehabisan memori GPU: {s}"),
            Self::DeviceLost => write!(f, "Device render GPU terputus (device lost)"),
            Self::Other(s) => write!(f, "Kesalahan render umum: {s}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// Trait tipis antarmuka renderer (spec §11).
pub trait Renderer {
    /// Menyesuaikan ukuran swapchain / framebuffer dengan jendela.
    fn resize(&mut self, width: u32, height: u32);

    /// Mengatur atau memperbarui gambar latar belakang.
    fn set_background(&mut self, image: &WallpaperImage);

    /// Mengunggah satu halaman atlas ikon (RGBA 1536×1536) ke GPU.
    fn update_atlas(&mut self, page: usize, pixels: &[u8]);

    /// Menggambar satu frame lengkap (background + bubbles + labels + overlay).
    fn draw(&mut self, frame: &FrameData) -> Result<(), RenderError>;

    /// Melepaskan resource render saat keluar.
    fn shutdown(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_colors_have_high_alpha_and_distinct_values() {
        let kinds = [
            BubbleKind::App,
            BubbleKind::Folder,
            BubbleKind::Document,
            BubbleKind::Media,
            BubbleKind::Other,
        ];
        for k in kinds {
            let color = k.default_ring_color();
            assert!(color[3] >= 0.9, "alpha harus tinggi/jelas untuk cincin");
            assert!(color[0] >= 0.0 && color[0] <= 1.0);
            assert!(color[1] >= 0.0 && color[1] <= 1.0);
            assert!(color[2] >= 0.0 && color[2] <= 1.0);
        }
        // Pastikan warna tiap jenis berbeda
        assert_ne!(
            BubbleKind::App.default_ring_color(),
            BubbleKind::Folder.default_ring_color()
        );
        assert_ne!(
            BubbleKind::Document.default_ring_color(),
            BubbleKind::Media.default_ring_color()
        );
    }
}
