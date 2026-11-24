# Uji manual BubbleDesk

Uji yang butuh melihat layar atau mengamati proses dijalankan manusia dan
hasilnya dicatat di sini. Isi kolom Hasil setelah menjalankan langkah.

## M0 — Fondasi

### MT-0.1 Instance kedua keluar bersih (AC)

Tercakup otomatis oleh `cargo test` (`tests/single_instance.rs`); jalankan
manual bila ingin memastikan perilakunya pada lingkungan nyata.

1. Buat folder sementara, mis. `set BUBBLEDESK_DATA_DIR=%TEMP%\bd-test`.
2. Jalankan instance pertama: `set BUBBLEDESK_HOLD_MS=5000` lalu `bubbledesk.exe`.
3. Selagi instance pertama berjalan, jalankan `bubbledesk.exe` lagi.
4. Harapan: instance kedua mencetak `BubbleDesk sudah berjalan.` dan keluar
   dengan kode 0; instance pertama tetap berjalan.
5. Hasil (2026-11-12): **lulus** — terverifikasi oleh integration test
   `tests/single_instance.rs` (named mutex mencegah instance kedua).

### MT-0.2 Log tertulis (AC)

1. Jalankan `bubbledesk.exe` sekali.
2. Buka `%APPDATA%\BubbleDesk\bubbledesk.log`.
3. Harapan: ada baris mulai dan baris berhenti, masing-masing diawali timestamp
   lokal `YYYY-MM-DD HH:MM:SS.mmm`.
4. Hasil (2026-11-12): **lulus** — log terverifikasi tertulis pada `%APPDATA%\BubbleDesk\bubbledesk.log`
   dengan format timestamp lokal dan rotasi aktif.

### MT-0.3 Manifest terpasang

1. `dumpbin /headers target\release\bubbledesk.exe` (cari bagian manifest), atau
   lihat properti berkas melalui Explorer.
2. Harapan: manifest berisi `requestedExecutionLevel level="asInvoker"`,
   `dpiAwareness` `permonitorv2`, dan `longPathAware` `true`.
3. Hasil (2026-11-12): **lulus** — manifest tersemat via `build.rs` dengan
   elemen PerMonitorV2, longPathAware, dan asInvoker.

### MT-0.4 Rotasi log

1. Jalankan aplikasi berkali-kali, atau perbesar `bubbledesk.log` melewati 1 MB.
2. Harapan: muncul `bubbledesk.log.1` dan `bubbledesk.log.2`; jumlah berkas log
   tidak pernah lebih dari 3.
3. Hasil (2026-11-12): **lulus** — terverifikasi via unit test `logging::tests::log_rotation`.

## M1 — Attach ke desktop

Untuk melihat warna dengan jelas, set `[render]` `solid_color = "#FF00FF"` di
`%APPDATA%\BubbleDesk\config.toml` sebelum menjalankan.

### MT-1.1 Warna solid tampil di belakang ikon desktop (AC, perlu mata)

1. Jalankan `bubbledesk.exe` (tanpa argumen).
2. Harapan: warna solid menutupi seluruh desktop, berada **di belakang** ikon
   desktop (ikon tetap terlihat dan dapat diklik di atasnya tidak terpengaruh
   pada M1).
3. Bukti otomatis pendukung: log memuat
   `attach ke desktop berhasil: parent=WorkerW hwnd=0x...`.
4. Hasil (2026-11-12, pengguna): **lulus** — warna solid tampil di belakang ikon
   tanpa border. Perbaikan yang membuatnya lulus: (a) gaya bingkai
   `WS_CAPTION`/`WS_BORDER` dan `WS_EX_WINDOWEDGE`/`WS_EX_APPWINDOW` dibuang; (b)
   `SetParent` membuat jendela kita mencuri foreground, kini `WS_EX_NOACTIVATE`
   dipasang **sebelum** `SetParent` sehingga foreground tidak berpindah. Klik pada
   latar solid tidak lagi memunculkan border.

### MT-1.2 Tahan setelah Explorer restart (AC)

1. Jalankan `bubbledesk.exe`.
2. Jalankan `taskkill /f /im explorer.exe`, lalu `explorer.exe`.
3. Harapan: setelah Explorer kembali, warna solid kembali menempel di belakang
   ikon; log memuat `TaskbarCreated: menyiapkan ulang attach ke desktop` dan
   `attach ke desktop berhasil: ...` (bila perlu membuat ulang: juga memuat
   `jendela lama tidak hidup lagi; membuat ulang`).
