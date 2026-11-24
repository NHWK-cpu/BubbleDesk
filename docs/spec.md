# BubbleDesk — Spesifikasi Produk & Teknis

*Pembaca utama: agen AI pembangun. Platform utama: Windows 10 22H2; Windows 11 sekunder. Bahasa implementasi: Rust. Versi dokumen 1.0 · 12 November 2026.*

## 0. Cara membaca dokumen ini

- Dokumen ini adalah satu-satunya sumber kebenaran. Baca seluruhnya sebelum menulis kode.
- Bagian 1–5 menjelaskan apa dan mengapa. Bagian 6–16 menjelaskan bagaimana. Bagian 17–19 berisi urutan kerja, pengujian, dan risiko. Bagian 20 berisi aturan kerja untuk agen. Bagian 21 berisi keputusan yang harus ditanyakan ke manusia.
- Kata **HARUS** berarti wajib, **SEBAIKNYA** berarti direkomendasikan, **BOLEH** berarti opsional.
- Semua angka (ukuran, durasi, ambang) adalah nilai default dan HARUS bisa diubah lewat konfigurasi (§15).
- Nama kerja produk: **BubbleDesk**. Nama crate: `bubbledesk`.

## 1. Ringkasan & visi

BubbleDesk adalah aplikasi Windows yang sangat ringan, mengubah desktop menjadi “wallpaper hidup” berisi gelembung (bubble). Setiap file, folder, dan shortcut aplikasi yang biasanya berserakan di desktop diwakili oleh satu bubble berisi ikonnya.

- Semakin sering sebuah item dibuka lewat BubbleDesk, semakin **besar** bubble-nya, sampai batas ukuran maksimum.
- Item yang lama tidak dipakai perlahan **mengecil** kembali.
- Bubble bergerak lembut, saling dorong, bisa di-hover, di-drag, dan diklik untuk membuka item.

**Masalah yang diselesaikan:** desktop penuh ikon kecil yang bercampur antara shortcut aplikasi dan file yang sering diakses, sehingga sulit menemukan yang penting.

**Tiga prinsip produk:**

1. **Ringan.** RAM kecil, CPU nyaris nol saat tidak ada gerakan.
2. **Tidak merusak.** BubbleDesk tidak memindah, mengganti nama, atau menghapus file pengguna, kecuali fitur “Tidy” yang eksplisit dan dapat di-undo (Fase 5).
3. **Terasa mulus.** Animasi 60 FPS saat bergerak, tanpa patah-patah.

## 2. Tujuan dan non-tujuan

**Tujuan v1**

- T1: pengguna melihat sekilas item mana yang paling sering dipakai.
- T2: membuka item cukup dengan satu klik.
- T3: desktop tampak rapi tanpa pengguna menata manual.
- T4: RAM idle ≤ 40 MB, aktif animasi ≤ 80 MB (100 bubble).
- T5: aman bagi file pengguna, tanpa hak admin, tanpa jaringan.

**Non-tujuan v1**

- Bukan pengganti Start Menu, taskbar, atau Explorer.
- Tidak ada sinkronisasi lintas perangkat, cloud, akun, atau telemetri.
- Tidak mendukung macOS/Linux.
- Satu monitor saja (monitor primer atau yang dipilih di config). Multi-monitor penuh ditunda.
- Tidak ada widget lain (jam, cuaca, dsb.).

## 3. Skenario pengguna

- **US-1** Saya ingin semua item desktop tampil sebagai bubble agar tidak perlu mencari ikon kecil.
- **US-2** Saya ingin membuka item dengan satu klik pada bubble.
- **US-3** Saya ingin bubble item yang sering saya buka menjadi lebih besar.
- **US-4** Saya ingin item yang lama tidak dipakai mengecil sendiri.
- **US-5** Saya ingin menggeser bubble ke posisi favorit dan posisinya diingat.
- **US-6** Saat hover, saya ingin melihat nama, jumlah dibuka, dan kapan terakhir dibuka.
- **US-7** Dengan **Ctrl + klik kiri**, saya ingin: buka lokasi file, sembunyikan bubble, reset hitungan, ganti nama item (sekaligus mengganti nama file/folder aslinya di desktop).
- **US-8** File baru di desktop muncul otomatis; file yang dihapus hilang otomatis.
- **US-9** Aplikasi tidak terasa berat dan tidak mengganggu game/aplikasi layar penuh.
- **US-10** Lewat ikon tray saya bisa jeda, buka pengaturan, atur mulai-otomatis, dan keluar.
- **US-11** Tidak ada file saya yang dipindah atau dihapus tanpa izin saya.
- **US-12** Jika Explorer crash atau restart, bubble kembali sendiri.

## 4. Kebutuhan fungsional

