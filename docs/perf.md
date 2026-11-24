# Perf BubbleDesk

Angka performa aktual (bukan target). Anggaran ada di spec §5.
Pengukuran dimulai **M2**, saat jalur render pertama ada. Untuk sekarang
belum ada angka karena aplikasi hanya menggambar warna solid lewat GDI
sesaat (bukan loop render).

## Cara mengukur (rencana, spec §18)

Skrip PowerShell mencatat working set setiap 5 detik selama 10 menit pada
kondisi idle dan animasi; hasilnya ditempel di bawah.

## Riwayat

| Milestone | Working set | Private bytes | FPS | Frame time | Catatan |
|---|---|---|---|---|---|
| M0–M1 | belum diukur | - | - | - | belum ada loop render |
| M2 (DX12) | 180.1 MB | 483.0 MB | 60.0 | 16.67 ms | 20 bubble dummy aktif (1600x900), vsync Fifo, D3D12 runtime |
| M3 | 204.4 MB | 500.7 MB | 61.3 | 16.31 ms | 32 bubble nyata desktop, skala adaptif `k=3.17`, atlas ikon 1536×1536 + teks glyph R8, fisika settle + `sleep=true` (~1.5 dtk) |
| M4 | 206.9 MB | 501.1 MB | 58.3–61.7 | 16.20–17.16 ms | 32 bubble + `WH_MOUSE_LL` hook aktif + launcher + `MenuHelper`, `sleep=true` dalam ~1.4 dtk, 0 frame saat `ControlFlow::Wait` |
| M5 | 203.5 MB | 501.7 MB | 59.3–60.1 | 16.63–16.85 ms | 35 bubble + tray + notifikasi daya/sesi + watcher config; `sleep=true` ~0.3 dtk setelah mulai; tidak ada kenaikan RAM nyata dari integrasi sistem (jendela helper & tray memakai memori minimal) |

## Analisis M2 & Risiko R4

Target anggaran RAM (§5):
- Idle: ≤ 40 MB
- Aktif: ≤ 80 MB (100 bubble)

Hasil pengukuran aktual di M2 (Windows 10 22H2, AMD Radeon Graphics):
- Target 60 FPS tercapai sempurna (60.0 FPS konstan, frame time 16.67 ms dengan vsync `Fifo`).
- Working set aktif berada di kisaran **180 MB** (DX12) dan **210 MB** (Vulkan).
- Memori aplikasi Rust murni (tekstur wallpaper 1600×900 RGBA + buffer instance + pipeline state) hanya memakan sekitar **12–15 MB**. Sisanya (~165–195 MB) adalah alokasi bawaan driver grafis GPU (runtime DLL DirectX 12 / Vulkan dan konteks device).
- Karena RAM > 60 MB, ini memicu aturan AGENTS.md §9 dan Risiko R4 (spec §19).
- **Usulan keputusan (§21):** Kode telah dipersiapkan dengan trait `Renderer` (`src/render/mod.rs`). Jika pengguna mengutamakan batas ketat RAM ≤ 40 MB/80 MB, backend CPU (`tiny-skia` + `softbuffer` + dirty rect) dapat diimplementasikan untuk menggantikan `WgpuRenderer`. Jika overhead driver GPU dianggap dapat diterima untuk mendapatkan visual shader modern, `wgpu` dapat dipertahankan. Keputusan diserahkan kepada pengguna.
