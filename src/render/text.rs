//! Render teks sederhana untuk debug overlay (spec §11, §14).
//!
//! Menggunakan `fontdue` untuk membaca font sistem Windows (Segoe UI)
//! tanpa membundel berkas font demi efisiensi ukuran biner dan RAM.

use super::{LabelRenderData, TooltipRenderData};
use bytemuck::{Pod, Zeroable};
use fontdue::{Font, FontSettings};
use std::collections::HashMap;
use std::path::Path;

/// Ukuran panel overlay (piksel).
pub const OVERLAY_WIDTH: u32 = 420;
pub const OVERLAY_HEIGHT: u32 = 84;

/// Ukuran tekstur glyph atlas R8 (piksel, spec §11: 512×512).
pub const GLYPH_ATLAS_SIZE: u32 = 512;

/// Ukuran font label bubble (piksel).
const LABEL_FONT_SIZE: f32 = 13.0;
const LABEL_PAD_X: f32 = 6.0;
const LABEL_PAD_Y: f32 = 2.0;
const LABEL_OFFSET_Y: f32 = 4.0;
const LABEL_HEIGHT: f32 = 18.0;

const TOOLTIP_MAX_WIDTH: f32 = 380.0;
const TOOLTIP_PAD_X: f32 = 10.0;
const TOOLTIP_PAD_Y: f32 = 7.0;
const TOOLTIP_LINE_HEIGHT: f32 = 17.0;
const TOOLTIP_GAP_Y: f32 = 8.0;
const TOOLTIP_BOTTOM_SAFE: f32 = 48.0;

/// Instance latar pill di belakang teks label.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct LabelBgInstance {
    pub rect_pos: [f32; 2],
    pub rect_size: [f32; 2],
}

/// Instance satu quad glyph teks untuk shader.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct GlyphInstance {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub color: [f32; 4],
}

#[derive(Clone, Copy)]
struct GlyphEntry {
    width: u32,
    height: u32,
    xmin: i32,
    ymin: i32,
    advance_width: f32,
    uv_min: [f32; 2],
    uv_max: [f32; 2],
}

/// Penggambar teks dan panel overlay + glyph atlas R8 untuk label bubble.
pub struct TextRenderer {
    font: Option<Font>,
    glyph_pixels: Vec<u8>,
    glyph_cache: HashMap<char, GlyphEntry>,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    atlas_dirty: bool,
    /// Buffer sementara untuk menampung teks terpotong tanpa alokasi baru setiap item.
    scratch_chars: Vec<char>,
}

impl TextRenderer {
    /// Membuat perender teks baru dengan memuat font sistem (Segoe UI / Arial).
    pub fn new() -> Self {
        let font = load_system_font();
        Self {
            font,
            glyph_pixels: vec![0u8; (GLYPH_ATLAS_SIZE * GLYPH_ATLAS_SIZE) as usize],
            glyph_cache: HashMap::new(),
            cursor_x: 1,
            cursor_y: 1,
            row_height: 0,
            atlas_dirty: false,
            scratch_chars: Vec::with_capacity(64),
        }
    }

    /// Menyiapkan instance latar pill dan glyph quad untuk seluruh label bubble.
    /// Mengembalikan `Some(&[u8])` (buffer R8 512×512) hanya jika ada glyph baru
    /// yang dirasterisasi sehingga perlu di-upload ulang ke GPU.
    #[allow(dead_code)]
    pub fn prepare_labels(
        &mut self,
        labels: &[LabelRenderData],
        screen_w: f32,
        screen_h: f32,
        out_bgs: &mut Vec<LabelBgInstance>,
        out_glyphs: &mut Vec<GlyphInstance>,
    ) -> Option<&[u8]> {
        self.prepare_labels_and_tooltip(labels, None, screen_w, screen_h, out_bgs, out_glyphs)
    }

