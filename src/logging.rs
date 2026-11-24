//! Logger berkas sederhana: satu berkas dengan rotasi maksimum 1 MB × 3.
//!
//! Dipilih dibanding `tracing` demi jumlah dependensi dan RAM (§14): kebutuhannya
//! hanya menulis baris teks ke berkas. Bila kebutuhan tumbuh (level, span,
//! filtr), ganti dengan `tracing` + `tracing-appender`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Ukuran maksimum satu berkas log sebelum dirotasi (1 MB).
const MAX_LOG_BYTES: u64 = 1024 * 1024;
/// Jumlah berkas log yang disimpan, termasuk yang aktif.
const MAX_LOG_FILES: usize = 3;

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Menyiapkan logger di `dir`. Aman dipanggil sekali per proses.
pub fn init(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let path = dir.join("bubbledesk.log");
    rotate_if_needed(&path, MAX_LOG_BYTES);
    let _ = LOG_PATH.set(path);
    Ok(())
}

/// Menulis satu baris log. Tidak melakukan apa pun bila `init` belum dipanggil.
pub fn write(level: &str, message: &str) {
    let Some(path) = LOG_PATH.get() else {
        return;
    };
    rotate_if_needed(path, MAX_LOG_BYTES);
    let line = format!(
        "{} [{}] {}\n",
        crate::platform::now_local_string(),
        level,
        message
    );
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Menggeser berkas log bila ukurannya mencapai `max_bytes`.
///
/// Menyimpan `base`, `base.1`, `base.2`; berkas terlama dibuang.
fn rotate_if_needed(path: &Path, max_bytes: u64) {
    let over = fs::metadata(path)
        .map(|m| m.len() >= max_bytes)
        .unwrap_or(false);
    if !over {
        return;
    }
    let _ = fs::remove_file(rotated(path, MAX_LOG_FILES - 1));
    for n in (1..MAX_LOG_FILES - 1).rev() {
        let _ = fs::rename(rotated(path, n), rotated(path, n + 1));
    }
    let _ = fs::rename(path, rotated(path, 1));
}

fn rotated(path: &Path, n: usize) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{n}"));
    PathBuf::from(name)
}

/// Menulis baris log level INFO.
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => { $crate::logging::write("INFO", &format!($($arg)*)) };
}

/// Menulis baris log level WARN.
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => { $crate::logging::write("WARN", &format!($($arg)*)) };
}

/// Menulis baris log level ERROR.
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => { $crate::logging::write("ERROR", &format!($($arg)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(path: &Path, bytes: &[u8]) {
        assert!(fs::write(path, bytes).is_ok());
    }

    #[test]
    fn rotation_shifts_files_and_drops_oldest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path().join("x.log");
        write_file(&base, b"0123456789");
        write_file(&rotated(&base, 1), b"one");
        write_file(&rotated(&base, 2), b"oldest");

        rotate_if_needed(&base, 4); // 10 >= 4 -> rotasi

        assert!(!base.exists(), "berkas aktif harus digeser");
        assert_eq!(fs::read(rotated(&base, 1)).expect("read .1"), b"0123456789");
        assert_eq!(fs::read(rotated(&base, 2)).expect("read .2"), b"one");
        assert!(!rotated(&base, 3).exists(), "tidak boleh ada berkas ke-4");
    }

    #[test]
    fn no_rotation_below_threshold() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path().join("y.log");
        write_file(&base, b"hi");

        rotate_if_needed(&base, 100);

        assert!(base.exists(), "berkas kecil tidak boleh dirotasi");
        assert!(!rotated(&base, 1).exists());
    }
}