- **FR-1 Pemindaian.** Sumber default: folder Desktop pengguna (`SHGetKnownFolderPath(FOLDERID_Desktop)`, ikut menangani Desktop yang dialihkan OneDrive) dan Public Desktop (`FOLDERID_PublicDesktop`). Abaikan `desktop.ini`, `Thumbs.db`, dan file beratribut hidden/system. Pantau perubahan dengan crate `notify`, debounce 300 ms.
- **FR-2 Jenis item.** File biasa, folder, `.lnk`, `.url`, `.exe`. Satu bubble per entri.
- **FR-3 Peluncuran.** Buka lewat `ShellExecuteExW` (verb default) di thread worker. Jika gagal (target hilang, drive terlepas), bubble “bergetar” dan tooltip menampilkan pesan singkat. Jangan menaikkan hitungan jika gagal.
- **FR-4 Penghitung.** Tiap peluncuran sukses lewat BubbleDesk menaikkan skor (§9) dan `total_launches`.
- **FR-5 Ukuran.** Radius bubble mengikuti skor sesuai §9.
- **FR-6 Fisika dan animasi.** Sesuai §10.
- **FR-7 Interaksi.** Hover, klik, drag, dan menu konteks via Ctrl + klik kiri sesuai §12.
- **FR-8 Persistensi.** State dan posisi disimpan ke JSON secara atomik (§8).
- **FR-9 Latar belakang.** Tampilkan wallpaper aktif pengguna (dengan dim dan blur ringan) atau gradien/warna solid. Jendela kita menutupi wallpaper asli, jadi kita HARUS menggambar latar sendiri.
- **FR-10 Ikon bawaan.** Opsi menyembunyikan ikon desktop bawaan (default aktif), dengan jaminan dipulihkan saat keluar atau setelah crash (§7.4).
- **FR-11 Tray.** Menu: Jeda/Lanjut, Muat ulang, Buka config, Mulai bersama Windows (toggle), Keluar.
- **FR-12 Satu instance.** Instance kedua HARUS keluar dan, bila mungkin, membangunkan instance pertama.
- **FR-13 Hemat daya.** Sesuai §14.
- **FR-14 Fase lanjut.** Impor penggunaan pasif, mode Tidy, warna per kategori (Fase 5).
- **FR-15 Mode kompatibilitas.** Jika attach ke WorkerW gagal, fallback ke jendela borderless di bawah semua jendela (`HWND_BOTTOM`). Catat dalam log bahwa mode ini aktif (pada Windows 10 22H2 jendela tanpa `WS_MINIMIZEBOX` tetap bertahan saat Win+D, namun di versi Windows lain dapat tersembunyi).

## 5. Kebutuhan non-fungsional

- **RAM:** working set idle ≤ 40 MB; saat animasi ≤ 80 MB, diukur dengan 100 bubble. Debug overlay HARUS menampilkan angka ini (`GetProcessMemoryInfo`).
- **CPU:** idle < 0,5% (praktisnya 0 karena tidak ada redraw); animasi < 5% satu core pada laptop kelas menengah.
- **Frame:** 60 FPS stabil, frame time p95 < 16,6 ms.
- **Startup:** bubble pertama tampil < 1,5 detik setelah proses mulai; ikon dimuat bertahap di latar.
- **Skala:** sampai 300 item tanpa turun dari 60 FPS.
- **Ukuran binary release:** < 15 MB.
- **Keandalan:** tidak crash saat file dihapus, drive dicabut, Explorer restart, resolusi/DPI berubah, sleep/resume, lock/unlock.
- **Privasi dan keamanan:** semua data lokal, tanpa akses jaringan, tanpa telemetri, tanpa hak admin.
- **DPI:** per-monitor DPI aware v2 (lewat manifest), layout benar pada 100–300%.
- **Aksesibilitas:** opsi `reduce_motion` mematikan drift; kontras label memadai.
- **OS:** Windows 10 22H2 sebagai target utama (dikembangkan dan diuji di sini). Windows 11 23H2 dan 24H2 atau lebih baru sebagai target sekunder, dikerjakan setelah M5.

## 6. Arsitektur

Satu proses, beberapa thread:

1. **Main/UI thread:** event loop `winit`, state aplikasi, simulasi fisika, render.
2. **Input thread:** memasang `WH_MOUSE_LL` hook dan menjalankan message loop sendiri; mengirim event ke main thread lewat `EventLoopProxy`. Hook proc HARUS mengembalikan kontrol secepat mungkin dan selalu memanggil `CallNextHookEx` (jangan pernah menelan input pengguna).
3. **Worker (1–2 thread):** ekstraksi ikon, peluncuran (`ShellExecuteExW` bisa memblokir), tulis state ke disk, watcher folder.

**Alur data:** `Scanner` → `ItemStore` (skor) → `World` (badan fisika) → `Renderer`. Alur klik: `Input` → `App` (hit test) → `Launcher` → `ItemStore.record_launch` → persist (debounced).

**State aplikasi:** `Starting → Attaching → Running ⇄ Paused(reason) → Exiting`. Alasan jeda: `UserPaused`, `FullscreenApp`, `SessionLocked`, `DisplayOff`. Jika `Attaching` gagal: masuk `Running` dengan mode kompatibilitas (FR-15).

**Modul utama:** `platform::workerw` (attach desktop), `platform::input` (mouse hook), `platform::icons`, `platform::launch`, `platform::power`, `platform::wallpaper`, `platform::singleton`, `platform::autostart`, `scanner`, `scoring`, `store`, `physics`, `render`, `config`, `ui::tray`, `app`.

## 7. Integrasi desktop Windows (bagian paling kritis)

### 7.1 Konsep

Windows menggambar desktop lewat jendela `Progman`. Jika kita mengirim pesan khusus `0x052C` ke Progman, Explorer membuat jendela `WorkerW` yang berada di belakang lapisan ikon (`SHELLDLL_DefView`). Jika jendela kita dijadikan child dari WorkerW tersebut, jendela kita tampil seperti wallpaper dan berada di bawah ikon. Teknik ini **tidak terdokumentasi resmi** dan dipakai luas oleh aplikasi wallpaper animasi. Karena itu implementasi HARUS defensif: deteksi saat runtime, logging jelas, dan fallback.

### 7.2 Algoritma attach