    /// Menyiapkan instance latar pill dan glyph quad untuk seluruh label bubble
    /// serta kartu tooltip (jika aktif).
    pub fn prepare_labels_and_tooltip(
        &mut self,
        labels: &[LabelRenderData],
        tooltip: Option<&TooltipRenderData>,
        screen_w: f32,
        screen_h: f32,
        out_bgs: &mut Vec<LabelBgInstance>,
        out_glyphs: &mut Vec<GlyphInstance>,
    ) -> Option<&[u8]> {
        out_bgs.clear();
        out_glyphs.clear();

        if self.font.is_none() || (labels.is_empty() && tooltip.is_none()) {
            return None;
        }

        let ellipsis_w = self
            .ensure_glyph('…')
            .map(|g| g.advance_width)
            .unwrap_or(8.0);

        for label in labels {
            let max_text_w = (label.max_width - LABEL_PAD_X * 2.0).max(24.0);
            let total_w = self.layout_truncated_line(&label.text, max_text_w, ellipsis_w);
            if self.scratch_chars.is_empty() || total_w <= 0.0 {
                continue;
            }

            let pill_w = total_w + LABEL_PAD_X * 2.0;
            let pill_h = LABEL_HEIGHT;
            let pill_x =
                (label.anchor[0] - pill_w * 0.5).clamp(4.0, (screen_w - pill_w - 4.0).max(4.0));
            let pill_y =
                (label.anchor[1] + LABEL_OFFSET_Y).clamp(4.0, (screen_h - pill_h - 4.0).max(4.0));

            out_bgs.push(LabelBgInstance {
                rect_pos: [pill_x, pill_y],
                rect_size: [pill_w, pill_h],
            });

            self.emit_scratch_glyphs(
                pill_x + LABEL_PAD_X,
                pill_y + LABEL_PAD_Y + LABEL_FONT_SIZE - 2.0,
                [0.95, 0.96, 0.98, 1.0],
                out_glyphs,
            );
        }

        if let Some(tip) = tooltip {
            self.append_tooltip(tip, screen_w, screen_h, ellipsis_w, out_bgs, out_glyphs);
        }

        if self.atlas_dirty {
            self.atlas_dirty = false;
            Some(&self.glyph_pixels)
        } else {
            None
        }
    }

    fn layout_truncated_line(&mut self, text: &str, max_text_w: f32, ellipsis_w: f32) -> f32 {
        self.scratch_chars.clear();
        let mut total_w = 0.0_f32;
        let mut truncated = false;
        for ch in text.chars() {
            let adv = self
                .ensure_glyph(ch)
                .map(|g| g.advance_width)
                .unwrap_or(0.0);
            if total_w + adv > max_text_w {
                truncated = true;
                break;
            }
            self.scratch_chars.push(ch);
            total_w += adv;
        }
        if truncated {
            while let Some(&last) = self.scratch_chars.last() {
                if total_w + ellipsis_w <= max_text_w {
                    break;
                }
                self.scratch_chars.pop();
                total_w -= self
                    .glyph_cache
                    .get(&last)
                    .map(|g| g.advance_width)
                    .unwrap_or(0.0);
            }
            self.scratch_chars.push('…');
            total_w += ellipsis_w;
        }
        total_w
    }

    fn emit_scratch_glyphs(
        &self,
        start_x: f32,
        baseline: f32,
        text_color: [f32; 4],
        out_glyphs: &mut Vec<GlyphInstance>,
    ) {
        let mut pen_x = start_x;
        for &ch in &self.scratch_chars {
            if let Some(g) = self.glyph_cache.get(&ch) {
                if g.width > 0 && g.height > 0 {
                    let gx = (pen_x + g.xmin as f32).round();
                    let gy = (baseline - g.height as f32 - g.ymin as f32).round();
                    out_glyphs.push(GlyphInstance {
                        pos: [gx, gy],
                        size: [g.width as f32, g.height as f32],
                        uv_min: g.uv_min,
                        uv_max: g.uv_max,
                        color: text_color,
                    });
                }
                pen_x += g.advance_width;
            }
        }
    }

    fn append_tooltip(
        &mut self,
        tip: &TooltipRenderData,
        screen_w: f32,
        screen_h: f32,
        ellipsis_w: f32,
        out_bgs: &mut Vec<LabelBgInstance>,
        out_glyphs: &mut Vec<GlyphInstance>,
    ) {
        let max_box_w = TOOLTIP_MAX_WIDTH.min((screen_w - 16.0).max(120.0));
        let max_text_w = (max_box_w - TOOLTIP_PAD_X * 2.0).max(40.0);

        let w1 = self.layout_truncated_line(&tip.title, max_text_w, ellipsis_w);
        let w2 = self.layout_truncated_line(&tip.subtitle, max_text_w, ellipsis_w);
        let w3 = self.layout_truncated_line(&tip.detail, max_text_w, ellipsis_w);
        let content_w = w1.max(w2).max(w3);
        if content_w <= 0.0 {
            return;
        }

        let box_w = content_w + TOOLTIP_PAD_X * 2.0;
        let box_h = TOOLTIP_LINE_HEIGHT * 3.0 + TOOLTIP_PAD_Y * 2.0;
        let box_x = (tip.anchor_x - box_w * 0.5).clamp(8.0, (screen_w - box_w - 8.0).max(8.0));
        let below_y = tip.bubble_center_y + tip.bubble_radius + TOOLTIP_GAP_Y;
        let box_y = if below_y + box_h <= screen_h - TOOLTIP_BOTTOM_SAFE {
            below_y
        } else {
            (tip.bubble_center_y - tip.bubble_radius - box_h - TOOLTIP_GAP_Y)
                .clamp(8.0, (screen_h - box_h - 8.0).max(8.0))
        };

        out_bgs.push(LabelBgInstance {
            rect_pos: [box_x, box_y],
            rect_size: [box_w, box_h],
        });

        let text_x = box_x + TOOLTIP_PAD_X;
        let sub_color = if tip.is_error {
            [1.0, 0.52, 0.52, 1.0]
        } else {
            [0.76, 0.84, 0.95, 1.0]
        };
        let lines: [(&str, [f32; 4]); 3] = [
            (&tip.title, [0.97, 0.98, 1.0, 1.0]),
            (&tip.subtitle, sub_color),
            (&tip.detail, [0.64, 0.70, 0.78, 1.0]),
        ];
        for (idx, (text, color)) in lines.iter().enumerate() {
            self.layout_truncated_line(text, max_text_w, ellipsis_w);
            let baseline =
                box_y + TOOLTIP_PAD_Y + idx as f32 * TOOLTIP_LINE_HEIGHT + LABEL_FONT_SIZE - 1.0;
            self.emit_scratch_glyphs(text_x, baseline, *color, out_glyphs);
        }
    }