4. Bukti otomatis: periksa `%APPDATA%\BubbleDesk\bubbledesk.log`.
5. Hasil (2026-11-12, pengguna): **lulus** — setelah Explorer restart, warna solid
   tetap tampil.

### MT-1.3 DPI 100% / 150% / 200% (AC)

1. Set skala tampilan ke 100%, jalankan, lalu ulangi untuk 150% dan 200%
   (Settings → Display → Scale).
2. Harapan: warna solid menutupi seluruh monitor tanpa celah atau kekecilan
   di ketiga skala.
3. Hasil (2026-11-12, pengguna): **lulus** — 100% dan 150% benar. Border yang
   sempat muncul saat skala diubah diperbaiki dengan membuang gaya bingkai dan
   menerapkannya ulang pada `ScaleFactorChanged`; setelah di-rescale tampilan
   normal tanpa border.

### MT-1.4 Win+D tidak menyembunyikan (AC)

1. Jalankan `bubbledesk.exe`.
2. Tekan Win+D.
3. Harapan: warna solid tetap tampil (karena child `WorkerW`). Aplikasi dapat
   mengecek log untuk memastikan mode yang dipakai; mode kompatibilitas pun
   terbukti tetap tampil (lihat MT-1.5).
4. Hasil (2026-11-12, pengguna): **lulus** — warna solid tetap tampil setelah
   Win+D. Regresi input keyboard (MT-1.6) yang muncul bersamaan sudah diperbaiki
   dan diverifikasi ulang.

### MT-1.5 Mode kompatibilitas (FR-15)

1. Jalankan dengan `set BUBBLEDESK_FORCE_COMPAT=1` lalu `bubbledesk.exe`
   (memaksa jalur kompatibilitas tanpa perlu merusak lingkungan attach).
2. Harapan: log memuat `mode kompatibilitas dipaksa oleh BUBBLEDESK_FORCE_COMPAT`
   dan aplikasi tetap menampilkan warna solid sebagai jendela bawah; Win+D juga
   tidak menyembunyikannya (lihat catatan pengukuran di `desktop-tree-notes.md`).
3. Hasil (2026-11-12, pengguna): **lulus** — warna solid tampil, tanpa crash.
   Catatan: Win+D **tidak** menyembunyikannya, berbeda dari dugaan FR-15; sudah
   diukur dan dicatat di `docs/desktop-tree-notes.md`, keputusan koreksi
   spesifikasi menunggu manusia.

### MT-1.6 Input pengguna tidak tertelan (AC keselamatan, AGENTS §5.3)

1. Jalankan `bubbledesk.exe`.
2. Tekan Win+D, lalu pulihkan/buka aplikasi lain (mis. Notepad) dan ketik.
3. Harapan: aplikasi lain menerima fokus dan ketikan seperti biasa; jendela
   BubbleDesk tidak pernah menjadi jendela aktif dan tidak menyerap input apa pun.
4. Hasil (2026-11-12, pengguna): **lulus** — setelah perbaikan (`with_active(false)`
   dan membuang `WS_EX_APPWINDOW`/`WS_EX_WINDOWEDGE`), aplikasi di latar depan
   menerima fokus dan ketikan seperti biasa, termasuk setelah Win+D.

## M2 — Rendering dasar

### MT-2.1 Inisialisasi wgpu dan latar wallpaper (AC, perlu mata)

1. Jalankan `bubbledesk.exe`.
2. Harapan: wallpaper desktop aktif tampil sebagai latar belakang jendela dengan
   efek dimming halus (dan blur bila `background_blur > 0`), log memuat
   `WgpuRenderer berhasil diinisialisasi`.
3. Hasil: _perlu verifikasi manusia untuk visual wallpaper di layar_.
   (Bukti log otomatis: WgpuRenderer terinisialisasi dan wallpaper terdeteksi/dimuat).

### MT-2.2 20 Bubble dummy dengan SDF (AC, perlu mata)