1. `progman = FindWindowW("Progman", null)`.
2. `SendMessageTimeoutW(progman, 0x052C, 0xD, 0x1, SMTO_NORMAL, 1000, ...)`. Jika langkah 3–4 gagal, ulangi sekali dengan `(0xD, 0)`.
3. **Layout A (klasik, Windows 10 dan Windows 11 sebelum 24H2; target utama):** `EnumWindows`; untuk tiap top-level `hwnd`, cari child `SHELLDLL_DefView`. Jika ada, `workerw = FindWindowExW(null, hwnd, "WorkerW", null)` (WorkerW saudara setelahnya).
4. **Layout B (sekunder, dikerjakan setelah M5; tidak diperlukan di Windows 10 22H2; dilaporkan pada Windows 11 24H2 dan lebih baru):** `WorkerW` dan `SHELLDLL_DefView` menjadi child dari `Progman`. Cari `workerw = FindWindowExW(progman, null, "WorkerW", null)`. Struktur persisnya HARUS diverifikasi pada mesin nyata. Pilih strategi lewat deteksi runtime (coba A, lalu B), bukan lewat nomor versi OS.
5. Buat jendela `winit`: tanpa dekorasi, tidak muncul di taskbar/Alt-Tab (`WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`). Ambil `HWND` lewat `raw-window-handle`, lalu `SetParent(hwnd, workerw)`, ubah style dari `WS_POPUP` menjadi `WS_CHILD | WS_VISIBLE`.
6. `SetWindowPos` ke rect monitor target, relatif terhadap parent (origin parent = pojok kiri-atas virtual screen).
7. Verifikasi: `GetParent(hwnd) == workerw` dan `IsWindowVisible`. Jika gagal, fallback ke FR-15.

Sediakan perintah debug `bubbledesk --dump-desktop-tree` yang mencetak pohon jendela desktop (kelas, handle, parent, visibilitas, rect) agar struktur tiap versi Windows bisa dianalisis dan dicatat di `docs/desktop-tree-notes.md`.

### 7.3 Ketahanan

- **Explorer restart:** daftarkan `RegisterWindowMessageW("TaskbarCreated")`. Saat diterima, jalankan ulang §7.2 sepenuhnya.
- **Perubahan display/DPI:** tangani `WM_DISPLAYCHANGE`, `WM_DPICHANGED`, dan `ScaleFactorChanged` dari winit: ubah ukuran swapchain dan hitung ulang layout.
- **Win+D:** tidak berpengaruh karena kita child WorkerW (pada mode kompatibilitas `HWND_BOTTOM` tanpa `WS_MINIMIZEBOX` di Windows 10 22H2 juga terbukti tetap bertahan).
- **Lock/sleep:** pindah ke `Paused(SessionLocked)` dan hentikan render. Lanjutkan saat unlock/resume.
- **Wallpaper berubah:** tangani `WM_SETTINGCHANGE` dengan `SPI_SETDESKWALLPAPER`, muat ulang latar.

### 7.4 Ikon desktop bawaan

Ikon asli (`SysListView32` di dalam `SHELLDLL_DefView`) berada di atas jendela kita. Opsi `hide_native_icons` menyembunyikannya dengan mengirim `WM_COMMAND` dengan id `0x7402` ke `SHELLDLL_DefView` (setara menu “Tampilkan ikon desktop”). Ini **toggle**, bukan set, sehingga HARUS memeriksa status dulu lewat `IsWindowVisible` pada `SysListView32`.

Jaminan pemulihan (WAJIB):

- Simpan flag `icons_hidden_by_us = true` di state saat menyembunyikan.
- Pulihkan saat keluar normal, di panic hook, dan di startup berikutnya jika flag masih true namun proses sebelumnya tidak keluar bersih.
- Jangan pernah membiarkan pengguna kehilangan ikon desktop tanpa jalan kembali. Menu tray HARUS punya “Tampilkan ikon desktop asli”.

### 7.5 Penerimaan input

Karena jendela kita berada di bawah lapisan ikon, jendela ini tidak menerima pesan mouse biasa. Solusi: `WH_MOUSE_LL` hook di input thread (§6, §12). Setiap event diteruskan ke main thread hanya jika kursor berada di atas desktop kosong, yaitu `WindowFromPoint(pt)` mengembalikan jendela berkelas `Progman`, `WorkerW`, `SHELLDLL_DefView`, atau `SysListView32`. Jika ada jendela aplikasi di atasnya, event diabaikan.

## 8. Model data dan penyimpanan

