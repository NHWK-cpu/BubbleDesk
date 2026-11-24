# AGENTS.md — BubbleDesk

Panduan kerja untuk agen AI yang membangun proyek ini. Baca seluruh file ini dan `docs/spec.md` sebelum menulis kode. Jika dua dokumen bertentangan, `docs/spec.md` menang, kecuali bagian "Keputusan yang sudah diambil" di bawah.

## 1. Ringkasan proyek

BubbleDesk adalah aplikasi Windows ringan yang mengubah desktop menjadi wallpaper hidup berisi gelembung (bubble). Setiap file, folder, dan shortcut di desktop diwakili satu bubble berisi ikonnya. Semakin sering item dibuka lewat BubbleDesk, semakin besar bubble-nya (ada ukuran maksimum). Item yang lama tak dipakai perlahan mengecil.

- Bahasa: Rust. Target build: `x86_64-pc-windows-msvc`.
- OS utama: **Windows 10 22H2**. Windows 11 adalah target sekunder, dikerjakan setelah M5.
- Prioritas utama: RAM kecil, CPU nyaris nol saat diam, animasi mulus, tidak pernah merusak file pengguna.

## 2. Keputusan yang sudah diambil

Ini mengesampingkan "keputusan terbuka" di spesifikasi (§21) sampai manusia mengubahnya:

- Nama produk: BubbleDesk (crate `bubbledesk`).
- Klik tunggal membuka item (`click_mode = "double"`).
- Lisensi: GNU Affero General Public License v3.0 (`AGPL-3.0-only`).
- Bahasa antarmuka: Indonesia. Semua string UI ditaruh di satu modul agar mudah dilokalisasi.
- Backend render awal: `wgpu`, dievaluasi ulang setelah pengukuran di M2.
- Tidak ada mode Tidy, impor UserAssist, maupun multi-monitor sebelum M6 dan hanya atas persetujuan manusia.

## 3. Alur kerja (wajib)

1. Kerjakan **satu milestone per sesi**, sesuai urutan di spesifikasi §17 (M0 sampai M7). Jangan melompat dan jangan mengerjakan milestone berikutnya sebelum diminta.
2. Awali setiap milestone dengan ringkasan pemahaman (maksimal 10 baris) dan daftar asumsi atau pertanyaan. Jika ada hal yang menghalangi, tanya manusia sebelum menulis kode.
3. Implementasikan sampai semua kriteria penerimaan (AC) milestone terpenuhi.
4. Akhiri dengan laporan, lalu **berhenti**. Format laporan:
   - Yang selesai (daftar singkat).
   - Hasil tiap AC: lulus, gagal, atau perlu pengujian manual oleh manusia (jelaskan langkahnya).
   - Angka performa terbaru (setelah M2: working set, FPS, frame time).
   - Masalah terbuka dan hal yang perlu keputusan manusia.
5. Hal yang hanya bisa diverifikasi dengan melihat layar (misalnya apakah bubble muncul di belakang ikon desktop) HARUS dilaporkan sebagai "perlu verifikasi manusia". Jangan mengklaim berhasil tanpa bukti.

## 4. Perintah standar

```
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
cargo run -- --dump-desktop-tree     # debug struktur jendela desktop (mulai M1)
```

Sebelum setiap commit, tiga perintah pertama HARUS bersih. Jangan commit kode yang gagal build atau gagal tes.

## 5. Aturan keselamatan (tidak boleh dilanggar)

1. **Jangan pernah** memindah, mengganti nama, atau menghapus berkas pengguna. Satu-satunya lokasi yang boleh ditulis adalah `%APPDATA%\BubbleDesk` dan `%LOCALAPPDATA%\BubbleDesk`. Pengecualian hanya mode Tidy di M6, dengan konfirmasi eksplisit pengguna dan log undo.
2. **Ikon desktop bawaan tidak boleh tertinggal tersembunyi.** Jika BubbleDesk menyembunyikannya, ia HARUS memulihkannya saat keluar normal, di panic hook, dan di startup berikutnya bila flag `icons_hidden_by_us` masih aktif. Menu tray HARUS punya opsi manual "Tampilkan ikon desktop asli".
3. **Jangan pernah menelan input pengguna.** Mouse hook selalu memanggil `CallNextHookEx` dan harus selesai secepat mungkin.
4. Tidak ada akses jaringan, telemetri, atau kebutuhan hak admin. Manifest memakai `asInvoker`.
5. Jika `state.json` rusak, pindahkan ke `state.corrupt-<timestamp>.json`, mulai dari kosong, dan catat ke log. Jangan crash.
6. Jika WorkerW sudah memiliki child jendela asing (aplikasi wallpaper lain), jangan menimpanya. Catat peringatan dan masuk mode kompatibilitas.

## 6. Konvensi kode