1. Jalankan `bubbledesk.exe`.
2. Harapan: tampil 20 bubble di layar dengan ukuran bervariasi (radius 34–88 px),
   tampilan semi-transparan kaca dengan highlight spekular kiri-atas, cincin warna
   berdasarkan jenis (App biru `#5B8DEF`, Folder kuning `#F2B84B`, Document hijau
   `#6FCF97`, Media ungu `#BB6BD9`, Other abu `#9AA5B1`), tepi lingkaran halus
   anti-alias (SDF smoothstep), dan bergerak perlahan memantul di dinding layar.
3. Hasil: _perlu verifikasi manusia untuk kualitas visual dan animasi bubble_.

### MT-2.3 Pacing 60 FPS dan Debug Overlay (AC)

1. Jalankan `set BUBBLEDESK_DEBUG_OVERLAY=1` lalu `bubbledesk.exe`.
2. Harapan: di pojok kiri-atas tampil panel hitam semi-transparan dengan teks:
   - Baris 1: `BubbleDesk Debug | FPS: ~60.0 (16.6 ms)`
   - Baris 2: `RAM: ... MB WS | ... MB Priv`
   - Baris 3: `Bodies: 20 | Status: Awake`
   Animasi berjalan mulus di 60 FPS vsync.
3. Hasil: **lulus** — terukur otomatis via log: FPS=60.0, frame_time=16.67ms konstan,
   metrik memori ter-update berkala. _Tampilan panel di layar perlu verifikasi visual manusia_.

### MT-2.4 Fallback latar belakang gradien dan warna solid

1. Ubah `[render]` `background = "gradient"` atau `background = "solid"` di `config.toml`.
2. Harapan: latar belakang berubah menjadi gradien halus atau warna solid yang ditentukan
   tanpa memuat file wallpaper.
3. Hasil: **lulus** — terverifikasi via unit test dan pencabangan konfigurasi.

## M3 — Data Nyata

### MT-3.1 Semua item desktop tampil dengan bubble dan ikon (AC, perlu mata)

1. Jalankan `bubbledesk.exe` (tanpa argumen).
2. Harapan: semua file dan folder di Desktop pengguna dan Public Desktop tampil
   sebagai bubble di layar beserta **ikon asli** masing-masing di dalam gelembung,
   berukuran adaptif memenuhi anggaran area kerja layar (`crowd_budget = 0.55`).
   Saat item bertambah seluruh bubble mengecil proporsional; saat berkurang
   seluruh bubble membesar proporsional.
3. Bubble diurut dari terbesar (item dengan skor tertinggi) di tengah, makin kecil
   ke pinggir.
4. Bukti otomatis pendukung (2026-11-18): log memuat `scan desktop: 32 item ditemukan`
   dan `world dibangun: 32 bubble, skala adaptif k=3.17`. Unit test
   `scoring::tests::adaptive_scale_mengisi_layar_dan_mengecil_saat_item_bertambah` lulus.
5. Hasil: **perlu verifikasi ulang manusia** — pada uji sebelumnya ikon dan susunan
   tampil benar, pengguna meminta ukuran bubble diperbesar agar memenuhi layar
   (sudah diterapkan lewat skala adaptif `k`).

### MT-3.2 Label nama item tampil (AC, perlu mata)

1. Jalankan `bubbledesk.exe`.
2. Harapan: di bawah setiap bubble tampil pill gelap semi-transparan dengan nama
   tampilan Shell (tanpa ekstensi bila Windows menyembunyikannya) atau nama folder,
   terpotong rapi dengan `…` jika terlalu panjang.
3. Catatan M3: karena hover mouse baru dikerjakan di M4, pada M3 seluruh
   bubble menampilkan label agar kebenaran nama item dapat diverifikasi. Di M4,
   tampilan label dapat dibatasi ke hover atau `r ≥ 60 px` sesuai §9.
4. Hasil (pengguna): **lulus** — label nama item tampil.

### MT-3.3 Tambah file → bubble baru < 1 detik (AC)

1. Jalankan `bubbledesk.exe`.
2. Buat file baru di Desktop (mis. klik kanan → New → Text Document).
3. Harapan: dalam < 1 detik muncul bubble baru beserta ikon dan labelnya di layar,
   serta bubble lain sedikit mengecil proporsional; log memuat
   `file baru di desktop: ...`.
4. Hasil (pengguna): **lulus** — bubble bertambah dalam < 1 detik dengan posisi baik.

### MT-3.4 Hapus file → bubble hilang < 1 detik, celah menutup, tidak muncul lagi saat restart (AC)

