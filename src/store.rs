//! Penyimpanan state persisten (spec §8).
//!
//! Tulis atomik: state.json.tmp → flush → rename ke state.json.
//! Jika file rusak: pindahkan ke state.corrupt-<timestamp>.json, mulai kosong.

use crate::model::Item;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Versi skema state.json saat ini.
const SCHEMA_VERSION: u32 = 1;

/// Isi lengkap state.json.
#[derive(Debug, Serialize, Deserialize)]
pub struct State {
    pub schema: u32,
    pub items: Vec<Item>,
    #[serde(default)]
    pub icons_hidden_by_us: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            schema: SCHEMA_VERSION,
            items: Vec::new(),
            icons_hidden_by_us: false,
        }
    }
}

/// Memuat state dari direktori data aplikasi.
/// Jika file tidak ada → State::default().
/// Jika file rusak → pindahkan ke .corrupt-<ts>, kembalikan State::default().
pub fn load(dir: &Path) -> State {
    let path = dir.join("state.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return State::default(),
        Err(e) => {
            log_warn!("gagal membaca state.json: {e}");
            return State::default();
        }
    };
    match serde_json::from_str::<State>(&text) {
        Ok(mut s) => {
            // Migrasi skema berjenjang — saat ini tidak perlu karena schema=1
            s.schema = SCHEMA_VERSION;
            s
        }
        Err(e) => {
            log_warn!("state.json rusak ({e}); memindahkan dan mulai kosong");
            quarantine(&path, dir);
            State::default()
        }
    }
}

/// Menyimpan state secara atomik (tmp → rename).
pub fn save(dir: &Path, state: &State) -> Result<(), std::io::Error> {
    let tmp = dir.join("state.json.tmp");
    let dst = dir.join("state.json");
    let text = serde_json::to_string_pretty(state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, &text)?;
    std::fs::rename(&tmp, &dst)?;
    Ok(())
}

/// Memindahkan file rusak ke `state.corrupt-<timestamp>.json`.
fn quarantine(path: &Path, dir: &Path) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let corrupt: PathBuf = dir.join(format!("state.corrupt-{ts}.json"));
    if let Err(e) = std::fs::rename(path, &corrupt) {
        log_warn!("gagal memindahkan state.json ke {}: {e}", corrupt.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_file_tidak_ada_mengembalikan_default() {
        let dir = tempdir().expect("tempdir");
        let s = load(dir.path());
        assert_eq!(s.schema, 1);
        assert!(s.items.is_empty());
    }

    #[test]
    fn simpan_dan_muat_ulang() {
        let dir = tempdir().expect("tempdir");
        let mut state = State::default();
        state.items.push(crate::model::Item {
            id: "abc".to_owned(),
            path: PathBuf::from(r"C:\test.lnk"),
            label: None,
            kind: crate::model::ItemKind::App,
            score: 2.5,
            score_t0: 1_700_000_000,
            total_launches: 1,
            last_launch: Some(1_700_000_000),
            first_seen: 1_699_000_000,
            pinned_pos: None,
            hidden: false,
            missing: false,
            missing_since: None,
        });
        save(dir.path(), &state).expect("simpan");

        let loaded = load(dir.path());
        assert_eq!(loaded.items.len(), 1);
        assert_eq!(loaded.items[0].id, "abc");
    }

    #[test]
    fn file_rusak_dipindahkan_dan_dikosongkan() {
        let dir = tempdir().expect("tempdir");
        std::fs::write(dir.path().join("state.json"), b"ini bukan json").expect("tulis");
        let s = load(dir.path());
        // State dikosongkan
        assert!(s.items.is_empty());
        // File asli dipindahkan; tidak ada lagi state.json
        assert!(!dir.path().join("state.json").exists());
        // Ada file corrupt
        let corrupt: Vec<_> = std::fs::read_dir(dir.path())
            .expect("readdir")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("state.corrupt-")
            })
            .collect();
        assert_eq!(corrupt.len(), 1);
    }
}