**Lokasi:** `%APPDATA%\BubbleDesk\` berisi `config.toml`, `state.json`, `bubbledesk.log` (rotasi, maksimum 1 MB × 3).

**Item:**

```
Item {
  id: String,            // hash stabil dari path sumber (lowercase, full path)
  path: PathBuf,         // path entri di desktop (untuk .lnk: path .lnk itu sendiri)
  label: Option<String>, // label kustom; default nama tampilan dari shell
  kind: App | Folder | Document | Media | Other,
  score: f64,            // skor terakhir yang disimpan (lihat §9)
  score_t0: i64,         // unix detik saat skor dihitung
  total_launches: u64,
  last_launch: Option<i64>,
  first_seen: i64,
  pinned_pos: Option<[f32;2]>, // posisi ternormalisasi 0..1 terhadap area kerja
  hidden: bool,
}
```

**`state.json`:** `{ "schema": 1, "items": [...], "icons_hidden_by_us": false }`.

**Aturan tulis:**

- Tulis atomik: tulis ke `state.json.tmp`, `flush`, lalu `rename` menimpa `state.json`.
- Debounce 2 detik setelah perubahan; paksa flush saat keluar.
- Jika file rusak atau gagal di-parse: pindahkan ke `state.corrupt-<timestamp>.json`, mulai kosong, catat ke log. Jangan crash.
- Gunakan field `schema` dan fungsi migrasi berjenjang untuk versi mendatang.
- Item yang file-nya hilang dari desktop dipertahankan 30 hari dengan flag `missing` (jika file muncul lagi, skor kembali), lalu dibuang.

## 9. Skor dan ukuran bubble

Setiap item punya skor `S` dan waktu referensi `t0`. Half-life `H` (default 30 hari).

```
decayed(now) = S * 0.5 ^ ((now - t0) / H)
saat peluncuran sukses:  S = decayed(now) + 1.0 ;  t0 = now
x = clamp( ln(1 + decayed(now)) / ln(1 + S_cap), 0, 1 )     // S_cap default 40
r_target = r_min + (r_max - r_min) * x                      // r_min 28 px, r_max 110 px @100% DPI
```

Decay dihitung malas (lazy) saat dibutuhkan, tanpa timer. Dengan skala logaritmik, satu aplikasi favorit tidak menelan bubble lain.

**Penghalusan:** `r_display += (r_target - r_display) * (1 - exp(-dt / τ))`, τ = 0,35 detik. Saat peluncuran sukses, mainkan animasi “pop” (skala 1,0 → 1,15 → 1,0 dalam 250 ms dengan spring) dan cincin highlight singkat.

**Anti-sesak:** jika total luas semua lingkaran melebihi 55% area kerja, kalikan semua radius dengan `k = sqrt(0.55 * area_kerja / total_luas)`, dengan `k ≥ 0.5`.

**Batas klik minimum:** radius efektif ≥ 20 px.

**Label:** tampil saat hover, atau permanen jika `r ≥ 60 px` (dapat diubah di config).

## 10. Fisika dan animasi

**Badan (body):** `pos`, `vel`, `radius`, `mass = radius²`, flag `pinned`, `dragging`.

**Integrasi:** semi-implicit Euler, langkah tetap `dt = 1/120 s` dengan akumulator, maksimum 4 substep per frame.

**Gaya dan aturan:**

1. **Home:** jika item punya `pinned_pos`, tarik dengan spring ke posisi itu (k = 2,0). Jika tidak, tarik sangat lemah ke pusat area (k = 0,15).
2. **Drift:** gaya noise halus (amplitudo kecil, \~0,5 px/s²) supaya terasa hidup. Nonaktif saat `reduce_motion` atau saat tidur.
3. **Tumbukan lingkaran–lingkaran:** broad phase dengan grid spasial (sel = 2 × `r_max`). Jika `jarak < ri + rj + gap` (gap 6 px), koreksi posisi sebanding massa terbalik (3 iterasi) lalu impuls dengan restitusi 0,2.
4. **Dinding:** batas = work area monitor (`rcWork`, sehingga tidak menimpa taskbar) dikurangi margin 8 px, restitusi 0,3.
5. **Damping:** `vel *= exp(-3.0 * dt)` agar terasa seperti bergerak di air.
6. **Drag:** body mengikuti kursor dengan spring kuat. Saat dilepas, set `pinned_pos = pos`, kecepatan lempar dibatasi.
7. **Hover:** skala visual target 1,08 dengan spring (stiffness 180, damping 22). Hover hanya mengubah tampilan, bukan radius fisika, supaya tidak bergetar.
8. **Penempatan awal:** item terbesar dulu, tata dengan spiral dari pusat (atau Poisson-disk) memakai RNG ber-seed tetap agar dapat diuji.

**Tidur (sleep):** jika `max |vel| < 2 px/s` dan tidak ada perubahan radius target selama 500 ms, simulasi dan render berhenti total (§14). Bangun oleh: hover, drag, perubahan radius, item ditambah/dihapus, resize, wallpaper berubah.

## 11. Rendering

- **Backend:** `wgpu` (DX12 sebagai utama). Bungkus dengan trait tipis `Renderer` (`init`, `resize`, `set_background`, `draw(frame)`, `shutdown`) supaya backend CPU (`tiny-skia` + `softbuffer`) bisa menggantikan jika target RAM tidak tercapai (lihat risiko R4).
- **Bubble:** satu draw call instanced berupa quad + fragment shader WGSL berbasis SDF lingkaran. Data per instance: pusat, radius, skala visual, warna cincin, lebar cincin, glow, rect UV ikon, flag (hover, pop, broken). Tepi anti-alias lewat `smoothstep` pada SDF, tanpa MSAA dan tanpa depth buffer.
- **Tampilan bubble:** isi semi-transparan ala kaca (gradasi halus, highlight kiri-atas), cincin tipis berwarna per jenis, ikon di tengah dengan diameter ±60% dari bubble. Warna cincin default: App `#5B8DEF`, Folder `#F2B84B`, Document `#6FCF97`, Media `#BB6BD9`, Other `#9AA5B1`.
- **Atlas ikon:** slot 96×96 px, satu halaman 1536×1536 (256 slot, ±9 MB), maksimum 2 halaman, dialokasikan malas. Ikon yang belum siap menampilkan placeholder (huruf awal nama di dalam lingkaran).
- **Teks:** hanya untuk label saat hover/bubble besar dan tooltip. SEBAIKNYA `fontdue` atau `ab_glyph` dengan atlas glyph R8 512×512 sendiri, memakai font sistem (Segoe UI dari `C:\Windows\Fonts`, tanpa membundel font). Hindari stack teks yang berat (database font besar) demi RAM.
- **Latar:** baca path wallpaper dari `SystemParametersInfo(SPI_GETDESKWALLPAPER)`, decode dengan crate `image` (fitur png/jpeg/bmp saja), skala ke resolusi monitor, terapkan blur sekali di CPU (untuk `background_blur > 0`), unggah sebagai satu tekstur, lalu **bebaskan salinan CPU**. Dim dikerjakan di shader. Jika path kosong (Spotlight/slideshow) atau gagal dibaca, pakai gradien default.
- **Present mode:** `Fifo` (vsync), alpha opaque (kita menggambar latar sendiri).
- **Pacing:** render hanya saat `needs_redraw`. Tangani `SurfaceError::Lost/Outdated` dengan reconfigure, dan device lost dengan membuat ulang device, antrean, dan resource.
- **Alokasi:** tidak boleh ada alokasi heap per frame di jalur panas (buffer instance dialokasikan sekali dengan kapasitas tumbuh berjenjang).