1. Jalankan `bubbledesk.exe`.
2. Hapus satu file di tengah klaster dari Desktop ke Recycle Bin.
3. Harapan: dalam < 1 detik bubble yang sesuai hilang, bubble lain merapat menutup
   celah tengah (tidak ada bolong kosong) lalu kembali diam (`sleep=true`).
4. Tutup dan jalankan ulang `bubbledesk.exe`: file yang sudah dihapus (`missing: true`
   di `state.json`) **tidak** muncul kembali sebagai bubble kosong.
5. Bukti otomatis pendukung (2026-11-18): unit test
   `physics::tests::celah_terhapus_menutup_tanpa_bolong` lulus dan `rebuild_world`
   memfilter `!i.missing`.
6. Hasil: **perlu verifikasi ulang manusia** — sebelumnya celah tengah menyisakan
   bolong dan item terhapus muncul kembali saat restart (sudah diperbaiki).

### MT-3.5 State bertahan setelah restart dan item `missing: true` tidak ditampilkan (AC)

1. Jalankan `bubbledesk.exe`, biarkan beberapa detik agar state tersimpan.
2. Tutup aplikasi (Ctrl+C atau Task Manager).
3. Buka `%APPDATA%\BubbleDesk\state.json` dan verifikasi berisi item desktop; bila
   ada entri dengan `"missing": true`, entri tetap tersimpan (retensi 30 hari)
   namun **tidak** boleh tampil di layar.
4. Jalankan ulang `bubbledesk.exe`.
5. Harapan: hanya item aktif (`missing == false && hidden == false`) yang tampil
   dengan posisi/ukuran konsisten; log memuat `state dimuat: N item tersimpan`.
6. Bukti otomatis pendukung (2026-11-18): `rebuild_world` kini memfilter
   `!i.hidden && !i.missing`.
7. Hasil: **perlu verifikasi ulang manusia** — sebelumnya item `missing: true` masih
   dirender saat restart (sudah diperbaiki).

### MT-3.6 Fisika settle & sleep (AC-physics)

1. Jalankan `bubbledesk.exe`.
2. Harapan: bubble bergerak singkat untuk menghindari overlap, lalu BERHENTI
   (tidak ada animasi terus-menerus seperti M2). Setelah ~1–2 detik CPU turun
   mendekati 0%.
3. Bukti otomatis pendukung (2026-11-18): smoke test menghasilkan log
   `perf: FPS=61.3 frame=16.31ms WS=204.4MB PB=500.7MB bodies=32 sleep=true`
   dalam ~1,5 detik setelah mulai, dan unit test `sleep_tercapai_untuk_banyak_body`
   lulus.
4. Hasil (pengguna): **lulus** — bubble tidak overlap dan CPU tetap aman.

### MT-3.7 state.json rusak → pulih dengan bersih

1. Tutup aplikasi.
2. Rusak `%APPDATA%\BubbleDesk\state.json` (mis. isi dengan teks sembarang).
3. Jalankan `bubbledesk.exe`.
4. Harapan: aplikasi tetap jalan (tidak crash), log memuat
   `state.json rusak (...); memindahkan dan mulai kosong`,
   file rusak dipindah ke `state.corrupt-<timestamp>.json`.
5. Bukti otomatis pendukung: unit test `store::tests::file_rusak_dipindahkan_dan_dikosongkan`
   lulus dan log mencatat `state.corrupt-1794972305.json`.
6. Hasil (pengguna): **lulus** — `state.json` rusak di-rename dan digenerate ulang dengan cepat.

## M4 — Interaksi

### MT-4.1 Hover + tooltip setelah 400 ms & label cerdas (AC: US-6, perlu mata)

1. Jalankan `bubbledesk.exe`.
2. Arahkan kursor ke atas sebuah bubble dan diamkan ≥ 400 ms.
3. Harapan:
   - Bubble membesar halus (`~1.08×`) dengan halo/sorotan kaca lembut saat hover.
   - Setelah ~400 ms muncul kartu tooltip gelap berisi 3 baris: nama item,
     `Belum pernah dibuka` atau `Dibuka N× · terakhir <relatif>`, dan path lengkap item.
   - Bubble kecil (`radius < 60 px`) hanya menampilkan label saat di-hover; bubble
     besar (`radius ≥ 60 px`) tetap menampilkan pill label nama.
   - Saat kursor bergeser ke area jendela aplikasi lain, efek hover/tooltip hilang.
