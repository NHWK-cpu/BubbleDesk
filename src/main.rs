//! BubbleDesk — titik masuk aplikasi.
//!
//! M1: attach jendela warna solid ke desktop dan `--dump-desktop-tree`.
//! Belum ada ikon, fisika, atau input — itu mulai dikerjakan di M2/M3/M4.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("BubbleDesk hanya mendukung Windows.");

#[macro_use]
mod logging;
mod app;
mod config;
mod model;
mod physics;
mod platform;
mod render;
mod scanner;
mod scoring;
mod store;
mod strings;

use std::process::ExitCode;
use std::time::Duration;

fn main() -> ExitCode {
    let dir = platform::app_data_dir();
    if let Err(e) = logging::init(&dir) {
        eprintln!("BubbleDesk: gagal menyiapkan log: {e}");
    }
    log_info!("BubbleDesk {} mulai", env!("CARGO_PKG_VERSION"));

    // Pemulihan ikon desktop bila proses berakhir karena panic (AGENTS §5.2):
    // hook berjalan sebelum `panic = "abort"`, jadi ikon asli tidak tertinggal tersembunyi.
    {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log_error!("panic: {info}");
            platform::icons::force_show_native_icons();
            previous(info);
        }));
    }

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--dump-desktop-tree") {
        log_info!("mode --dump-desktop-tree");
        platform::workerw::dump_desktop_tree();
        return ExitCode::SUCCESS;
    }

    if args.iter().any(|arg| arg == "--uninstall-cleanup") {
        log_info!("mode --uninstall-cleanup");
        if let Err(e) = platform::autostart::set_enabled(false) {
            log_warn!("gagal menghapus entri autostart saat uninstall: {e}");
        }
        let mut state = store::load(&dir);
        if state.icons_hidden_by_us {
            platform::icons::force_show_native_icons();
            state.icons_hidden_by_us = false;
            if let Err(e) = store::save(&dir, &state) {
                log_warn!("gagal menyimpan state saat uninstall: {e}");
            }
        }
        return ExitCode::SUCCESS;
    }

    let _guard = match platform::acquire_single_instance() {
        Ok(Some(guard)) => guard,
        Ok(None) => {
            log_info!("instance lain sudah berjalan; keluar dengan bersih");
            eprintln!("BubbleDesk sudah berjalan.");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            log_warn!("gagal memeriksa single instance: {e}");
            eprintln!("BubbleDesk: peringatan single instance: {e}");
            return ExitCode::SUCCESS;
        }
    };

    // Mode uji: tahan proses tanpa UI, lalu keluar. Dipakai tes single-instance.
    if let Some(ms) = std::env::var("BUBBLEDESK_HOLD_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        log_info!("mode uji: menahan proses {ms} ms tanpa UI");
        std::thread::sleep(Duration::from_millis(ms));
        return ExitCode::SUCCESS;
    }

    let config = config::Config::load(&dir);
    log_info!(
        "konfigurasi dimuat dari {} (click_mode={:?}, background={:?})",
        dir.join("config.toml").display(),
        config.general.click_mode,
        config.render.background
    );

    // Selaraskan entri autostart registry dengan konfigurasi (spec §15, M5).
    platform::autostart::sync_with_config(config.general.start_with_windows);

    // Bantuan pengujian: jalankan UI sungguhan lalu keluar sendiri setelah N ms.
    let exit_after_ms = std::env::var("BUBBLEDESK_EXIT_AFTER_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok());

    match app::run(config, exit_after_ms) {
        Ok(()) => {
            log_info!("BubbleDesk berhenti");
            ExitCode::SUCCESS
        }
        Err(e) => {
            log_error!("loop event berhenti dengan error: {e}");
            ExitCode::FAILURE
        }
    }
}
