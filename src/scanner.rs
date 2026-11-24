//! Pemindai item desktop dan watcher perubahan file (spec §13).
//!
//! Scan awal non-rekursif pada folder Desktop pengguna dan Public Desktop.
//! Watcher `notify` + debouncer 300 ms mengirim event rekonsiliasi ke channel.

use crate::config::Sources as SourcesCfg;
use crate::model::{Item, ItemKind};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Event yang dikirim watcher ke main thread.
#[derive(Debug)]
pub enum ScanEvent {
    /// Item baru terdeteksi.
    Added(PathBuf),
    /// Item dihapus.
    Removed(PathBuf),
    /// Item diganti nama (rename dalam folder yang sama).
    Renamed { from: PathBuf, to: PathBuf },
}

/// Direktori Desktop pengguna.
pub fn user_desktop() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        crate::platform::icons::desktop_dir()
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .map(|p| p.join("Desktop"))
    }
}

/// Direktori Public Desktop.
pub fn public_desktop() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        crate::platform::icons::public_desktop_dir()
    }
    #[cfg(not(windows))]
    {
        // Biasanya C:\Users\Public\Desktop
        std::env::var_os("PUBLIC")
            .map(PathBuf::from)
            .map(|p| p.join("Desktop"))
    }
}

/// Mengambil nama tampilan Explorer untuk sebuah path (mis. menyembunyikan ekstensi `.lnk`).
pub fn resolve_display_name(path: &std::path::Path) -> Option<String> {
    #[cfg(windows)]
    {
        crate::platform::icons::shell_display_name(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

/// Memeriksa apakah atribut berkas Windows bertanda hidden atau system.
fn is_hidden_or_system(entry: &std::fs::DirEntry) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
        if let Ok(meta) = entry.metadata() {
            let attrs = meta.file_attributes();
            return (attrs & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM)) != 0;
        }
    }
    #[cfg(not(windows))]
    let _ = entry;
    false
}

/// Scan satu direktori secara non-rekursif.
/// Mengembalikan `Vec<Item>` baru untuk setiap entri yang lolos filter.
pub fn scan_dir(dir: &std::path::Path, cfg: &SourcesCfg) -> Vec<Item> {
    let now_secs = unix_now();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            log_warn!("gagal membaca direktori {}: {e}", dir.display());
            return Vec::new();
        }
    };
    let mut items = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_owned(),
            None => continue,
        };
        if should_ignore(&name, cfg) {
            continue;
        }
        if cfg.ignore_hidden && is_hidden_or_system(&entry) {
            continue;
        }
        let id = Item::make_id(&path);
        let kind = ItemKind::from_path(&path);
        let label = resolve_display_name(&path);
        items.push(Item {
            id,
            path,
            label,
            kind,
            score: 0.0,
            score_t0: now_secs,
            total_launches: 0,
            last_launch: None,
            first_seen: now_secs,
            pinned_pos: None,
            hidden: false,
            missing: false,
            missing_since: None,
        });
    }
    items
}