## 12. Input dan interaksi

- **Sumber input:** `WH_MOUSE_LL` di input thread. Event: `Move`, `LeftDown/Up`, `RightDown/Up` (wheel diabaikan). Koordinat dalam piksel fisik (butuh DPI-awareness per-monitor v2 lewat manifest).
- **Aktivasi:** event diproses hanya jika kursor berada di atas desktop kosong (§7.5), berada di monitor target, dan status bukan `Paused`.
- **Hover:** bubble yang mengandung titik kursor; jika bertumpuk, pilih yang `jarak/radius` terkecil. Tooltip muncul setelah 400 ms: nama, “Dibuka N× · terakhir \<relatif>”, dan path.
- **Klik:** tekan dan lepas di bubble yang sama dengan pergeseran < 4 px. `click_mode = single` (default) membuka pada klik tunggal; `double` memakai `GetDoubleClickTime()`.
- **Drag:** tekan di bubble lalu bergeser ≥ 4 px memulai drag. Saat dilepas, simpan `pinned_pos`.
- **Ctrl + klik kiri:** menu bawaan Win32 (`TrackPopupMenu`, lebih hemat RAM daripada library menu). Item: Buka, Buka lokasi file (`explorer.exe /select,"<path>"`), Salin path, Ganti nama…, Reset hitungan, Sembunyikan, Lepas pin (kembali ke posisi otomatis). Dipicu dengan Ctrl+klik kiri (bukan klik kanan) agar tidak bentrok dengan menu desktop bawaan Windows. Karena jendela kita tidak bisa menjadi foreground, buat **jendela helper tersembunyi** top-level sebagai owner, panggil `SetForegroundWindow(helper)` sebelum `TrackPopupMenu`, dan kirim `WM_NULL` sesudahnya agar menu menutup dengan benar saat klik di luar.
- **Ganti nama:** item menu "Ganti nama…" mengganti nama **file/folder asli** di desktop (via `std::fs::rename` pada direktori induk yang sama), bukan sekadar label tampilan. Ekstensi asli dipertahankan bila pengguna tidak menuliskannya (mis. `laporan` → `laporan.pdf`). Nama yang mengandung karakter terlarang (`< > : " / \ | ? *`) atau sudah dipakai ditolak dengan getaran + tooltip error.
- **Tidak menelan input:** hook selalu memanggil `CallNextHookEx`. Klik di bubble tetap diteruskan ke desktop di bawahnya (tidak berbahaya, terlebih jika ikon bawaan disembunyikan).
- **Keyboard:** tidak ada (jendela tidak fokus). BOLEH menambah hotkey global opsional (`RegisterHotKey`) untuk Jeda/Lanjut.

## 13. Ikon, peluncuran, dan pemindaian