- Semua kode `unsafe` dan pemanggilan Win32 hidup di `src/platform/`, dibungkus API aman. Setiap blok `unsafe` diberi komentar `// SAFETY:` yang menjelaskan invarian.
- Dilarang `unwrap()` dan `expect()` di kode non-test. Gunakan `Result` (`thiserror`/`anyhow`) dan catat ke log. Thread UI tidak boleh panic karena kegagalan I/O atau Win32.
- Tidak ada angka ajaib. Nilai ukuran, durasi, dan ambang berasal dari `config.rs` atau konstanta bernama.
- Tidak ada alokasi heap per frame di jalur render dan fisika.
- Gunakan `#[cfg(windows)]` untuk kode khusus Windows.
- Komentar dan dokumentasi dalam bahasa Indonesia; nama identifier dalam bahasa Inggris.
- Logging lewat satu mekanisme (`tracing` atau logger berkas sederhana), berkas di `%APPDATA%\BubbleDesk\bubbledesk.log`, rotasi maksimum 1 MB × 3.

## 7. Dependensi

Gunakan hanya dependensi di spesifikasi §16. Menambah dependensi baru HARUS disertai justifikasi tertulis di PR/commit message: kegunaan, dampak RAM, ukuran binary, jumlah dependensi transitif. Matikan fitur default pada crate bila tidak dipakai. Jangan memakai `tokio` atau runtime async berat, dan jangan memakai `EmptyWorkingSet`.

Profil release (di `Cargo.toml`):

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"   # panic hook tetap berjalan sebelum abort; hook memulihkan ikon desktop
strip = true
```

## 8. Struktur proyek

```
bubbledesk/
  Cargo.toml
  build.rs                 # embed manifest (PerMonitorV2 DPI, longPathAware, asInvoker)
  AGENTS.md
  docs/ (spec.md, perf.md, manual-test.md, desktop-tree-notes.md)
  assets/ (app.ico, manifest.xml)
  src/
    main.rs  app.rs  config.rs  store.rs  model.rs  scoring.rs
    scanner.rs  physics.rs
    render/ (mod.rs, wgpu_backend.rs, atlas.rs, text.rs, shaders/bubble.wgsl)
    platform/ (mod.rs, workerw.rs, input.rs, icons.rs, launch.rs,
               power.rs, wallpaper.rs, autostart.rs, singleton.rs, tray.rs)
  tests/
```

## 9. Hal yang paling rawan (baca ulang sebelum M1)

- **Integrasi desktop (spec §7).** Teknik WorkerW tidak terdokumentasi resmi. Di Windows 10 22H2 gunakan Layout A. Jangan mengasumsikan struktur jendela: deteksi saat runtime, catat pohon jendela lewat `--dump-desktop-tree` ke `docs/desktop-tree-notes.md`, dan sediakan fallback (`HWND_BOTTOM`). Layout B (Windows 11) cukup berupa stub sampai setelah M5.
- **Input.** Jendela berada di bawah lapisan ikon sehingga tidak menerima pesan mouse. Gunakan `WH_MOUSE_LL` di thread khusus, abaikan event jika bukan di atas desktop kosong (`WindowFromPoint`), dan lepas hook saat status `Paused`.
- **Target RAM.** Idle ≤ 40 MB dan aktif ≤ 80 MB (100 bubble) adalah target, bukan fakta. Ukur di M2. Jika idle > 60 MB, laporkan dan usulkan backend CPU di balik trait `Renderer`; jangan memutuskan sendiri.
- **Ikon.** Satu keputusan konsisten tentang premultiplied vs straight alpha dari ekstraksi sampai shader.
- **Menu klik kanan.** Butuh jendela helper tersembunyi sebagai owner, `SetForegroundWindow` sebelum `TrackPopupMenu`, dan `WM_NULL` sesudahnya.

## 10. Jika terjadi hal tak terduga

- Perilaku Windows berbeda dari spesifikasi: jangan menebak. Catat temuan di `docs/desktop-tree-notes.md`, sesuaikan implementasi seperlunya, dan laporkan ke manusia.
- Spesifikasi ambigu atau saling bertentangan: tanya manusia, jangan memilih sendiri.
- Target RAM, FPS, atau CPU tidak tercapai: laporkan angka aktual dan hipotesis penyebab sebelum mengubah arsitektur.
- AC tidak bisa diverifikasi di lingkunganmu (misalnya tidak ada GPU atau layar): tulis langkah uji manual di `docs/manual-test.md` dan minta manusia menjalankannya.

## 11. Definition of Done per milestone

- Semua AC milestone terpenuhi dan dicatat di laporan.
- `cargo fmt`, `cargo clippy -D warnings`, dan `cargo test` bersih.
- Uji manual yang relevan dijalankan (atau diserahkan ke manusia dengan langkah jelas) dan dicatat di `docs/manual-test.md`.
- Angka performa terbaru ada di `docs/perf.md` dan masih dalam anggaran (atau ada catatan alasan dan rencana perbaikan).
- Dokumen yang terdampak di `docs/` diperbarui. Dokumen yang usang dianggap bug.
- Tidak ada `TODO` tanpa penjelasan.
