<div align="center">

<h1><img src="assets/BubbleDesk_icon.png" alt="Logo BubbleDesk" width="40" align="middle" />&nbsp;BubbleDesk</h1>

**Aplikasi Windows ringan yang mengubah item desktop menjadi wallpaper hidup berisi gelembung (*bubble*) adaptif.**

<p>
  <a href="README.md">English</a> ·
  <a href="README.id.md"><strong>Bahasa Indonesia</strong></a>
</p>

[![Release](https://img.shields.io/github/v/release/NHWK-cpu/BubbleDesk?style=flat-square&color=0078D4&label=Rilis)](https://github.com/NHWK-cpu/BubbleDesk/releases/tag/v0.1.0)
[![CI](https://img.shields.io/github/actions/workflow/status/NHWK-cpu/BubbleDesk/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/NHWK-cpu/BubbleDesk/actions/workflows/ci.yml)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%2022H2%2B-0078D6?style=flat-square&logo=windows)](https://github.com/NHWK-cpu/BubbleDesk/releases/tag/v0.1.0)
[![Rust](https://img.shields.io/badge/Dibangun%20dengan-Rust%20%2B%20wgpu-orange?style=flat-square&logo=rust)](Cargo.toml)
[![License](https://img.shields.io/badge/Lisensi-AGPL--3.0-blue?style=flat-square)](LICENSE)

<p>
  <a href="#-pratinjau">Pratinjau</a> •
  <a href="#-fitur-utama">Fitur</a> •
  <a href="#-pemasangan">Pemasangan</a> •
  <a href="#-kontrol--penggunaan">Kontrol</a> •
  <a href="#-konfigurasi">Konfigurasi</a> •
  <a href="#-build-dari-source">Build</a> •
  <a href="#-roadmap">Roadmap</a>
</p>

</div>

---

## ✨ Pratinjau

<div align="center">
  <img src="assets/demo.gif" alt="Demo BubbleDesk" width="760" />
  <br />
  <sub>Gelembung tersusun lembut di atas wallpaper, merespons hover dan drag, serta membesar sesuai frekuensi pemakaian.</sub>
</div>

---

## 🚀 Fitur Utama

Setiap file, folder, dan shortcut di Desktop Windows diwakili satu bubble interaktif yang berjalan langsung di belakang ikon desktop.

<table>
  <tr>
    <td width="50%" valign="top">
      <h4>🫧 Ukuran Adaptif Berbasis Pemakaian</h4>
      <p>Item yang sering dibuka perlahan membesar (hingga batas maksimum); item yang jarang dipakai perlahan mengecil kembali.</p>
    </td>
    <td width="50%" valign="top">
      <h4>🪟 Terintegrasi di Lapisan Wallpaper</h4>
      <p>Menempel di belakang ikon lewat Windows <code>WorkerW</code>, tetap tampil saat <code>Win+D</code>, dan otomatis menempel ulang jika Explorer restart.</p>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <h4>⚡ Hemat Daya & Anti-Tumpuk</h4>
      <p>Fisika otomatis masuk mode tidur (<strong>~0% CPU</strong>, 0 frame saat diam). Otomatis jeda saat game/aplikasi layar penuh, <code>Win+L</code>, atau layar mati.</p>
    </td>
    <td width="50%" valign="top">
      <h4>🔄 Sinkronisasi Real-Time</h4>
      <p>Menambah, mengganti nama, atau menghapus file di Desktop langsung memperbarui tampilan bubble secara otomatis.</p>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <h4>🛡️ Aman & Tidak Merusak File</h4>
      <p>Tanpa hak administrator, tanpa akses jaringan, dan tanpa telemetri. Tidak pernah memindah atau menghapus file pengguna di balik layar.</p>
    </td>
    <td width="50%" valign="top">
      <h4>🔁 Pemulihan Ikon Bawaan Otomatis</h4>
      <p>Jika ikon desktop asli disembunyikan, BubbleDesk memulihkannya saat keluar normal, saat terjadi <em>crash</em> (panic hook), saat startup berikutnya, atau lewat menu tray.</p>
    </td>
  </tr>
</table>

---

## 📦 Pemasangan

### Opsi 1 — Installer MSI (Disarankan)

1. Unduh `BubbleDesk-0.1.0-x86_64.msi` dari halaman [**Rilis v0.1.0**](https://github.com/NHWK-cpu/BubbleDesk/releases/tag/v0.1.0).
2. Jalankan installer — dipasang per pengguna di `%LOCALAPPDATA%\Programs\BubbleDesk` **tanpa hak administrator**.
3. Jalankan **BubbleDesk** dari Start Menu.

> **Pencopotan Bersih:** Menghapus aplikasi lewat **Apps & Features** otomatis membersihkan entri autostart `HKCU\...\Run\BubbleDesk` dan memulihkan ikon desktop asli jika sedang tersembunyi.

### Opsi 2 — Paket Portabel (`.zip`)

1. Unduh arsip `.zip` dari halaman [**Rilis v0.1.0**](https://github.com/NHWK-cpu/BubbleDesk/releases/tag/v0.1.0).
2. Ekstrak ke folder pilihan Anda.
3. Jalankan `bubbledesk.exe`.

> **Kebutuhan Sistem:** Windows 10 22H2 (`x86_64`) dengan dukungan DirectX 12. Pada Windows 11 saat ini memakai mode kompatibilitas (`HWND_BOTTOM`); pengujian `WorkerW` Layout B dijadwalkan setelah M5.

---

## 🎮 Kontrol & Penggunaan

| Interaksi | Aksi |
|---|---|
| **Klik ganda** *(default)* / Klik tunggal | Membuka file, folder, atau shortcut |
| **Seret (Drag)** | Memindahkan posisi bubble dan menahannya (*pin*) |
| **Arahkan kursor (Hover)** | Menampilkan tooltip nama lengkap, jumlah dibuka, dan waktu terakhir dibuka |
| **Ctrl + Klik kiri** | Membuka menu konteks *(Buka, Buka lokasi file, Salin path, Ganti nama, Reset hitungan, Lepas pin, Sembunyikan)* |
| **Ikon Tray (Klik kanan)** | Tampilkan/sembunyikan ikon desktop asli, Jeda/Lanjutkan, Overlay debug, Buka folder data, Keluar |

---

## ⚙️ Konfigurasi

Seluruh konfigurasi dan data tersimpan lokal di `%APPDATA%\BubbleDesk\`:

| Berkas | Fungsi |
|---|---|
| `config.toml` | Pengaturan aplikasi (mendukung **hot-reload** saat berjalan) |
| `state.json` | Riwayat skor, jumlah dibuka, dan posisi pin bubble |
| `bubbledesk.log` | Catatan diagnostik lokal (rotasi `1 MB × 3`) |

```toml
[general]
click_mode = "double"       # "double" atau "single"
start_with_windows = false  # kelola entri HKCU\...\Run
hide_native_icons = true    # sembunyikan ikon desktop bawaan saat aktif
reduce_motion = false

[render]
background = "wallpaper"    # "wallpaper", "gradient", atau "solid"
fps_cap = 60                # batas FPS saat aktif
fps_cap_on_battery = 30     # batas FPS saat menggunakan baterai
```

<details>
<summary><strong>📄 Referensi lengkap <code>config.toml</code> & nilai default</strong></summary>

```toml
[general]
start_with_windows = false
monitor = "primary"
click_mode = "double"
hide_native_icons = true
reduce_motion = false

[sources]
include = ["desktop", "public_desktop"]
ignore_patterns = ["desktop.ini", "Thumbs.db", "~$*"]
ignore_hidden = true

[bubbles]
r_min = 28.0
r_max = 110.0
score_half_life_days = 30.0
score_cap = 40.0
min_hit_radius = 20.0
label_always_min_radius = 60.0
gap = 6.0
crowd_budget = 0.55

[physics]
damping = 3.0
drift = 0.5
restitution_wall = 0.3
restitution_body = 0.2

[render]
background = "wallpaper"
background_dim = 0.35
background_blur = 6
solid_color = "#101820"
fps_cap = 60
fps_cap_on_battery = 30

[power]
pause_on_fullscreen = true
pause_on_lock = true

[debug]
overlay = false
log_level = "info"
```

</details>

---

## 📊 Performa

Diukur pada **Windows 10 22H2** (AMD Radeon Graphics, backend `wgpu` DX12, VSync `Fifo`). Detail lengkap tersedia di [`docs/perf.md`](docs/perf.md).

| Metrik | Hasil Pengukuran |
|---|---|
| **Frame Rate Aktif** | `~59.3 – 60.1 FPS` (frame time `16.6 ms`) |
| **Redraw Saat Diam** | `0 frame` (masuk `ControlFlow::Wait` setelah settle dalam `~0.3 – 1.5 dtk`) |
| **Working Set (35 item nyata)** | `~203.5 MB` *(~12–15 MB memori aplikasi Rust; sisanya overhead runtime driver GPU DX12)* |

---

## 🛠️ Build dari Source

<details>
<summary><strong>Prasyarat, perintah Cargo, dan pembuatan MSI</strong></summary>

### Prasyarat
- **Rust** (`x86_64-pc-windows-msvc`)
- **Windows 10 SDK** (`rc.exe` untuk menyematkan ikon & metadata versi)
- **[WiX Toolset v5](https://wixtoolset.org/)** *(opsional, untuk membangun installer `.msi`)*

### Build & Verifikasi

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Binary hasil kompilasi berada di `target/release/bubbledesk.exe`.

### Flag Debug
- `cargo run -- --dump-desktop-tree` — Mencetak pohon jendela `Progman` / `WorkerW` untuk diagnostik integrasi desktop.

### Membangun Installer MSI

```sh
wix build installer/bubbledesk.wxs -d Version=0.1.0 -d ExePath=target/release/bubbledesk.exe -o dist/BubbleDesk-0.1.0-x86_64.msi
```

</details>

---

## 🗺️ Roadmap

- [x] **M0–M1:** Fondasi, single-instance lock, logging, integrasi desktop `WorkerW` & mode fallback
- [x] **M2:** Renderer bubble SDF berbasis `wgpu` (DX12), latar wallpaper, dan overlay debug
- [x] **M3:** Pemindai desktop, atlas ikon, skor berbasis pemakaian, dan persistensi JSON atomik
- [x] **M4:** Mouse hook level rendah, tooltip hover, drag & pin, peluncur worker, dan menu konteks
- [x] **M5:** Ikon tray, autostart, pemulihan ikon bawaan yang aman, auto-pause daya/layar penuh, hot-reload config
- [x] **M7:** Paket rilis `v0.1.0` (installer MSI & arsip portabel)
- [ ] **M5+:** Verifikasi `WorkerW` Layout B untuk Windows 11 24H2+
- [ ] **M6 *(opsional)*:** Impor pemakaian pasif (`UserAssist`/Recent), mode arsip **Tidy** (opt-in dengan undo penuh), dan dukungan multi-monitor

---

## 📄 Lisensi & Hak Cipta

Hak Cipta © 2026 **Hafizh Ammar Muflih**.  
Dirilis di bawah lisensi [**GNU Affero General Public License v3.0 (`AGPL-3.0-only`)**](LICENSE).

<div align="right">
  <sub><a href="#bubbledesk">↑ Kembali ke atas</a></sub>
</div>