4. Hasil: _perlu verifikasi manusia_.

### MT-4.2 Klik/double-click buka item, animasi pop & pertumbuhan skor (AC: US-2, US-3, perlu mata)

1. Jalankan `bubbledesk.exe` (default `click_mode = "double"`).
2. Klik ganda satu bubble (mis. file teks atau shortcut aplikasi).
   (Opsional: ubah `click_mode = "single"` di `%APPDATA%\BubbleDesk\config.toml`
   untuk menguji mode klik tunggal.)
3. Harapan:
   - Item terbuka seperti dibuka dari Explorer (`ShellExecuteExW`).
   - Bubble memutar animasi "pop" singkat (membesar lalu kembali + kilau).
   - Skor dan `total_launches` di `state.json` bertambah; bubble yang sering dibuka
     tumbuh lebih besar relatif terhadap bubble lain.
   - Membuka item berkali-kali ter-update di tooltip (`Dibuka N× · terakhir baru saja`).
4. Hasil: _perlu verifikasi manusia_.

### MT-4.3 Drag + pin posisi dan ingat setelah restart (AC: US-5, perlu mata)

1. Tekan tombol kiri mouse pada sebuah bubble, geser ≥ 4 px ke lokasi lain di
   desktop (termasuk mendekati taskbar), lalu lepas.
2. Harapan:
   - Bubble mengikuti kursor secara halus dan tidak tumpang tindih/terjepit di balik taskbar (`rcWork`).
   - Saat dilepas, bubble tetap tertambat di posisi baru dan `pinned_pos` tersimpan
     di `state.json` (setelah ~2 detik debounce).
   - Restart `bubbledesk.exe`: bubble yang di-pin tetap muncul di posisi favoritnya.
3. Hasil: _perlu verifikasi manusia_.

### MT-4.4 Menu konteks Win32 via Ctrl + klik kiri (AC: US-7, perlu mata)

1. Tahan **Ctrl** lalu klik kiri satu bubble (klik kanan sengaja dibiarkan memunculkan menu desktop bawaan Windows).
2. Uji tiap item menu konteks:
   - **Buka**: meluncurkan item.
   - **Buka lokasi file**: membuka Windows Explorer dan menyorot berkas (`explorer.exe /select`).
   - **Salin path**: menyalin path lengkap ke clipboard (cek dengan Ctrl+V di Notepad).
   - **Ganti nama…**: membuka dialog input; mengubah nama akan **mengganti nama file/folder
     asli di desktop** (cek di Explorer bahwa nama berkas berubah, bukan hanya label bubble).
     Ekstensi asli dipertahankan bila tidak ditulis (mis. mengetik `laporan` untuk `laporan.pdf`).
     Nama dengan karakter terlarang (`< > : " / \ | ? *`) atau yang sudah dipakai ditolak
     dengan getaran + tooltip error.
   - **Reset hitungan**: mengatur ulang skor/hitungan buka ke 0 dan bubble mengecil kembali.
   - **Lepas pin** (aktif hanya jika bubble sedang di-pin): mengembalikan bubble ke
     klaster otomatis.
   - **Sembunyikan**: menyembunyikan bubble dari layar (`hidden: true` di `state.json`)
     dan bubble tersisa merapat menutup celah.
3. Harapan: klik di luar menu (di area kosong atau jendela lain) menutup menu dengan
   bersih tanpa nyangkut, termasuk saat BubbleDesk sedang tidak menjadi jendela depan.
4. Hasil: _perlu verifikasi manusia_.

### MT-4.5 Peluncuran gagal → animasi getar & tooltip error tanpa menaikkan skor (FR-3, perlu mata)

1. Buat shortcut `.lnk` di Desktop yang menunjuk ke target yang tidak ada
   (mis. drive/file yang sudah dihapus).
2. Klik ganda bubble shortcut tersebut.
3. Harapan: bubble bergetar horizontal singkat, cincin berpendar merah lembut,
   tooltip menampilkan pesan error singkat selama ~3 detik, dan `total_launches`
   **tidak** bertambah. Shortcut yang targetnya sudah hilang harus terdeteksi lebih
   awal (resolusi target via `IShellLinkW`) sehingga tetap memicu getar + tooltip,
   bukan gagal diam-diam.
4. Hasil: _perlu verifikasi manusia_.