    /// Memastikan glyph untuk `ch` sudah dirasterisasi ke dalam atlas R8 512×512.
    fn ensure_glyph(&mut self, ch: char) -> Option<GlyphEntry> {
        if let Some(&entry) = self.glyph_cache.get(&ch) {
            return Some(entry);
        }
        let font = self.font.as_ref()?;
        let (metrics, bitmap) = font.rasterize(ch, LABEL_FONT_SIZE);
        let w = metrics.width as u32;
        let h = metrics.height as u32;

        if w == 0 || h == 0 {
            let entry = GlyphEntry {
                width: 0,
                height: 0,
                xmin: metrics.xmin,
                ymin: metrics.ymin,
                advance_width: metrics.advance_width,
                uv_min: [0.0, 0.0],
                uv_max: [0.0, 0.0],
            };
            self.glyph_cache.insert(ch, entry);
            return Some(entry);
        }

        // Cek ruang di baris saat ini (+1 px padding antar glyph)
        if self.cursor_x + w + 1 >= GLYPH_ATLAS_SIZE {
            self.cursor_x = 1;
            self.cursor_y += self.row_height + 1;
            self.row_height = 0;
        }
        // Jika atlas penuh, reset dan muat ulang agar tidak crash/kehilangan label baru
        if self.cursor_y + h + 1 >= GLYPH_ATLAS_SIZE {
            self.glyph_pixels.fill(0);
            self.glyph_cache.clear();
            self.cursor_x = 1;
            self.cursor_y = 1;
            self.row_height = 0;
        }

        let ox = self.cursor_x;
        let oy = self.cursor_y;
        let stride = GLYPH_ATLAS_SIZE as usize;
        for row in 0..h as usize {
            let src_start = row * w as usize;
            let dst_start = (oy as usize + row) * stride + ox as usize;
            self.glyph_pixels[dst_start..dst_start + w as usize]
                .copy_from_slice(&bitmap[src_start..src_start + w as usize]);
        }

        self.cursor_x += w + 1;
        self.row_height = self.row_height.max(h);
        self.atlas_dirty = true;

        let s = GLYPH_ATLAS_SIZE as f32;
        let entry = GlyphEntry {
            width: w,
            height: h,
            xmin: metrics.xmin,
            ymin: metrics.ymin,
            advance_width: metrics.advance_width,
            uv_min: [ox as f32 / s, oy as f32 / s],
            uv_max: [(ox + w) as f32 / s, (oy + h) as f32 / s],
        };
        self.glyph_cache.insert(ch, entry);
        Some(entry)
    }

    /// Merender panel debug overlay dengan latar semi-transparan dan teks metrik.
    pub fn render_overlay(
        &self,
        fps: f32,
        frame_time_ms: f32,
        working_set_bytes: u64,
        private_bytes: u64,
        body_count: usize,
        is_asleep: bool,
    ) -> Vec<u8> {
        let mut buffer = vec![0u8; (OVERLAY_WIDTH * OVERLAY_HEIGHT * 4) as usize];

        // Latar belakang panel: kotak gelap semi-transparan (rgba: 15, 20, 26, 210)
        for pixel in buffer.as_chunks_mut::<4>().0 {
            pixel[0] = 15;
            pixel[1] = 20;
            pixel[2] = 26;
            pixel[3] = 210;
        }

        let ws_mb = working_set_bytes as f64 / (1024.0 * 1024.0);
        let priv_mb = private_bytes as f64 / (1024.0 * 1024.0);
        let status = if is_asleep { "Asleep" } else { "Awake" };

        let line1 = format!("BubbleDesk Debug | FPS: {fps:.1} ({frame_time_ms:.2} ms)");
        let line2 = format!("RAM: {ws_mb:.1} MB WS | {priv_mb:.1} MB Priv");
        let line3 = format!("Bodies: {body_count} | Status: {status}");

        self.draw_string(&mut buffer, 12, 12, &line1, 14.0, [240, 240, 245, 255]);
        self.draw_string(&mut buffer, 12, 36, &line2, 13.0, [180, 210, 245, 255]);
        self.draw_string(&mut buffer, 12, 58, &line3, 13.0, [170, 230, 180, 255]);

        buffer
    }

