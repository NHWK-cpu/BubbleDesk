# Catatan struktur pohon jendela desktop

Dokumen ini mencatat struktur jendela yang diamati saat runtime (§7.2), karena
teknik `WorkerW` tidak terdokumentasi resmi. Perbarui setiap ada temuan baru
atau perubahan Windows. Data diambil dengan `bubbledesk --dump-desktop-tree`.

## Lingkungan yang diuji

| Tanggal | OS | Resolusi/DPI | Catatan |
|---|---|---|---|
| 2026-11-12 | Windows 10 22H2 (build 10.0.19045.6466) | 1600×900, 100% | Verifikasi awal M1 |

## Baseline — sebelum pesan 0x052C dikirim

`SHELLDLL_DefView` masih menjadi child langsung `Progman`:

```
Progman            hwnd=0x10110 visible=true rect=(0,0,1600,900) "Program Manager"
  SHELLDLL_DefView hwnd=0x10114 visible=true rect=(0,0,1600,900)
    SysListView32  hwnd=0x10116 visible=true rect=(0,0,1600,900) "FolderView"
      SysHeader32  hwnd=0x1011a visible=false
```

Ada pula banyak jendela top-level berkelas `WorkerW` milik proses lain
(berukuran kecil `(0,0,136,39)`). Karena itu kelas saja tidak cukup untuk
mengidentifikasi target; harus lewat struktur (Layout A).

## Setelah pesan 0x052C dikirim — Layout A terdeteksi

`SendMessageTimeoutW(Progman, 0x052C, 0xD, 0x1)` membuat Explorer memindahkan
`SHELLDLL_DefView` ke sebuah `WorkerW` top-level baru, lalu membuat `WorkerW`
kedua yang **kosong** tepat sesudahnya. `WorkerW` kosong inilah tempat jendela
kita menjadi child sehingga tampil di belakang ikon:

```
WorkerW            hwnd=0x60482  visible=true rect=(0,0,1600,900)
  SHELLDLL_DefView hwnd=0x10114  visible=true rect=(0,0,1600,900)
    SysListView32  hwnd=0x10116  visible=true rect=(0,0,1600,900) "FolderView"
WorkerW            hwnd=0x1b02a8 visible=true rect=(0,0,1600,900)   <- target attach
  Window Class     hwnd=0xa054a  visible=true rect=(0,0,1600,900) "BubbleDesk"  (jendela kita)
Progman            hwnd=0x10110  visible=true rect=(0,0,1600,900) "Program Manager"
```

Urutan z-order: `WorkerW(DefView)` di depan, lalu `WorkerW(sasaran)`, lalu
`Progman`. Jadi jendela kita berada di belakang lapisan ikon (`SysListView32`).

## Algoritma yang dipakai (Layout A)

1. `FindWindowW("Progman")`.
2. `SendMessageTimeoutW(progman, 0x052C, 0xD, 0x1, ...)`; ulangi sekali dengan
   `(0xD, 0x0)` bila gagal.
3. `EnumWindows`: cari top-level yang memiliki child `SHELLDLL_DefView`.
4. `FindWindowExW(null, hwnd, "WorkerW", null)` → `WorkerW` top-level setelahnya.
5. `SetParent(hwnd, workerw)`, ubah style ke `WS_CHILD | WS_VISIBLE`
   (+`WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`), lalu `SetWindowPos` ke rect monitor.

Terverifikasi pada Windows 10 22H2: log mencatat
`attach ke desktop berhasil: parent=WorkerW hwnd=0x...`, dan saat aplikasi
berjalan jendela `Window Class ("BubbleDesk")` tampak sebagai child visible
`WorkerW` sasaran.

## Layout B (Windows 11 24H2+)

Belum ada mesin uji Windows 11. Sesuai M1, Layout B hanya **dideteksi**: bila
`Progman` memiliki child `WorkerW`, kita mencatat peringatan dan masuk mode
kompatibilitas. Dukungan penuh menyusul setelah M5 dan hanya bila ada mesin uji.

## Mode kompatibilitas dan Win+D (temuan 2026-11-12)

FR-15 (spec §4) menyebut jendela mode kompatibilitas akan tersembunyi saat Win+D.
Pengukuran pada Windows 10 22H2 menunjukkan sebaliknya: jendela tetap tampil.

Kondisi jendela saat mode kompatibilitas dipaksa (`BUBBLEDESK_FORCE_COMPAT=1`):

```
top-level, parent=0x0
style   = 0x96000000 (WS_POPUP | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN)
exstyle = 0x08000090 (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_ACCEPTFILES)
```

Hasil setelah mengirim Show Desktop (`WM_COMMAND` `0x419` ke `Shell_TrayWnd`,
setara Win+D), diamati lewat `IsWindowVisible`/`IsIconic`:

| Tahap | `IsWindowVisible` | `IsIconic` |
|---|---|---|
| sebelum | true | false |
| setelah Show Desktop | true | false |
| setelah toggle balik | true | false |

Jendela dengan gaya di atas tidak ikut diminimalkan oleh Show Desktop. Dampaknya
menguntungkan: mode kompatibilitas tidak kehilangan warna solid saat Win+D, jadi
tidak ada perubahan implementasi yang diperlukan. Perlu keputusan manusia apakah
kalimat "akan tersembunyi saat Win+D" pada FR-15 dikoreksi.

## R6 — WorkerW milik aplikasi wallpaper lain

Sebelum attach, child langsung `WorkerW` sasaran diperiksa. `WorkerW` sasaran
yang normal kosong; bila ada child milik proses lain, kita tidak menimpanya dan
masuk mode kompatibilitas (dicatat di log).