- **Ikon:** `IShellItemImageFactory::GetImage` (ukuran 96×96) untuk mendapat `HBITMAP`, konversi ke RGBA dengan penanganan alpha yang konsisten dengan shader (premultiplied vs straight harus satu keputusan di seluruh pipeline). Fallback: `SHGetFileInfoW` + `GetIconInfo` + `GetDIBits`. Ekstraksi di worker thread. Cache disk opsional di `%LOCALAPPDATA%\BubbleDesk\icons\<hash>.png` dengan kunci (path, mtime) untuk startup lebih cepat. Periksa apakah overlay panah shortcut ikut terambil dan catat hasilnya di dokumen perf/visual.
- **Nama tampilan:** `SHGetFileInfoW(SHGFI_DISPLAYNAME)` agar mengikuti pengaturan Windows (misalnya ekstensi disembunyikan).
- **Klasifikasi `kind`:** folder → Folder; `.lnk/.url/.exe/.bat/.cmd` → App; ekstensi dokumen (`docx, xlsx, pptx, pdf, txt, md, csv, odt`) → Document; media (`png, jpg, gif, mp3, mp4, mkv, wav`) → Media; sisanya Other. Daftar ini HARUS ada di satu tempat dan mudah diubah.
- **Peluncuran:** `CoInitializeEx` (apartment-threaded) di worker, lalu `ShellExecuteExW` dengan `lpVerb = null` (aksi default), `nShow = SW_SHOWNORMAL`, `SEE_MASK_FLAG_NO_UI`. Hasil (sukses/kode error) dikirim ke main thread. Tampilkan pesan ramah di tooltip untuk error umum (`ERROR_FILE_NOT_FOUND`, `ERROR_PATH_NOT_FOUND`, akses ditolak).
- **Pemindaian awal:** enumerasi non-rekursif pada sumber di FR-1. Hitung `id` dari path ter-normalisasi (lowercase, `\\?\` dibuang).
- **Watcher:** `notify` (+ debouncer 300 ms). Rekonsiliasi berdasarkan `id`: tambah, hapus, ubah. Untuk event rename dalam folder yang sama, pindahkan skor ke `id` baru (pasangan `RenameMode::Both`).
- **Duplikat:** entri dengan nama sama di Desktop pengguna dan Public Desktop ditampilkan keduanya secara default; BOLEH menambah opsi `dedupe_by_name`.

## 14. Hemat daya dan RAM

- **Event-driven:** event loop memakai `ControlFlow::Wait`. Saat animasi aktif, `WaitUntil(next_frame)` atau `request_redraw()` berantai. Saat fisika tidur, kembali ke `Wait` (CPU ≈ 0).
- **Pemicu jeda (`Paused`):**
  - Aplikasi layar penuh atau mode presentasi: `SHQueryUserNotificationState` (`QUNS_BUSY`, `QUNS_RUNNING_D3D_FULL_SCREEN`, `QUNS_PRESENTATION_MODE`), dipoll tiap 2 detik.
  - Lock/unlock sesi: `WTSRegisterSessionNotification` (`WM_WTSSESSION_CHANGE`).
  - Layar mati: `RegisterPowerSettingNotification` dengan `GUID_CONSOLE_DISPLAY_STATE`.
  - Desktop tertutup total: jendela foreground maksimum di monitor target (`GetForegroundWindow` + `IsZoomed`, poll 1 detik).
- **Saat `Paused`:** hentikan render, hentikan simulasi, dan **lepas mouse hook** (`UnhookWindowsHookEx`); pasang kembali saat lanjut. Ini juga mengurangi risiko konflik dengan game/anti-cheat.
- **Baterai:** jika `GetSystemPowerStatus` menunjukkan baterai dan hemat daya aktif, batasi ke `fps_cap_on_battery` (default 30).
- **Disiplin RAM:** tekstur latar satu salinan saja; atlas ikon ≤ 9 MB per halaman; tanpa alokasi per frame; label memakai `Arc<str>`; nonaktifkan fitur default pada crate; jangan pakai runtime async berat (tanpa `tokio`). JANGAN memakai `EmptyWorkingSet` (hanya menyamarkan angka dan memicu page fault).
- **Pengukuran:** debug overlay (aktif dari tray atau `debug.overlay = true`) menampilkan FPS, frame time (ms), working set dan private bytes (`GetProcessMemoryInfo`), jumlah body, dan status awake/asleep. Hasil ukur dicatat di `docs/perf.md` pada setiap milestone.

## 15. Konfigurasi (`config.toml`)

Semua key opsional; nilai di bawah adalah default. File di-reload otomatis saat berubah (kecuali `monitor` dan `start_with_windows` yang diterapkan saat restart). Nilai tidak valid HARUS jatuh ke default dengan peringatan di log, bukan crash.

```toml
[general]
start_with_windows = false
monitor = "primary"        # "primary" atau indeks 0..n
click_mode = "single"      # "single" | "double"
hide_native_icons = true
reduce_motion = false

[sources]
include = ["desktop", "public_desktop"]   # boleh ditambah path folder lain
ignore_patterns = ["desktop.ini", "Thumbs.db", "~$*"]
ignore_hidden = true

[bubbles]
r_min = 28.0
r_max = 110.0
score_half_life_days = 30
score_cap = 40
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
background = "wallpaper"   # "wallpaper" | "gradient" | "solid"
background_dim = 0.35
background_blur = 6        # 0 = nonaktif
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

## 16. Struktur proyek dan dependensi

```
bubbledesk/
  Cargo.toml
  build.rs                 # embed manifest (DPI v2, longPathAware, asInvoker)
  AGENTS.md                # salin dari §20
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

**Dependensi yang disarankan** (pilih versi stabil terbaru saat implementasi; setiap penambahan di luar daftar ini HARUS dibenarkan dari sisi RAM/ukuran binary):

- `windows` (windows-rs) dengan fitur minimum: `Win32_Foundation`, `Win32_UI_WindowsAndMessaging`, `Win32_UI_Shell`, `Win32_UI_HiDpi`, `Win32_UI_Input_KeyboardAndMouse`, `Win32_Graphics_Gdi`, `Win32_System_Com`, `Win32_System_Threading`, `Win32_System_Power`, `Win32_System_RemoteDesktop`, `Win32_System_ProcessStatus`, `Win32_System_Registry`, `Win32_Storage_FileSystem`.
- `winit`, `raw-window-handle`, `wgpu`, `pollster`, `bytemuck`, `glam`.
- `notify` (+ debouncer), `serde`, `serde_json`, `toml`, `image` (`default-features = false`, fitur png/jpeg/bmp), `fontdue`.
- Logging ringan (`tracing` + `tracing-appender`, atau logger berkas buatan sendiri), `thiserror`/`anyhow`.
- Ikon tray: panggil `Shell_NotifyIconW` langsung lewat `windows` agar tidak menambah dependensi.
- Dev: `proptest`, `tempfile`.

**Profil release:**

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"   # panic hook tetap berjalan sebelum abort; hook HARUS memulihkan ikon desktop
strip = true
```

## 17. Roadmap milestone

Kerjakan berurutan. Jangan mulai milestone berikutnya sebelum kriteria penerimaan (AC) terpenuhi dan terdokumentasi.

- **M0 — Fondasi.** Proyek Rust, CI (GitHub Actions `windows-latest`: `cargo fmt --check`, `clippy -D warnings`, `test`, build release), logging, loader config dengan default, single-instance (named mutex), manifest. **AC:** instance kedua keluar dengan bersih; log tertulis; semua tes lulus.
- **M1 — Attach ke desktop.** `workerw.rs`, jendela child warna solid, `--dump-desktop-tree`, re-attach saat `TaskbarCreated`, fallback FR-15. **AC:** warna solid tampil di belakang ikon asli pada Windows 10 22H2 (untuk Windows 11, Layout B cukup berupa stub yang mencatat peringatan dan masuk mode kompatibilitas sampai setelah M5); bertahan setelah `taskkill /f /im explorer.exe` lalu `explorer.exe`; benar pada DPI 100/150/200%; Win+D tidak menyembunyikannya; catatan struktur tiap versi di `docs/desktop-tree-notes.md`.
- **M2 — Rendering dasar.** Inisialisasi wgpu, 20 bubble dummy dengan SDF, latar wallpaper, debug overlay. **AC:** 60 FPS; angka RAM idle/aktif dicatat di `docs/perf.md`; keputusan lanjut wgpu atau backend CPU diambil di sini (§21).
- **M3 — Data nyata.** Scanner, ekstraksi ikon, store, skor, ukuran dari skor, penempatan awal. **AC:** semua item desktop tampil dengan ikon dan label benar; menambah/menghapus file memperbarui tampilan < 1 detik; state bertahan setelah restart.
- **M4 — Interaksi.** Mouse hook, hover + tooltip, klik/peluncuran, drag + pin, menu konteks (Ctrl + klik kiri), animasi pop, fisika dengan sleep. **AC:** US-2, 3, 5, 6, 7 lulus uji manual; CPU idle ≈ 0% saat tidur; tidak ada input pengguna yang tertelan.
- **M5 — Integrasi sistem.** Tray, mulai bersama Windows (registry `HKCU\...\Run`), sembunyikan ikon bawaan dengan jaminan pemulihan, pemicu jeda daya, hot-reload config. **AC:** mematikan proses paksa saat ikon tersembunyi lalu menjalankan ulang memulihkan ikon; video/game layar penuh menghentikan render dan melepas hook. Setelah AC ini terpenuhi, kerjakan dukungan Windows 11 (Layout B) hanya jika ada mesin uji Windows 11, dan catat hasilnya di docs/desktop-tree-notes.md.
- **M6 — Fitur lanjut (terpisah, opsional).** (a) Impor penggunaan pasif: kunci registry UserAssist (nama value ber-ROT13, berisi hitungan eksekusi; format biner tidak terdokumentasi resmi sehingga HARUS diverifikasi) dan timestamp berkas di folder Recent. (b) Mode Tidy: memindahkan item yang lama tak dipakai ke subfolder arsip, hanya setelah dialog konfirmasi dengan pratinjau daftar, dengan log `tidy-log.json` yang memungkinkan undo penuh. (c) Multi-monitor. **AC:** tiap fitur di balik flag config, nonaktif secara default.
- **M7 — Rilis.** Installer MSI (mis. `cargo-wix`) dan zip portabel, README, ikon aplikasi, penomoran versi, code signing (opsional). **AC:** pasang dan copot bersih (tidak meninggalkan entri `Run` atau ikon tersembunyi).

## 18. Strategi pengujian

- **Unit test:** skor (decay, clamp, monotonik terhadap jumlah buka), anti-sesak, penempatan awal (deterministik karena seed), fisika (tidak overlap setelah settle, tetap dalam batas, sleep tercapai), store (tulis atomik, pemulihan file rusak, migrasi skema), config (default, nilai tidak valid), hashing path, rekonsiliasi scanner.
- **Property test (`proptest`):** fisika tidak menghasilkan `NaN`/`inf`; semua body tetap di dalam batas untuk input acak (jumlah body, radius, posisi awal).
- **Uji manual terstruktur** (`docs/manual-test.md`): matriks OS (Windows 10 22H2 wajib; Windows 11 23H2 dan 24H2+ opsional, setelah M5) × DPI (100/125/150/200%); Explorer restart; lock/unlock; sleep/resume; Remote Desktop; video/game layar penuh; hapus/tambah/ganti nama file; drive eksternal dicabut; Desktop yang dialihkan ke OneDrive; nama file Unicode dan path panjang; stress 300 item.
- **Uji performa:** skrip PowerShell yang mencatat working set tiap 5 detik selama 10 menit pada kondisi idle dan animasi; hasil masuk `docs/perf.md`. Bandingkan dengan anggaran §5.
- **Uji keselamatan:** (1) proses dimatikan paksa saat ikon tersembunyi → ikon pulih di startup berikutnya; (2) `state.json` dirusak manual → aplikasi tetap jalan; (3) tidak ada file pengguna berubah selama sesi normal (bandingkan snapshot daftar berkas sebelum/sesudah).

## 19. Risiko dan mitigasi

- **R1. Teknik WorkerW tidak terdokumentasi dan bisa berubah antar build Windows.** Deteksi runtime (layout A lalu B), `--dump-desktop-tree`, fallback FR-15, uji manual setiap pembaruan besar Windows. Di Windows 10 22H2 (target utama) risiko ini rendah karena Layout A sudah mapan; Layout B baru dikerjakan setelah M5.
- **R2. Windows mencopot hook mouse LL yang lambat.** Hook proc minimal (hanya kirim event ke channel), thread khusus, watchdog yang memasang ulang hook jika terdeteksi hilang.
- **R3. Antivirus memberi false positive** (hook global + `SetParent` ke jendela shell). Tanpa hak admin, jelaskan di README, code signing bila memungkinkan.
- **R4. RAM wgpu melebihi target.** Ukur di M2. Jika idle > 60 MB, evaluasi backend CPU (`tiny-skia` + dirty rect) di balik trait `Renderer`.
- **R5. Ikon desktop tersembunyi permanen.** Jaminan pemulihan §7.4 (flag, panic hook, cek startup, menu tray manual).
- **R6. Bentrok dengan aplikasi wallpaper lain** (Wallpaper Engine, Lively). Jika WorkerW sudah memiliki child asing, catat peringatan di log dan JANGAN menimpa; tampilkan notifikasi tray dan tawarkan mode kompatibilitas.
- **R7. Game/anti-cheat keberatan dengan hook global.** Lepas hook saat `Paused(FullscreenApp)` (§14).
- **R8. Multi-monitor dan campuran DPI.** v1 hanya satu monitor; item multi-monitor masuk M6.
- **R9. Wallpaper Spotlight/slideshow tidak punya path berkas.** Fallback gradien, tanpa error.
- **R10. Kehilangan data skor.** Tulis atomik, salinan harian `state.json.bak`.

## 20. Aturan kerja untuk agen (isi `AGENTS.md`)

1. Baca dokumen ini sampai habis sebelum menulis kode. Kerjakan per milestone (§17) dan berhenti untuk melapor di tiap akhir milestone dengan ringkasan: apa yang selesai, hasil AC, angka performa, masalah terbuka.
2. **Jangan pernah** memindah, mengganti nama, atau menghapus berkas pengguna di luar `%APPDATA%\BubbleDesk` dan `%LOCALAPPDATA%\BubbleDesk`, kecuali fitur Tidy (M6) setelah konfirmasi eksplisit pengguna.
3. Semua kode `unsafe` dan pemanggilan Win32 HARUS terisolasi di `src/platform/`, dibungkus API aman, dengan komentar `// SAFETY:` yang menjelaskan invarian.
4. Dilarang `unwrap()`/`expect()` di kode non-test. Gunakan `Result` dan catat ke log. Thread UI tidak boleh panic karena kegagalan I/O atau Win32.
5. Sebelum setiap commit: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test` harus bersih. Commit kecil dengan pesan jelas.
6. Dilarang menambah dependensi di luar daftar §16 tanpa justifikasi tertulis (dampak RAM, ukuran binary, jumlah dependensi transitif). Nonaktifkan fitur default bila tidak dipakai.
7. Jangan mengoptimalkan prematur, tetapi ukur: setelah M2 dan setiap milestone sesudahnya, jalankan pengukuran §14 dan catat di `docs/perf.md`.
8. Jika perilaku Windows berbeda dari asumsi dokumen ini (terutama §7), jangan menebak: catat temuan di `docs/desktop-tree-notes.md`, sesuaikan implementasi, dan laporkan.
9. Setiap nilai ajaib (angka ukuran, durasi, ambang) HARUS berasal dari `config.rs` atau konstanta bernama, bukan tersebar di kode.
10. Pada keputusan di §21, berhenti dan tanyakan manusia; jangan memilih sendiri.
11. Target build: `x86_64-pc-windows-msvc`. Kode khusus Windows memakai `#[cfg(windows)]`. Pengembangan dan uji manual dilakukan di Windows nyata (bukan VM tanpa GPU) bila memungkinkan. Target OS utama adalah Windows 10 22H2; jangan mengerjakan Layout B (Windows 11) sebelum M5 selesai.
12. Perbarui dokumen di `docs/` setiap kali perilaku berubah. Dokumen yang usang dianggap bug.

## 21. Keputusan terbuka (tanyakan ke manusia)

1. **Klik tunggal atau ganda** sebagai default pembuka item (usulan: tunggal).
2. **Nama final produk dan ikon aplikasi** (nama kerja: BubbleDesk).
3. **Backend render:** wgpu atau CPU, diputuskan setelah pengukuran M2.
4. **Lisensi:** GNU Affero General Public License v3.0 (`AGPL-3.0-only`) — diputuskan oleh pemilik proyek.
5. **Bahasa antarmuka:** Indonesia atau Inggris (usulan: semua string di satu berkas agar mudah dilokalisasi).
6. **Apakah mode Tidy** (memindahkan berkas) memang diinginkan, atau cukup tampilan saja.
7. **Apakah impor penggunaan pasif** (UserAssist/Recent) diinginkan untuk mengisi skor awal.
8. **Dukungan multi-monitor** pada v1.x atau ditunda.

## Lampiran A — Pseudocode loop utama

```
loop {
  event = wait_event()                    // winit, ControlFlow::Wait
  match event {
    Input(e)      => app.handle_input(e)  // hit test, hover, drag, klik; bisa membangunkan world
    Frame(now)    => {
      dt = now - last_frame
      world.step(dt)                      // substep tetap 1/120 s, maks 4
      renderer.draw(world.snapshot())
      if world.is_asleep() { set_control_flow(Wait) } else { request_redraw() }
    }
    LaunchResult(id, ok) => store.record_launch_if(ok, id); world.pop(id)
    ScanChanged(diff)    => store.apply(diff); world.sync(store)
    SystemEvent(s)       => app.set_pause_state(s) // fullscreen, lock, display off, TaskbarCreated
    Exit                 => store.flush(); restore_icons_if_hidden(); break
  }
}
```

## Lampiran B — Definition of Done per milestone

- Semua AC milestone terpenuhi dan dicatat.
- `cargo fmt`, `clippy -D warnings`, `cargo test` bersih; CI hijau.
- Daftar uji manual yang relevan dijalankan dan hasilnya dicatat di `docs/manual-test.md`.
- Angka performa terbaru ada di `docs/perf.md` dan masih dalam anggaran §5 (atau ada catatan alasan dan rencana perbaikan).
- Dokumen yang terdampak diperbarui; tidak ada TODO tanpa nomor isu atau penjelasan.
