//! Semua string antarmuka pengguna (bahasa Indonesia) terpusat di sini
//! agar mudah dilokalisasi (AGENTS.md §2, spec §21).

// ─── Menu konteks (Ctrl + klik kiri, spec §12) ────────────────────────────────

pub const MENU_OPEN: &str = "Buka";
pub const MENU_OPEN_LOCATION: &str = "Buka lokasi file";
pub const MENU_COPY_PATH: &str = "Salin path";
pub const MENU_RENAME_LABEL: &str = "Ganti nama…";
pub const MENU_RESET_SCORE: &str = "Reset hitungan";
pub const MENU_UNPIN: &str = "Lepas pin";
pub const MENU_HIDE: &str = "Sembunyikan";

// ─── Dialog ganti nama file / label ───────────────────────────────────────────

pub const RENAME_DIALOG_TITLE: &str = "Ganti nama";
pub const RENAME_DIALOG_PROMPT: &str = "Nama baru (mengganti nama item di desktop):";
pub const RENAME_DIALOG_OK: &str = "Simpan";
pub const RENAME_DIALOG_CANCEL: &str = "Batal";

pub const ERR_RENAME_INVALID_NAME: &str = "Nama file mengandung karakter tidak valid";
pub const ERR_RENAME_EXISTS: &str = "Nama file sudah digunakan di desktop";

pub fn format_rename_failed(err: &str) -> String {
    format!("Gagal mengganti nama: {err}")
}

// ─── Tooltip (spec §12) ───────────────────────────────────────────────────────

pub const TOOLTIP_NEVER_OPENED: &str = "Belum pernah dibuka";

/// Memformat baris statistik tooltip: `"Dibuka N× · terakhir <relatif>"`.
pub fn format_tooltip_stats(
    total_launches: u64,
    last_launch: Option<i64>,
    now_secs: i64,
) -> String {
    if total_launches == 0 {
        return TOOLTIP_NEVER_OPENED.to_owned();
    }
    match last_launch {
        Some(ts) => {
            let rel = format_relative_time((now_secs - ts).max(0));
            format!("Dibuka {total_launches}× · terakhir {rel}")
        }
        None => format!("Dibuka {total_launches}×"),
    }
}

/// Memformat selisih waktu (detik) menjadi teks relatif bahasa Indonesia.
pub fn format_relative_time(elapsed_secs: i64) -> String {
    if elapsed_secs < 60 {
        return "baru saja".to_owned();
    }
    let mins = elapsed_secs / 60;
    if mins < 60 {
        return format!("{mins} menit lalu");
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{hours} jam lalu");
    }
    let days = hours / 24;
    format!("{days} hari lalu")
}

// ─── Pesan error peluncuran (FR-3, spec §13) ──────────────────────────────────

pub const ERR_FILE_NOT_FOUND: &str = "File atau target tidak ditemukan";
pub const ERR_PATH_NOT_FOUND: &str = "Lokasi folder atau drive tidak ditemukan";
pub const ERR_ACCESS_DENIED: &str = "Akses ditolak saat membuka item";

pub fn format_launch_error_code(code: u32) -> String {
    format!("Gagal membuka item (kode {code})")
}

// ─── Ikon tray (M5) ───────────────────────────────────────────────────────────

pub const TRAY_TOOLTIP: &str = "BubbleDesk";
pub const TRAY_SHOW_ICONS: &str = "Tampilkan ikon desktop asli";
pub const TRAY_HIDE_ICONS: &str = "Sembunyikan ikon desktop";
pub const TRAY_PAUSE: &str = "Jeda";
pub const TRAY_RESUME: &str = "Lanjutkan";
pub const TRAY_OVERLAY_SHOW: &str = "Tampilkan overlay debug";
pub const TRAY_OVERLAY_HIDE: &str = "Sembunyikan overlay debug";
pub const TRAY_OPEN_DATA: &str = "Buka folder data";
pub const TRAY_QUIT: &str = "Keluar";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_belum_pernah_dibuka() {
        assert_eq!(format_tooltip_stats(0, None, 1000), "Belum pernah dibuka");
    }

    #[test]
    fn tooltip_waktu_relatif() {
        assert_eq!(
            format_tooltip_stats(3, Some(1000 - 30), 1000),
            "Dibuka 3× · terakhir baru saja"
        );
        assert_eq!(
            format_tooltip_stats(5, Some(1000 - 180), 1000),
            "Dibuka 5× · terakhir 3 menit lalu"
        );
        assert_eq!(
            format_tooltip_stats(12, Some(1000 - 7200), 1000),
            "Dibuka 12× · terakhir 2 jam lalu"
        );
        assert_eq!(
            format_tooltip_stats(2, Some(1000 - 3 * 86_400), 1000),
            "Dibuka 2× · terakhir 3 hari lalu"
        );
    }
}
