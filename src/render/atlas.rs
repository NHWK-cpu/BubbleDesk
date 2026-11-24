//! Manajemen atlas ikon untuk rendering bubble (spec §11).
//!
//! Slot 96×96 px, satu halaman 1536×1536 (256 slot), maksimum 2 halaman.
//! Alokasi malas: halaman baru dibuat saat slot penuh.
//! Upload ke GPU dilakukan via `wgpu::Queue::write_texture`.

/// Ukuran satu slot ikon (piksel).
pub const ICON_SLOT_SIZE: u32 = 96;

/// Ukuran satu halaman atlas (piksel).
pub const ATLAS_PAGE_SIZE: u32 = 1536;

/// Jumlah slot per baris dalam satu halaman atlas (1536 / 96 = 16).
pub const SLOTS_PER_ROW: u32 = ATLAS_PAGE_SIZE / ICON_SLOT_SIZE;

/// Jumlah maksimum slot per halaman (16 * 16 = 256 slot).
pub const MAX_SLOTS_PER_PAGE: usize = (SLOTS_PER_ROW * SLOTS_PER_ROW) as usize;

/// Lokasi satu slot ikon di dalam atlas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlotLocation {
    /// Indeks halaman (0-based).
    pub page: usize,
    /// Kolom slot dalam halaman.
    pub col: u32,
    /// Baris slot dalam halaman.
    pub row: u32,
}

/// Koordinat UV satu slot dalam atlas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconUvRect {
    pub u_min: f32,
    pub v_min: f32,
    pub u_max: f32,
    pub v_max: f32,
}

impl Default for IconUvRect {
    fn default() -> Self {
        Self {
            u_min: 0.0,
            v_min: 0.0,
            u_max: 1.0,
            v_max: 1.0,
        }
    }
}

impl SlotLocation {
    /// Koordinat UV untuk slot ini.
    pub fn uv_rect(&self) -> IconUvRect {
        let s = ATLAS_PAGE_SIZE as f32;
        let slot = ICON_SLOT_SIZE as f32;
        IconUvRect {
            u_min: self.col as f32 * slot / s,
            v_min: self.row as f32 * slot / s,
            u_max: (self.col as f32 + 1.0) * slot / s,
            v_max: (self.row as f32 + 1.0) * slot / s,
        }
    }
}

/// Satu halaman atlas di CPU (buffer piksel RGBA).
struct AtlasPage {
    /// Data piksel RGBA, ukuran ATLAS_PAGE_SIZE × ATLAS_PAGE_SIZE × 4.
    pixels: Vec<u8>,
    /// Jumlah slot yang sudah terisi.
    used: usize,
    /// Apakah halaman ini perlu di-upload ulang ke GPU.
    dirty: bool,
}

impl AtlasPage {
    fn new() -> Self {
        let total = (ATLAS_PAGE_SIZE * ATLAS_PAGE_SIZE * 4) as usize;
        Self {
            pixels: vec![0u8; total],
            used: 0,
            dirty: false,
        }
    }

    fn is_full(&self) -> bool {
        self.used >= MAX_SLOTS_PER_PAGE
    }

    /// Menulis data ikon (RGBA 96×96) ke slot berikutnya.
    /// Mengembalikan `SlotLocation` jika berhasil.
    fn write_slot(&mut self, page_idx: usize, rgba: &[u8]) -> Option<SlotLocation> {
        if self.is_full() {
            return None;
        }
        let slot_idx = self.used;
        let col = (slot_idx % SLOTS_PER_ROW as usize) as u32;
        let row = (slot_idx / SLOTS_PER_ROW as usize) as u32;

        let x0 = (col * ICON_SLOT_SIZE) as usize;
        let y0 = (row * ICON_SLOT_SIZE) as usize;
        let stride = ATLAS_PAGE_SIZE as usize * 4;

        for y in 0..ICON_SLOT_SIZE as usize {
            let src_start = y * ICON_SLOT_SIZE as usize * 4;
            let dst_start = (y0 + y) * stride + x0 * 4;
            let len = ICON_SLOT_SIZE as usize * 4;
            self.pixels[dst_start..dst_start + len]
                .copy_from_slice(&rgba[src_start..src_start + len]);
        }

        self.used += 1;
        self.dirty = true;
        Some(SlotLocation {
            page: page_idx,
            col,
            row,
        })
    }
}

/// Manajer atlas ikon (CPU-side).
/// Pasangkan dengan `GpuAtlas` di renderer untuk sisi GPU.
pub struct IconAtlas {
    pages: Vec<AtlasPage>,
    /// Maks halaman (spec §11: maks 2).
    max_pages: usize,
}