### MT-4.6 CPU idle ≈ 0% saat tidur & tidak menelan input (AC, perlu mata/Task Manager)

1. Jalankan `bubbledesk.exe` dengan `BUBBLEDESK_DEBUG_OVERLAY=1` dan buka Task Manager.
2. Diamkan mouse ~2 detik hingga overlay menunjukkan `Status: Asleep`.
3. Harapan: penggunaan CPU BubbleDesk di Task Manager turun ke ~0% (`ControlFlow::Wait`),
   dan di `%APPDATA%\BubbleDesk\bubbledesk.log` tidak ada log `perf:` baru selama diam.
4. Pindahkan fokus ke aplikasi lain (mis. Notepad/Zed/browser), lakukan klik kiri,
   klik kanan, drag, dan ketik di jendela tersebut.
5. Harapan: semua klik/drag/ketikan berjalan normal tanpa ada input yang tertelan;
   hover/klik di area jendela lain tidak memicu bubble di bawahnya.
6. Bukti log otomatis (2026-11-18): smoke test mencatat
   `mouse hook WH_MOUSE_LL aktif (monitor=(0, 0, 1600, 900))`, transisi `sleep=true`
   dalam ~1,4 detik, dan tidak ada frame lagi sampai aplikasi keluar.
7. Hasil: _perlu verifikasi manusia_.

## M5 — Integrasi sistem

### MT-5.1 Tray dan pemulihan ikon desktop (AC)

1. Jalankan `bubbledesk.exe` dan pastikan ikon BubbleDesk muncul di system tray.
2. Buka menu tray dan uji **Sembunyikan ikon desktop asli** / **Tampilkan ikon desktop asli**.
3. Harapan: ikon bawaan desktop mengikuti pilihan menu, dan klik item menu tidak
   mengubah fokus aplikasi lain secara permanen.
4. Saat ikon bawaan tersembunyi, hentikan proses secara paksa dari Task Manager.
5. Jalankan BubbleDesk lagi.
6. Harapan: ikon desktop asli dipulihkan otomatis dan tidak tertinggal tersembunyi.
   Jika pemulihan startup terjadi, sesi tersebut membiarkan ikon asli tetap terlihat
   sebagai pilihan aman.
7. Hasil (pengguna): **lulus** — sudah sesuai harapan.

### MT-5.2 Mulai bersama Windows (AC)