    /// Menggambar string ke buffer RGBA pada koordinat (start_x, start_y).
    fn draw_string(
        &self,
        buffer: &mut [u8],
        start_x: u32,
        start_y: u32,
        text: &str,
        size: f32,
        color: [u8; 4],
    ) {
        let Some(font) = &self.font else {
            return;
        };

        let mut pen_x = start_x as f32;
        let baseline = start_y as f32 + size;

        for ch in text.chars() {
            let (metrics, bitmap) = font.rasterize(ch, size);
            let glyph_x = pen_x + metrics.xmin as f32;
            let glyph_y = baseline - metrics.height as f32 - metrics.ymin as f32;

            for row in 0..metrics.height {
                let py = (glyph_y + row as f32) as i32;
                if py < 0 || py >= OVERLAY_HEIGHT as i32 {
                    continue;
                }
                for col in 0..metrics.width {
                    let px = (glyph_x + col as f32) as i32;
                    if px < 0 || px >= OVERLAY_WIDTH as i32 {
                        continue;
                    }
                    let alpha = bitmap[row * metrics.width + col];
                    if alpha == 0 {
                        continue;
                    }

                    let idx = ((py as u32 * OVERLAY_WIDTH + px as u32) * 4) as usize;
                    let a_norm = alpha as f32 / 255.0;

                    // Alpha blending teks di atas latar belakang panel
                    let bg_r = buffer[idx] as f32;
                    let bg_g = buffer[idx + 1] as f32;
                    let bg_b = buffer[idx + 2] as f32;

                    buffer[idx] = (color[0] as f32 * a_norm + bg_r * (1.0 - a_norm)) as u8;
                    buffer[idx + 1] = (color[1] as f32 * a_norm + bg_g * (1.0 - a_norm)) as u8;
                    buffer[idx + 2] = (color[2] as f32 * a_norm + bg_b * (1.0 - a_norm)) as u8;
                    buffer[idx + 3] = 235; // Alpha tetap solid/tinggi untuk panel
                }
            }

            pen_x += metrics.advance_width;
        }
    }
}

/// Membaca font sistem Windows (Segoe UI atau Arial) dari `C:\Windows\Fonts`.
fn load_system_font() -> Option<Font> {
    let candidates = [
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arial.ttf",
        r"C:\Windows\Fonts\tahoma.ttf",
    ];

    for path in &candidates {
        if Path::new(path).is_file() {
            if let Ok(data) = std::fs::read(path) {
                if let Ok(font) = Font::from_bytes(data, FontSettings::default()) {
                    return Some(font);
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_overlay_generates_correct_buffer_size() {
        let renderer = TextRenderer::new();
        let bytes =
            renderer.render_overlay(60.0, 16.6, 35 * 1024 * 1024, 18 * 1024 * 1024, 20, false);
        assert_eq!(bytes.len(), (OVERLAY_WIDTH * OVERLAY_HEIGHT * 4) as usize);
        // Alpha panel harus terisi (tidak nol)
        assert!(bytes.iter().skip(3).step_by(4).any(|&a| a > 0));
    }

    #[test]
    fn prepare_labels_populates_glyph_atlas_and_instances() {
        let mut renderer = TextRenderer::new();
        let labels = vec![LabelRenderData {
            anchor: [400.0, 300.0],
            text: "Dokumen Penting".to_owned(),
            max_width: 120.0,
        }];
        let mut bgs = Vec::new();
        let mut glyphs = Vec::new();
        let has_font = renderer.font.is_some();
        let uploaded = renderer.prepare_labels(&labels, 1600.0, 900.0, &mut bgs, &mut glyphs);
        if has_font {
            assert_eq!(bgs.len(), 1);
            assert!(!glyphs.is_empty());
            assert!(uploaded.is_some());
            // Pemanggilan kedua dengan teks yang sama tidak perlu upload ulang atlas
            let uploaded2 = renderer.prepare_labels(&labels, 1600.0, 900.0, &mut bgs, &mut glyphs);
            assert!(uploaded2.is_none());
        }
    }
}