/// Rekonsiliasi: gabungkan item dari disk dengan state tersimpan.
///
/// - Item baru di disk yang tidak ada di store: tambah.
/// - Item di store yang ada di disk: perbarui path & label tampilan jika perlu, pertahankan skor.
/// - Item di store yang tidak ada di disk: tandai `missing`, buang setelah 30 hari.
///
/// Mengembalikan daftar item final.
pub fn reconcile(stored: Vec<Item>, disk: Vec<Item>) -> Vec<Item> {
    use std::collections::{HashMap, HashSet};

    let mut by_id: HashMap<String, Item> = stored.into_iter().map(|i| (i.id.clone(), i)).collect();
    let mut seen_on_disk: HashSet<String> = HashSet::with_capacity(disk.len());
    let now_secs = unix_now();

    for disk_item in disk {
        seen_on_disk.insert(disk_item.id.clone());
        match by_id.get_mut(&disk_item.id) {
            Some(entry) => {
                // Perbarui path, jenis, label tampilan, dan pulihkan jika sebelumnya missing.
                entry.path = disk_item.path;
                entry.kind = disk_item.kind;
                if disk_item.label.is_some() {
                    entry.label = disk_item.label;
                }
                entry.missing = false;
                entry.missing_since = None;
            }
            None => {
                by_id.insert(disk_item.id.clone(), disk_item);
            }
        }
    }

    // Tandai semua yang tidak ditemukan di disk sebagai missing, buang yang sudah > 30 hari.
    const MISSING_RETENTION_SECS: i64 = 30 * 86_400;
    by_id.retain(|id, item| {
        if seen_on_disk.contains(id) {
            return true;
        }
        if !item.missing {
            item.missing = true;
            item.missing_since = Some(now_secs);
            true
        } else if let Some(since) = item.missing_since {
            now_secs - since <= MISSING_RETENTION_SECS
        } else {
            item.missing_since = Some(now_secs);
            true
        }
    });

    by_id.into_values().collect()
}

/// Memulai watcher file dan mengembalikan `Receiver<ScanEvent>`.
/// Watcher berjalan di thread background.
/// Mengembalikan `None` jika direktori tidak bisa diwatch.
pub fn start_watcher(
    dirs: Vec<PathBuf>,
    tx: mpsc::SyncSender<ScanEvent>,
    on_wake: impl Fn() + Send + Sync + 'static,
) -> Option<notify::RecommendedWatcher> {
    use notify::{Config, RecursiveMode, Watcher};

    let tx2 = tx.clone();
    let mut watcher = notify::RecommendedWatcher::new(
        move |res: notify::Result<notify::Event>| {
            let ev = match res {
                Ok(e) => e,
                Err(e) => {
                    log_warn!("watcher error: {e}");
                    return;
                }
            };
            if handle_notify_event(ev, &tx2) {
                on_wake();
            }
        },
        Config::default().with_poll_interval(std::time::Duration::from_millis(300)),
    )
    .ok()?;

    let mut ok = false;
    for dir in &dirs {
        if watcher.watch(dir, RecursiveMode::NonRecursive).is_ok() {
            ok = true;
        } else {
            log_warn!("tidak bisa memonitor {}", dir.display());
        }
    }
    if ok {
        Some(watcher)
    } else {
        None
    }
}

fn handle_notify_event(ev: notify::Event, tx: &mpsc::SyncSender<ScanEvent>) -> bool {
    use notify::event::{ModifyKind, RenameMode};
    use notify::EventKind::*;
    let mut sent = false;
    match ev.kind {
        Create(_) => {
            for path in ev.paths {
                let _ = tx.try_send(ScanEvent::Added(path));
                sent = true;
            }
        }
        Remove(_) => {
            for path in ev.paths {
                let _ = tx.try_send(ScanEvent::Removed(path));
                sent = true;
            }
        }
        Modify(ModifyKind::Name(RenameMode::Both)) if ev.paths.len() >= 2 => {
            let _ = tx.try_send(ScanEvent::Renamed {
                from: ev.paths[0].clone(),
                to: ev.paths[1].clone(),
            });
            sent = true;
        }
        Modify(ModifyKind::Name(RenameMode::From)) => {
            for path in ev.paths {
                let _ = tx.try_send(ScanEvent::Removed(path));
                sent = true;
            }
        }
        Modify(ModifyKind::Name(RenameMode::To)) => {
            for path in ev.paths {
                let _ = tx.try_send(ScanEvent::Added(path));
                sent = true;
            }
        }
        Modify(ModifyKind::Name(RenameMode::Any)) => {
            for path in ev.paths {
                if path.exists() {
                    let _ = tx.try_send(ScanEvent::Added(path));
                } else {
                    let _ = tx.try_send(ScanEvent::Removed(path));
                }
                sent = true;
            }
        }
        _ => {}
    }
    sent
}