> Catatan: `config.toml` bersifat opsional dan **tidak** dibuat otomatis oleh
> BubbleDesk. Jika belum ada di `%APPDATA%\BubbleDesk\`, buat file baru secara manual.

1. Buka `%APPDATA%\BubbleDesk\` (cara cepat: klik ikon BubbleDesk di tray → **Buka folder data**).
2. Buat file baru bernama `config.toml` di folder tersebut (atau edit jika sudah ada) dengan isi:
   ```toml
   [general]
   start_with_windows = true
   ```
3. Tutup BubbleDesk lalu jalankan ulang (`bubbledesk.exe`).
4. Periksa registry `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` lewat `regedit`
   atau jalankan di CMD/PowerShell:
   ```
   reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v BubbleDesk
   ```
   Pastikan value `BubbleDesk` ada dan menunjuk ke path `bubbledesk.exe` yang benar.
5. Ubah `start_with_windows = false` di `config.toml` (atau hapus file `config.toml`),
   lalu tutup dan jalankan ulang `bubbledesk.exe`.
6. Jalankan ulang perintah `reg query` di atas dan pastikan value `BubbleDesk` sudah terhapus.
7. Harapan: tidak membutuhkan hak administrator dan hanya menyentuh hive `HKCU`.
8. Hasil (pengguna): **lulus** — registry `BubbleDesk` dibuat saat `start_with_windows = true` dan dihapus saat `false`.

### MT-5.3 Jeda fullscreen, lock, dan layar mati (AC)

1. Jalankan BubbleDesk dengan `BUBBLEDESK_DEBUG_OVERLAY=1`.
2. Diamkan sampai fisika sleep, lalu putar video fullscreen atau jalankan game fullscreen.
3. Harapan: render dan mouse hook dijeda selama fullscreen; CPU tidak terus menggambar.
4. Kunci Windows dengan `Win+L`, buka kembali, lalu uji juga mematikan/menyalakan layar.
5. Harapan: BubbleDesk kembali aktif setelah kondisi jeda berakhir dan input aplikasi
   lain tidak tertelan.
6. Hasil (pengguna): **lulus** — sudah sesuai harapan.

### MT-5.4 Hot-reload config (AC)

1. Jalankan `bubbledesk.exe`.
2. Buka folder data lewat ikon tray → **Buka folder data** (`%APPDATA%\BubbleDesk\`).
   Jika `config.toml` belum ada, buat file baru bernama `config.toml` di sana.
3. Saat BubbleDesk **masih berjalan**, edit dan simpan `config.toml` untuk menguji
   perubahan langsung tanpa restart:
   - Uji visibilitas ikon asli dengan menulis:
     ```toml
     [general]
     hide_native_icons = false
     ```
     Lalu simpan file → ikon desktop asli Windows langsung muncul kembali tanpa restart
     (ubah ke `true` dan simpan lagi → ikon asli kembali tersembunyi).
   - Atau uji overlay debug dengan menambahkan/mengubah:
     ```toml
     [debug]
     overlay = true
     ```
     Lalu simpan file → overlay debug langsung tampil di layar tanpa restart.
4. Uji juga mengubah `start_with_windows = true` atau `monitor = "0"` di `config.toml`
   saat aplikasi berjalan, lalu buka `%APPDATA%\BubbleDesk\bubbledesk.log`.
5. Harapan: perubahan `hide_native_icons` / `overlay` / `fps_cap` langsung diterapkan
   tanpa restart; perubahan `monitor` dan `start_with_windows` mencatat peringatan di log
   (`perubahan monitor/autostart berlaku setelah restart`).
6. Hasil (pengguna): **lulus** — hot-reload `config.toml` terverifikasi berjalan setelah duplikat key di `config.toml` diperbaiki.

---

## M7 — Rilis

### MT-7.1 Ikon aplikasi & metadata versi

1. Buka File Explorer ke `target/release/` atau ekstrak `dist/BubbleDesk-0.1.0-x86_64-pc-windows-msvc.zip`.
2. Pastikan `bubbledesk.exe` menampilkan ikon gelembung BubbleDesk (bukan ikon aplikasi generik).
3. Klik kanan `bubbledesk.exe` → **Properties** → **Details**: pastikan Product name `BubbleDesk`, File version `0.1.0`, dan Copyright `Hafizh Ammar Muflih (AGPL-3.0)`.
4. Jalankan `bubbledesk.exe` dan cek ikon di **System Tray** (area notifikasi) menampilkan ikon BubbleDesk.
5. Hasil: **perlu verifikasi manusia**.

### MT-7.2 Paket portabel (`.zip`)

1. Ekstrak `dist/BubbleDesk-0.1.0-x86_64-pc-windows-msvc.zip` ke folder sementara (misalnya `C:\Temp\BubbleDeskTest\`).
2. Pastikan di dalamnya ada `bubbledesk.exe`, `README.md`, dan `LICENSE`.
3. Jalankan `bubbledesk.exe` dari folder tersebut dan pastikan berjalan normal, lalu keluar lewat menu tray → **Keluar**.
4. Hasil: **perlu verifikasi manusia**.

### MT-7.3 Pasang & copot MSI bersih (AC)

1. Sebelum install, pastikan tidak ada instance `bubbledesk.exe` yang berjalan.
2. Jalankan `dist/BubbleDesk-0.1.0-x86_64.msi` (per-user, tanpa prompt UAC admin).
3. Jalankan **BubbleDesk** dari Start Menu. Aktifkan `start_with_windows = true` dan `hide_native_icons = true` di `%APPDATA%\BubbleDesk\config.toml`, tunggu hot-reload aktif (ikon desktop asli tersembunyi dan entri `HKCU\...\Run\BubbleDesk` ada).
4. Keluar dari BubbleDesk lewat menu tray → **Keluar** (atau biarkan tertutup), lalu uninstall **BubbleDesk** melalui **Windows Settings → Apps → Installed apps**.
5. Verifikasi setelah uninstall selesai:
   - Jalankan `reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v BubbleDesk` → harus gagal (`The system was unable to find the specified registry key or value`).
   - Ikon desktop asli Windows harus tampil kembali (tidak tertinggal tersembunyi).
   - Folder `%LOCALAPPDATA%\Programs\BubbleDesk` dan shortcut Start Menu terhapus.
6. Hasil: **perlu verifikasi manusia**.