/// Maks halaman atlas (spec §11).
const MAX_PAGES: usize = 2;

impl IconAtlas {
    pub fn new() -> Self {
        Self {
            pages: vec![AtlasPage::new()],
            max_pages: MAX_PAGES,
        }
    }

    /// Menambahkan ikon ke atlas.
    /// Mengembalikan `SlotLocation` jika berhasil, `None` jika atlas penuh.
    pub fn add_icon(&mut self, rgba: &[u8]) -> Option<SlotLocation> {
        // Coba halaman terakhir dulu
        let last = self.pages.len() - 1;
        if let Some(loc) = self.pages[last].write_slot(last, rgba) {
            return Some(loc);
        }
        // Halaman penuh — buat baru jika masih bisa
        if self.pages.len() < self.max_pages {
            self.pages.push(AtlasPage::new());
            let new_idx = self.pages.len() - 1;
            return self.pages[new_idx].write_slot(new_idx, rgba);
        }
        None
    }

    /// Jumlah slot yang sudah terisi di seluruh halaman.
    // ponytail: dipakai M4 untuk perf logging
    #[allow(dead_code)]
    pub fn total_used(&self) -> usize {
        self.pages.iter().map(|p| p.used).sum()
    }

    /// Iterasi halaman yang perlu di-upload ke GPU.
    pub fn flush_dirty<F>(&mut self, mut f: F)
    where
        F: FnMut(usize, &[u8]),
    {
        for (i, page) in self.pages.iter_mut().enumerate() {
            if page.dirty {
                f(i, &page.pixels);
                page.dirty = false;
            }
        }
    }

    /// Jumlah halaman yang ada.
    // ponytail: dipakai M4 untuk inisialisasi GPU texture array
    #[allow(dead_code)]
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

impl Default for IconAtlas {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_icon() -> Vec<u8> {
        vec![128u8; (ICON_SLOT_SIZE * ICON_SLOT_SIZE * 4) as usize]
    }

    #[test]
    fn tambah_satu_slot() {
        let mut atlas = IconAtlas::new();
        let loc = atlas.add_icon(&dummy_icon()).expect("harus berhasil");
        assert_eq!(loc.page, 0);
        assert_eq!(loc.col, 0);
        assert_eq!(loc.row, 0);
        assert_eq!(atlas.total_used(), 1);
    }

    #[test]
    fn uv_rect_slot_pertama() {
        let loc = SlotLocation {
            page: 0,
            col: 0,
            row: 0,
        };
        let uv = loc.uv_rect();
        assert_eq!(uv.u_min, 0.0);
        assert_eq!(uv.v_min, 0.0);
        let expected_max = ICON_SLOT_SIZE as f32 / ATLAS_PAGE_SIZE as f32;
        assert!((uv.u_max - expected_max).abs() < 1e-6);
    }

    #[test]
    fn uv_rect_slot_kedua() {
        let loc = SlotLocation {
            page: 0,
            col: 1,
            row: 0,
        };
        let uv = loc.uv_rect();
        let slot_uv = ICON_SLOT_SIZE as f32 / ATLAS_PAGE_SIZE as f32;
        assert!((uv.u_min - slot_uv).abs() < 1e-6);
    }

    #[test]
    fn halaman_penuh_membuat_halaman_baru() {
        let mut atlas = IconAtlas::new();
        // Isi halaman pertama penuh
        for _ in 0..MAX_SLOTS_PER_PAGE {
            atlas.add_icon(&dummy_icon()).expect("harus berhasil");
        }
        assert_eq!(atlas.page_count(), 1);
        // Slot berikutnya buat halaman baru
        let loc = atlas.add_icon(&dummy_icon()).expect("halaman baru");
        assert_eq!(loc.page, 1);
        assert_eq!(atlas.page_count(), 2);
    }

    #[test]
    fn atlas_penuh_mengembalikan_none() {
        let mut atlas = IconAtlas::new();
        for _ in 0..(MAX_SLOTS_PER_PAGE * MAX_PAGES) {
            atlas.add_icon(&dummy_icon()).expect("harus berhasil");
        }
        assert!(atlas.add_icon(&dummy_icon()).is_none());
    }

    #[test]
    fn flush_dirty_dipanggil_sekali() {
        let mut atlas = IconAtlas::new();
        atlas.add_icon(&dummy_icon());
        let mut count = 0;
        atlas.flush_dirty(|_, _| count += 1);
        assert_eq!(count, 1);
        // Kedua kali tidak ada yang dirty
        atlas.flush_dirty(|_, _| count += 1);
        assert_eq!(count, 1);
    }
}
