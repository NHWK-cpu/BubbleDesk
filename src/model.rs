//! Model data item desktop (spec §8).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Klasifikasi item untuk pewarnaan cincin bubble (spec §13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    App,
    Folder,
    Document,
    Media,
    Other,
}

impl ItemKind {
    /// Menentukan jenis item dari path (spec §13).
    pub fn from_path(path: &std::path::Path) -> Self {
        if path.is_dir() {
            return Self::Folder;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        match ext.as_deref() {
            Some("lnk") | Some("url") | Some("exe") | Some("bat") | Some("cmd") => Self::App,
            Some("docx") | Some("xlsx") | Some("pptx") | Some("pdf") | Some("txt") | Some("md")
            | Some("csv") | Some("odt") => Self::Document,
            Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("mp3") | Some("mp4")
            | Some("mkv") | Some("wav") | Some("webp") | Some("bmp") => Self::Media,
            _ => Self::Other,
        }
    }
}

/// Satu item desktop (spec §8).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    /// Hash stabil dari path sumber (lowercase, full path).
    pub id: String,
    /// Path entri di desktop (untuk .lnk: path .lnk itu sendiri).
    pub path: PathBuf,
    /// Label kustom; kosong = nama tampilan dari shell.
    pub label: Option<String>,
    pub kind: ItemKind,
    /// Skor terakhir yang disimpan (§9).
    pub score: f64,
    /// Unix detik saat skor dihitung.
    pub score_t0: i64,
    pub total_launches: u64,
    pub last_launch: Option<i64>,
    pub first_seen: i64,
    /// Posisi ternormalisasi 0..1 (opsional, diatur saat drag+pin).
    pub pinned_pos: Option<[f32; 2]>,
    pub hidden: bool,
    /// Item yang file-nya menghilang; akan dibuang setelah 30 hari.
    #[serde(default)]
    pub missing: bool,
    /// Unix detik pertama kali item dinyatakan missing.
    #[serde(default)]
    pub missing_since: Option<i64>,
}

impl Item {
    /// Membuat ID stabil dari path (hash lowercase full path).
    pub fn make_id(path: &std::path::Path) -> String {
        // Normalisasi: lowercase, strip `\\?\` prefix
        let s = path.to_string_lossy();
        let s = s.trim_start_matches(r"\\?\");
        let s = s.to_ascii_lowercase();
        // Gunakan hash FNV-1a 64-bit (tidak perlu crate — 10 baris)
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for b in s.bytes() {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{hash:016x}")
    }

    /// Nama tampilan: label kustom atau nama file tanpa ekstensi (fallback).
    #[allow(dead_code)]
    pub fn display_name(&self) -> &str {
        if let Some(lbl) = &self.label {
            return lbl.as_str();
        }
        self.path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn id_deterministik() {
        let p = Path::new(r"C:\Users\User\Desktop\test.lnk");
        let id1 = Item::make_id(p);
        let id2 = Item::make_id(p);
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 16);
    }

    #[test]
    fn id_beda_untuk_path_berbeda() {
        let a = Item::make_id(Path::new(r"C:\Desktop\a.lnk"));
        let b = Item::make_id(Path::new(r"C:\Desktop\b.lnk"));
        assert_ne!(a, b);
    }

    #[test]
    fn kind_dari_ekstensi() {
        assert_eq!(ItemKind::from_path(Path::new("foo.lnk")), ItemKind::App);
        assert_eq!(
            ItemKind::from_path(Path::new("foo.pdf")),
            ItemKind::Document
        );
        assert_eq!(ItemKind::from_path(Path::new("foo.mp4")), ItemKind::Media);
        assert_eq!(ItemKind::from_path(Path::new("foo.xyz")), ItemKind::Other);
    }
}
