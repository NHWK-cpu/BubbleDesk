<div align="center">

<h1><img src="assets/BubbleDesk_icon.png" alt="BubbleDesk Logo" width="40" align="middle" />&nbsp;BubbleDesk</h1>

**A lightweight Windows live-wallpaper desktop that turns your desktop items into living, usage-aware bubbles.**

<p>
  <a href="README.md"><strong>English</strong></a> ·
  <a href="README.id.md">Bahasa Indonesia</a>
</p>

[![Release](https://img.shields.io/github/v/release/NHWK-cpu/BubbleDesk?style=flat-square&color=0078D4&label=Release)](https://github.com/NHWK-cpu/BubbleDesk/releases/tag/v0.1.0)
[![CI](https://img.shields.io/github/actions/workflow/status/NHWK-cpu/BubbleDesk/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/NHWK-cpu/BubbleDesk/actions/workflows/ci.yml)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%2022H2%2B-0078D6?style=flat-square&logo=windows)](https://github.com/NHWK-cpu/BubbleDesk/releases/tag/v0.1.0)
[![Rust](https://img.shields.io/badge/Built%20with-Rust%20%2B%20wgpu-orange?style=flat-square&logo=rust)](Cargo.toml)
[![License](https://img.shields.io/badge/License-AGPL--3.0-blue?style=flat-square)](LICENSE)

<p>
  <a href="#-preview">Preview</a> •
  <a href="#-features">Features</a> •
  <a href="#-installation">Installation</a> •
  <a href="#-controls">Controls</a> •
  <a href="#-configuration">Configuration</a> •
  <a href="#-build-from-source">Build</a> •
  <a href="#-roadmap">Roadmap</a>
</p>

</div>

---

## ✨ Preview

<div align="center">
  <img src="assets/demo.gif" alt="BubbleDesk Live Demo" width="760" />
  <br />
  <sub>Bubbles settle naturally on the desktop wallpaper, respond to hover and drag, and scale based on how often you launch them.</sub>
</div>

---

## 🚀 Features

Every file, folder, and shortcut on your Windows Desktop becomes an interactive bubble rendered directly behind desktop icons.

<table>
  <tr>
    <td width="50%" valign="top">
      <h4>🫧 Usage-Driven Bubble Sizing</h4>
      <p>Items you open frequently grow larger (up to a configurable cap); rarely used items gradually shrink back over time.</p>
    </td>
    <td width="50%" valign="top">
      <h4>🪟 Native Desktop Integration</h4>
      <p>Attaches behind desktop icons via Windows <code>WorkerW</code>, stays visible on <code>Win+D</code>, and auto-reattaches if Explorer restarts.</p>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <h4>⚡ Zero-Waste Power & Physics</h4>
      <p>Anti-overlap physics settles and sleeps at <strong>~0% CPU</strong> (0 frames rendered while idle). Auto-pauses on fullscreen apps/games, <code>Win+L</code>, or display off.</p>
    </td>
    <td width="50%" valign="top">
      <h4>🔄 Real-Time Desktop Sync</h4>
      <p>Adding, renaming, or deleting files on your Desktop updates the bubbles automatically in real time.</p>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <h4>🛡️ Non-Destructive & Safe</h4>
      <p>No admin rights, no network access, no telemetry. Never moves or deletes your files in the background.</p>
    </td>
    <td width="50%" valign="top">
      <h4>🔁 Guaranteed Icon Recovery</h4>
      <p>If native desktop icons are hidden, BubbleDesk restores them on normal exit, on crash (panic hook), on next startup, or anytime from the system tray.</p>
    </td>
  </tr>
</table>

---

## 📦 Installation

### Option 1 — MSI Installer (Recommended)

1. Download `BubbleDesk-0.1.0-x86_64.msi` from the [**v0.1.0 Release**](https://github.com/NHWK-cpu/BubbleDesk/releases/tag/v0.1.0).
2. Run the installer — installs per-user into `%LOCALAPPDATA%\Programs\BubbleDesk` with **no administrator privileges** required.
3. Launch **BubbleDesk** from the Start Menu.

> **Clean Uninstall:** Uninstalling via Windows **Apps & Features** automatically cleans up the `HKCU\...\Run\BubbleDesk` autostart entry and restores native desktop icons if they were hidden.

### Option 2 — Portable Archive (`.zip`)

1. Download the portable `.zip` from [**v0.1.0 Release**](https://github.com/NHWK-cpu/BubbleDesk/releases/tag/v0.1.0).
2. Extract it to any folder.
3. Run `bubbledesk.exe`.

> **System Requirements:** Windows 10 22H2 (`x86_64`) with DirectX 12 support. Windows 11 compatibility is currently fallback-based (`HWND_BOTTOM`) and scheduled for dedicated `WorkerW` Layout B testing after M5.

---

## 🎮 Controls

> **Note:** The application UI (tray menu, tooltips, and context menu) is currently localized in Indonesian.

| Interaction | Action |
|---|---|
| **Double-click** *(default)* / Single-click | Launch file, folder, or shortcut |
| **Drag** | Move a bubble and pin it in place |
| **Hover** | Show tooltip with item name, launch count, and last opened time |
| **Ctrl + Left-click** | Open context menu *(Open, Open file location, Copy path, Rename, Reset count, Unpin, Hide)* |
| **Tray Icon (Right-click)** | Show/hide native desktop icons, Pause/Resume, Toggle debug overlay, Open data folder, Quit |

---

## ⚙️ Configuration

All settings and state live locally inside `%APPDATA%\BubbleDesk\`:

| File | Purpose |
|---|---|
| `config.toml` | Application settings (supports **hot-reload** while running) |
| `state.json` | Launch scores, launch counts, and pinned bubble positions |
| `bubbledesk.log` | Local diagnostics log (rotated at `1 MB × 3`) |

```toml
[general]
click_mode = "double"       # "double" or "single"
start_with_windows = false  # manage HKCU\...\Run entry
hide_native_icons = true    # hide native desktop icons while active
reduce_motion = false

[render]
background = "wallpaper"    # "wallpaper", "gradient", or "solid"
fps_cap = 60                # active FPS cap
fps_cap_on_battery = 30     # FPS cap when running on battery
```

<details>
<summary><strong>📄 Full <code>config.toml</code> reference & defaults</strong></summary>

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

## 📊 Performance

Measured on **Windows 10 22H2** (AMD Radeon Graphics, `wgpu` DX12 backend, VSync `Fifo`). See [`docs/perf.md`](docs/perf.md) for full notes.

| Metric | Measured Result |
|---|---|
| **Active Frame Rate** | `~59.3 – 60.1 FPS` (`16.6 ms` frame time) |
| **Idle Redraws** | `0 frames` (enters `ControlFlow::Wait` after settling in `~0.3 – 1.5s`) |
| **Working Set (35 live items)** | `~203.5 MB` *(~12–15 MB Rust app memory; remainder is GPU DX12 driver runtime)* |

---

## 🛠️ Build from Source

<details>
<summary><strong>Prerequisites, Cargo commands, and MSI packaging</strong></summary>

### Prerequisites
- **Rust** (`x86_64-pc-windows-msvc`)
- **Windows 10 SDK** (`rc.exe` for embedding app icon & version metadata)
- **[WiX Toolset v5](https://wixtoolset.org/)** *(optional, for building the `.msi` installer)*

### Build & Verify

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

The compiled executable is placed at `target/release/bubbledesk.exe`.

### Useful Debug Flags
- `cargo run -- --dump-desktop-tree` — Prints the Windows `Progman` / `WorkerW` window hierarchy for desktop integration diagnostics.

### Build MSI Installer

```sh
wix build installer/bubbledesk.wxs -d Version=0.1.0 -d ExePath=target/release/bubbledesk.exe -o dist/BubbleDesk-0.1.0-x86_64.msi
```

</details>

---

## 🗺️ Roadmap

- [x] **M0–M1:** Foundation, single-instance lock, logging, `WorkerW` desktop integration & fallback mode
- [x] **M2:** `wgpu` (DX12) SDF bubble renderer, wallpaper backdrop, and debug overlay
- [x] **M3:** Desktop scanner, icon atlas, usage-based scoring/decay, and atomic JSON persistence
- [x] **M4:** Low-level mouse hook, hover tooltips, drag & pin, launch worker, and context menu
- [x] **M5:** System tray, autostart, safe native-icon restoration, power/fullscreen auto-pause, config hot-reload
- [x] **M7:** `v0.1.0` MSI installer & portable release packaging
- [ ] **M5+:** Dedicated Windows 11 24H2+ `WorkerW` Layout B verification
- [ ] **M6 *(optional)*:** Passive usage import (`UserAssist`/Recent), opt-in **Tidy** archive mode with full undo, and multi-monitor support

---

## 📄 License & Author

Copyright © 2026 **Hafizh Ammar Muflih**.  
Licensed under the [**GNU Affero General Public License v3.0 (`AGPL-3.0-only`)**](LICENSE).

<div align="right">
  <sub><a href="#bubbledesk">↑ Back to top</a></sub>
</div>