/// Menyaring nama file berdasarkan konfigurasi `sources`.
pub fn should_ignore(name: &str, cfg: &SourcesCfg) -> bool {
    if cfg.ignore_hidden && name.starts_with('.') {
        return true;
    }
    for pattern in &cfg.ignore_patterns {
        if glob_match(pattern, name) {
            return true;
        }
    }
    false
}

/// Pencocokan glob sederhana (hanya mendukung `*` sebagai wildcard).
fn glob_match(pattern: &str, name: &str) -> bool {
    if pattern == name {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return name.starts_with(prefix);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return name.ends_with(suffix);
    }
    false
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn dummy_cfg() -> SourcesCfg {
        SourcesCfg {
            include: vec!["desktop".to_owned()],
            ignore_patterns: vec!["desktop.ini".to_owned(), "~$*".to_owned()],
            ignore_hidden: true,
        }
    }

    #[test]
    fn filter_desktop_ini() {
        let cfg = dummy_cfg();
        assert!(should_ignore("desktop.ini", &cfg));
        assert!(!should_ignore("foto.jpg", &cfg));
    }

    #[test]
    fn filter_tilde_wildcard() {
        let cfg = dummy_cfg();
        assert!(should_ignore("~$dokumen.docx", &cfg));
        assert!(!should_ignore("dokumen.docx", &cfg));
    }

    #[test]
    fn filter_hidden() {
        let cfg = dummy_cfg();
        assert!(should_ignore(".hidden", &cfg));
    }

    #[test]
    fn scan_dir_menghasilkan_item() {
        let dir = tempdir().expect("tempdir");
        std::fs::write(dir.path().join("catatan.txt"), b"isi").expect("tulis");
        std::fs::write(dir.path().join("desktop.ini"), b"[.ShellClassInfo]").expect("tulis");
        let cfg = dummy_cfg();
        let items = scan_dir(dir.path(), &cfg);
        assert_eq!(items.len(), 1);
        assert!(items[0].path.ends_with("catatan.txt"));
    }

    #[test]
    fn rekonsiliasi_pertahankan_skor() {
        let now = unix_now();
        let stored = vec![Item {
            id: "aaa".to_owned(),
            path: PathBuf::from(r"C:\Desktop\a.lnk"),
            label: None,
            kind: ItemKind::App,
            score: 5.0,
            score_t0: now,
            total_launches: 3,
            last_launch: None,
            first_seen: now,
            pinned_pos: None,
            hidden: false,
            missing: false,
            missing_since: None,
        }];
        let disk = vec![Item {
            id: "aaa".to_owned(),
            path: PathBuf::from(r"C:\Desktop\a-renamed.lnk"),
            label: None,
            kind: ItemKind::App,
            score: 0.0,
            score_t0: now,
            total_launches: 0,
            last_launch: None,
            first_seen: now,
            pinned_pos: None,
            hidden: false,
            missing: false,
            missing_since: None,
        }];
        let result = reconcile(stored, disk);
        let item = result.iter().find(|i| i.id == "aaa").expect("harus ada");
        // Skor dari store dipertahankan
        assert_eq!(item.score, 5.0);
        // Path diperbarui ke versi disk (rename)
        assert!(item.path.ends_with("a-renamed.lnk"));
    }

    #[test]
    fn rekonsiliasi_tandai_item_hilang_sebagai_missing() {
        let now = unix_now();
        let stored = vec![Item {
            id: "hilang".to_owned(),
            path: PathBuf::from(r"C:\Desktop\hilang.txt"),
            label: None,
            kind: ItemKind::Document,
            score: 3.0,
            score_t0: now,
            total_launches: 2,
            last_launch: None,
            first_seen: now,
            pinned_pos: None,
            hidden: false,
            missing: false,
            missing_since: None,
        }];
        let result = reconcile(stored, Vec::new());
        assert_eq!(result.len(), 1);
        assert!(result[0].missing);
        assert!(result[0].missing_since.is_some());
    }
}
