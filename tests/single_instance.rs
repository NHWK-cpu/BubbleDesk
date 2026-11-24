//! Uji integrasi AC M0: instance kedua keluar dengan bersih.

use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn second_instance_exits_cleanly() {
    let exe = env!("CARGO_BIN_EXE_bubbledesk");
    let dir = tempfile::tempdir().expect("tempdir");

    // Instance pertama menahan mutex beberapa detik.
    let mut first = Command::new(exe)
        .env("BUBBLEDESK_DATA_DIR", dir.path())
        .env("BUBBLEDESK_HOLD_MS", "5000")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("jalankan instance pertama");

    // Beri waktu instance pertama mengakuisisi mutex.
    std::thread::sleep(Duration::from_millis(800));

    let second = Command::new(exe)
        .env("BUBBLEDESK_DATA_DIR", dir.path())
        .env("BUBBLEDESK_HOLD_MS", "5000")
        .output()
        .expect("jalankan instance kedua");

    let _ = first.kill();
    let _ = first.wait();

    assert!(
        second.status.success(),
        "instance kedua harus keluar dengan kode 0"
    );
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("sudah berjalan"),
        "instance kedua harus melaporkan instance lain sedang berjalan; stderr: {stderr}"
    );
}
