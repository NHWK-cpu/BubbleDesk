# BubbleDesk

**BubbleDesk** adalah aplikasi Windows ringan yang mengubah desktop menjadi wallpaper hidup berisi gelembung (*bubble*). Setiap file, folder, dan shortcut di desktop diwakili satu bubble berisi ikonnya. Semakin sering suatu item dibuka lewat BubbleDesk, semakin besar ukuran bubble-nya; item yang lama tidak dipakai perlahan mengecil.

- **Target OS:** Windows 10 22H2 (`x86_64-pc-windows-msvc`)
- **Bahasa antarmuka:** Indonesia
- **Lisensi:** GNU Affero General Public License v3.0 (`AGPL-3.0-only`)

---

## Fitur Utama

- **Terintegrasi di belakang ikon desktop:** Berjalan sebagai lapisan wallpaper (`WorkerW`), tidak mengganggu jendela aplikasi lain, dan tetap tampil saat `Win+D`.
- **Ukuran adaptif berbasis pemakaian:** Item yang sering dibuka membesar (dengan batas maksimum agar tidak memakan seluruh layar); item yang jarang dipakai perlahan mengecil.
- **Anti-tumpuk & hemat daya:** Gelembung tersusun proporsional mengisi layar, otomatis masuk mode tidur (*sleep*) saat diam sehingga penggunaan CPU nyaris 0%, dan otomatis jeda saat aplikasi/video/game layar penuh, `Win+L`, atau layar mati.
- **Pembaruan real-time:** Menambah, mengubah nama, atau menghapus file di Desktop langsung memperbarui tampilan bubble dalam hitungan detik.
- **Kontrol penuh & aman:**
  - Tidak pernah memindah, mengganti nama otomatis, atau menghapus file pengguna di balik layar.
  - Opsi menyembunyikan ikon desktop bawaan dilengkapi pemulihan otomatis saat keluar normal, saat terjadi *crash*, maupun saat startup berikutnya jika sesi sebelumnya terhenti paksa.
  - Menu tray untuk menampilkan kembali ikon desktop asli kapan saja.

---

## Pemasangan

### 1. Menggunakan Installer MSI (disarankan)
1. Unduh berkas `BubbleDesk-<versi>-x86_64.msi` dari halaman [Releases](https://github.com/NHWK-cpu/BubbleDesk/releases).
2. Jalankan installer (tidak memerlukan hak administrator — dipasang per pengguna di `%LOCALAPPDATA%\Programs\BubbleDesk`).
3. Jalankan **BubbleDesk** dari Start Menu.

> Pencopotan (*uninstall*) lewat **Apps & Features** / **Programs and Features** secara otomatis membersihkan entri autostart `HKCU\...\Run\BubbleDesk` dan memulihkan ikon desktop asli jika sedang tersembunyi.

### 2. Menggunakan Paket Portabel (`.zip`)
1. Unduh `BubbleDesk-<versi>-x86_64-pc-windows-msvc.zip`.
2. Ekstrak ke folder pilihan Anda.
3. Jalankan `bubbledesk.exe`.

---

## Kontrol & Penggunaan

| Interaksi | Aksi |
|---|---|
| **Klik ganda** (default) / Klik tunggal | Membuka file, folder, atau shortcut |
| **Seret (Drag)** | Memindahkan posisi bubble dan menahannya (*pin*) |
| **Ctrl + Klik kiri** | Membuka menu konteks (Buka, Buka lokasi berkas, Ganti nama/label, Lepas sematan/Pin, Sembunyikan) |
| **Arahkan kursor (Hover)** | Menampilkan tooltip nama lengkap dan jumlah dibuka |
| **Ikon Tray (klik kanan)** | Tampilkan/sembunyikan ikon desktop asli, Jeda/Lanjutkan, Overlay debug, Buka folder data, Keluar |

---

## Konfigurasi (`config.toml`)

Semua data dan konfigurasi BubbleDesk disimpan di `%APPDATA%\BubbleDesk\`:
- `config.toml` — pengaturan aplikasi (mendukung *hot-reload* saat aplikasi berjalan).
- `state.json` — riwayat skor, jumlah dibuka, dan posisi pin.
- `bubbledesk.log` — catatan aktivitas & diagnostik (rotasi 1 MB × 3).

Contoh `config.toml`:

```toml
[general]
click_mode = "double"       # "double" atau "single"
start_with_windows = false  # tambah/hapus entri HKCU\...\Run
hide_native_icons = false   # sembunyikan ikon desktop bawaan saat aktif

[render]
fps_cap = 60                # batas FPS saat aktif

[debug]
overlay = false             # tampilkan info FPS & status di sudut layar
```

---

## Build dari Source

Prasyarat:
- Rust (`x86_64-pc-windows-msvc`)
- Windows 10 SDK (`rc.exe` untuk menyematkan ikon & metadata versi)
- [WiX Toolset v5](https://wixtoolset.org/) (opsional, untuk membangun `.msi`)

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Binary release berada di `target/release/bubbledesk.exe`. Untuk membangun MSI:

```sh
wix build installer/bubbledesk.wxs -d Version=0.1.0 -d ExePath=target/release/bubbledesk.exe -o dist/BubbleDesk-0.1.0-x86_64.msi
```

---

## Lisensi & Hak Cipta

Hak Cipta © 2026 **Hafizh Ammar Muflih**.
Dirilis di bawah lisensi [GNU Affero General Public License v3.0](LICENSE).
