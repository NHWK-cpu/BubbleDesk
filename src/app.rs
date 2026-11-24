//! State aplikasi dan loop event (spec §6, §11, §12, §14).
//!
//! M4: Interaksi penuh — mouse hook global, hover + tooltip, klik/peluncuran,
//! drag + pin, menu konteks (Ctrl + klik kiri), animasi pop & shake, serta sleep hemat daya.

use crate::config::{Background, ClickMode, Config};
use crate::model::{Item, ItemKind};
use crate::physics::{initial_positions_in_rect, Body, PhysicsWorld};
use crate::platform::input::{double_click_time_ms, InputHook, MouseButton, MouseEvent};
use crate::platform::launch::{LaunchResult, Launcher};
use crate::platform::menu::{prompt_rename_label, ContextAction, MenuHelper};
use crate::platform::tray::{Tray, TrayAction};
use crate::platform::wallpaper::{self, WallpaperImage};
use crate::platform::{self, Hwnd, ProcessMemory, SystemEvent};
use crate::render::atlas::{IconAtlas, SlotLocation};
use crate::render::{
    BubbleKind, BubbleRenderData, FrameData, LabelRenderData, OverlayRenderData, RenderError,
    Renderer, TooltipRenderData, WgpuRenderer, BUBBLE_FLAG_ERROR, BUBBLE_FLAG_HOVER,
};
use crate::scanner::{self, ScanEvent};
use crate::scoring;
use crate::store;
use crate::strings;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{Window, WindowId};

// ─── Tipe alias & konstanta ───────────────────────────────────────────────────

/// Waktu debounce simpan state ke disk (spec §8).
const SAVE_DEBOUNCE: Duration = Duration::from_secs(2);
/// Lama kursor diam di atas bubble sebelum tooltip muncul (spec §12).
const HOVER_TOOLTIP_DELAY: Duration = Duration::from_millis(400);
/// Pergeseran minimum piksel untuk membedakan drag dari klik (spec §12).
const DRAG_THRESHOLD_PX: f32 = 4.0;
/// Skala target saat hover dan konstanta pegasnya (spec §10 aturan 7).
const HOVER_TARGET_SCALE: f32 = 1.08;
const HOVER_SPRING_K: f32 = 180.0;
const HOVER_SPRING_C: f32 = 22.0;
/// Durasi dan amplitudo animasi pop saat peluncuran sukses (spec §9).
const POP_DURATION_SECS: f32 = 0.25;
const POP_AMPLITUDE: f32 = 0.15;
const POP_GLOW_MAX: f32 = 12.0;
const HOVER_GLOW: f32 = 5.0;
const ERROR_GLOW: f32 = 8.0;
/// Durasi dan amplitudo getaran saat peluncuran gagal (FR-3).
const SHAKE_DURATION_SECS: f32 = 0.35;
const SHAKE_AMPLITUDE_PX: f32 = 7.0;
const ERROR_TOOLTIP_DURATION: Duration = Duration::from_secs(3);
/// Jeda antar evaluasi pemicu jeda daya (spec §14).
const POWER_POLL_INTERVAL: Duration = Duration::from_secs(2);

// ─── State per-bubble ─────────────────────────────────────────────────────────

/// State runtime satu bubble (tidak disimpan ke disk).
struct BubbleState {
    /// Lokasi slot ikon di atlas (None = belum diekstrak).
    icon_slot: Option<SlotLocation>,
    /// Skala visual hover (1.0 normal, ~1.08 saat hover).
    visual_scale: f32,
    /// Kecepatan pegas skala hover.
    visual_scale_vel: f32,
    /// Waktu berjalan animasi pop (0..POP_DURATION_SECS).
    pop_elapsed: Option<f32>,
    /// Waktu berjalan animasi getar gagal buka (0..SHAKE_DURATION_SECS).
    shake_elapsed: Option<f32>,
    /// Pesan error peluncuran sementara beserta batas waktu tampilnya.
    error_msg: Option<(String, Instant)>,
}

impl BubbleState {
    fn new(icon_slot: Option<SlotLocation>) -> Self {
        Self {
            icon_slot,
            visual_scale: 1.0,
            visual_scale_vel: 0.0,
            pop_elapsed: None,
            shake_elapsed: None,
            error_msg: None,
        }
    }
}

/// State penekanan tombol mouse sementara untuk membedakan klik dan drag.
struct PressState {
    id: String,
    start_pos: [f32; 2],
    dragging: bool,
    ctrl: bool,
}

// ─── App ──────────────────────────────────────────────────────────────────────

pub fn run(config: Config, auto_exit_ms: Option<u64>) -> Result<(), winit::error::EventLoopError> {
    let event_loop = EventLoop::<SystemEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    if let Err(e) = platform::workerw::install_taskbar_listener(proxy.clone()) {
        log_warn!("gagal memasang listener TaskbarCreated: {e}");
    }
    let mut app = App::new(config, auto_exit_ms, proxy);
    event_loop.run_app(&mut app)
}

struct App {
    config: Config,
    event_proxy: EventLoopProxy<SystemEvent>,
    window: Option<Arc<Window>>,
    hwnd: Option<Hwnd>,
    renderer: Option<Box<dyn Renderer>>,

    // ─── Data & fisika ───────────────────────────────────────────────────────
    /// Item yang sedang ditampilkan (id → Item).
    items: HashMap<String, Item>,
    /// State runtime per bubble (id → BubbleState).
    bubble_states: HashMap<String, BubbleState>,
    /// Urutan id bubble (agar render deterministik).
    bubble_order: Vec<String>,
    /// Dunia fisika.
    world: PhysicsWorld,
    /// Atlas ikon (CPU).
    atlas: IconAtlas,
    /// GPU tekstur atlas per halaman (indeks halaman → wgpu::Texture handle-nya
    /// disimpan sebagai bool "sudah dibuat"; upload lewat Renderer::set_atlas_page).
    atlas_gpu_dirty: Vec<bool>,

    // ─── Watcher & channel ───────────────────────────────────────────────────
    scan_rx: Option<mpsc::Receiver<ScanEvent>>,
    icon_rx: Option<mpsc::Receiver<(String, Vec<u8>)>>,
    /// Simpan watcher agar tidak di-drop (drop = berhenti).
    _watcher: Option<notify::RecommendedWatcher>,

    // ─── Interaksi M4 ────────────────────────────────────────────────────────
    input_hook: Option<InputHook>,
    launcher: Option<Launcher>,
    menu_helper: Option<MenuHelper>,
    hovered_id: Option<String>,
    hover_since: Option<Instant>,
    tooltip_ready_drawn: bool,
    left_press: Option<PressState>,
    pending_double_click: Option<(String, Instant)>,
    visual_anim_active: bool,
    scratch_mouse_events: Vec<MouseEvent>,
    tooltip_data: Option<TooltipRenderData>,

    // ─── Integrasi sistem (M5) ───────────────────────────────────────────────
    tray: Option<Tray>,
    power: Option<platform::power::PowerMonitor>,
    /// Jeda manual dari menu tray.
    paused_user: bool,
    /// Jeda otomatis karena layar penuh/lock/layar mati.
    paused_power: bool,
    /// True bila sesi sebelumnya tidak bersih; lewati auto-sembunyikan ikon sesi ini.
    restored_stale_icons: bool,
    config_watcher: Option<notify::RecommendedWatcher>,
    last_power_poll: Instant,
    cached_on_battery: bool,

    // ─── Persist ─────────────────────────────────────────────────────────────
    app_data_dir: PathBuf,
    state_dirty: bool,
    last_state_change: Instant,

    // ─── Layar ───────────────────────────────────────────────────────────────
    screen_width: u32,
    screen_height: u32,
    monitor_origin: (i32, i32),
    work_min: [f32; 2],
    work_max: [f32; 2],

    // ─── Timing & metrik ─────────────────────────────────────────────────────
    auto_exit_ms: Option<u64>,
    started: Instant,
    last_frame_instant: Instant,
    last_fps: f32,
    last_frame_time_ms: f32,
    last_mem_sample: Instant,
    last_perf_log: Instant,
    cached_memory: ProcessMemory,
    overlay_active: bool,
    scratch_bubbles: Vec<BubbleRenderData>,
    scratch_labels: Vec<LabelRenderData>,
}

impl App {
    fn new(
        config: Config,
        auto_exit_ms: Option<u64>,
        event_proxy: EventLoopProxy<SystemEvent>,
    ) -> Self {
        let (screen_width, screen_height) = platform::workerw::primary_monitor_size();
        let monitor_origin = platform::workerw::primary_monitor_origin();
        let (work_min, work_max) = platform::workerw::primary_work_area_bounds();
        let overlay_active =
            config.debug.overlay || std::env::var_os("BUBBLEDESK_DEBUG_OVERLAY").is_some();
        let app_data_dir = platform::app_data_dir();
        let mut world = PhysicsWorld::new(screen_width as f32, screen_height as f32);
        world.set_work_bounds(work_min, work_max);

        Self {
            config,
            event_proxy,
            window: None,
            hwnd: None,
            renderer: None,
            items: HashMap::new(),
            bubble_states: HashMap::new(),
            bubble_order: Vec::new(),
            world,
            atlas: IconAtlas::new(),
            atlas_gpu_dirty: Vec::new(),
            scan_rx: None,
            icon_rx: None,
            _watcher: None,
            input_hook: None,
            launcher: None,
            menu_helper: None,
            hovered_id: None,
            hover_since: None,
            tooltip_ready_drawn: false,
            left_press: None,
            pending_double_click: None,
            visual_anim_active: false,
            scratch_mouse_events: Vec::with_capacity(32),
            tooltip_data: None,
            tray: None,
            power: None,
            paused_user: false,
            paused_power: false,
            restored_stale_icons: false,
            config_watcher: None,
            last_power_poll: Instant::now(),
            cached_on_battery: false,
            app_data_dir,
            state_dirty: false,
            last_state_change: Instant::now(),
            screen_width,
            screen_height,
            monitor_origin,
            work_min,
            work_max,
            auto_exit_ms,
            started: Instant::now(),
            last_frame_instant: Instant::now(),
            last_fps: 60.0,
            last_frame_time_ms: 16.6,
            last_mem_sample: Instant::now() - Duration::from_secs(10),
            last_perf_log: Instant::now(),
            cached_memory: ProcessMemory::default(),
            overlay_active,
            scratch_bubbles: Vec::new(),
            scratch_labels: Vec::new(),
        }
    }

    // ─── Attach & init ───────────────────────────────────────────────────────

    fn ensure_attached(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let attributes = Window::default_attributes()
                .with_title("BubbleDesk")
                .with_decorations(false)
                .with_resizable(false)
                .with_visible(false)
                .with_active(false)
                .with_skip_taskbar(true);
            match event_loop.create_window(attributes) {
                Ok(window) => {
                    let window = Arc::new(window);
                    self.hwnd = Hwnd::from_winit(&window);
                    self.window = Some(window);
                }
                Err(e) => {
                    log_error!("gagal membuat jendela: {e}");
                    event_loop.exit();
                    return;
                }
            }
        }
        let Some(hwnd) = self.hwnd else {
            log_error!("HWND tidak tersedia");
            return;
        };
        self.reparent(hwnd);
        self.init_renderer();
        self.init_system_integration();
        self.load_items();
        self.start_watcher();
        self.init_interaction();
    }

    /// Menyiapkan integrasi sistem M5: ikon tray, notifikasi daya, hot-reload config.
    fn init_system_integration(&mut self) {
        if self.tray.is_none() {
            match Tray::new(self.event_proxy.clone()) {
                Ok(t) => self.tray = Some(t),
                Err(e) => log_warn!("gagal membuat ikon tray: {e}"),
            }
        }
        if self.power.is_none() {
            match platform::power::install(self.event_proxy.clone()) {
                Ok(p) => self.power = Some(p),
                Err(e) => log_warn!("gagal memasang notifikasi daya: {e}"),
            }
        }
        if self.config_watcher.is_none() {
            self.start_config_watcher();
        }
        self.evaluate_power_pause();
        self.sync_tray_state();
    }

    fn init_interaction(&mut self) {
        if self.launcher.is_none() {
            self.launcher = Some(Launcher::start(self.event_proxy.clone()));
        }
        if self.menu_helper.is_none() {
            match MenuHelper::new() {
                Ok(h) => self.menu_helper = Some(h),
                Err(e) => log_warn!("gagal membuat MenuHelper: {e}"),
            }
        }
        if self.input_hook.is_none() {
            let rect = self.monitor_rect();
            match InputHook::start(self.event_proxy.clone(), self.hwnd, rect) {
                Ok(hook) => {
                    log_info!("mouse hook WH_MOUSE_LL aktif (monitor={rect:?})");
                    self.input_hook = Some(hook);
                }
                Err(e) => log_warn!("gagal memasang mouse hook: {e}"),
            }
        }
        self.apply_pause_state();
    }

    fn monitor_rect(&self) -> (i32, i32, i32, i32) {
        (
            self.monitor_origin.0,
            self.monitor_origin.1,
            self.monitor_origin.0 + self.screen_width as i32,
            self.monitor_origin.1 + self.screen_height as i32,
        )
    }

    fn refresh_monitor_metrics(&mut self) {
        let (w, h) = platform::workerw::primary_monitor_size();
        let origin = platform::workerw::primary_monitor_origin();
        let (work_min, work_max) = platform::workerw::primary_work_area_bounds();
        self.screen_width = w;
        self.screen_height = h;
        self.monitor_origin = origin;
        self.work_min = work_min;
        self.work_max = work_max;
        self.world.area_w = w as f32;
        self.world.area_h = h as f32;
        self.world.set_work_bounds(work_min, work_max);
        if let Some(hook) = &self.input_hook {
            hook.update_target(self.hwnd, self.monitor_rect());
        }
    }

    fn reparent(&mut self, hwnd: Hwnd) {
        let result = platform::workerw::attach(hwnd);
        if result.attached && !force_compat() {
            log_info!("attach ke desktop berhasil: {}", result.detail);
        } else {
            if result.attached {
                log_warn!("mode kompatibilitas dipaksa oleh BUBBLEDESK_FORCE_COMPAT");
            } else {
                log_warn!(
                    "attach gagal ({}); masuk mode kompatibilitas",
                    result.detail
                );
            }
            platform::workerw::attach_compat(hwnd);
        }
        platform::workerw::place_primary_monitor(hwnd);
        self.refresh_monitor_metrics();
    }

    fn init_renderer(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let (w, h) = platform::workerw::primary_monitor_size();
        self.screen_width = w;
        self.screen_height = h;

        match WgpuRenderer::new(window.clone(), w, h) {
            Ok(mut r) => {
                log_info!("WgpuRenderer diinisialisasi ({}x{})", w, h);
                let wp_path = wallpaper::current_wallpaper_path();
                let bg: WallpaperImage = match self.config.render.background {
                    Background::Wallpaper => wallpaper::load_wallpaper_or_default(
                        wp_path.as_deref(),
                        w,
                        h,
                        self.config.render.background_blur,
                    ),
                    Background::Gradient => wallpaper::generate_default_gradient(w, h),
                    Background::Solid => {
                        wallpaper::generate_solid_color(w, h, self.config.render.solid_color_ref())
                    }
                };
                r.set_background(&bg);
                drop(bg);
                self.renderer = Some(Box::new(r));
                if let Some(win) = &self.window {
                    win.request_redraw();
                }
            }
            Err(e) => log_error!("gagal inisialisasi WgpuRenderer: {e}"),
        }
    }

    // ─── Load items dari store & disk ────────────────────────────────────────

    fn load_items(&mut self) {
        // Pastikan direktori ada
        if let Err(e) = std::fs::create_dir_all(&self.app_data_dir) {
            log_warn!("gagal membuat direktori data: {e}");
        }

        // Muat state dari disk
        let mut state = store::load(&self.app_data_dir);
        log_info!("state dimuat: {} item tersimpan", state.items.len());

        // Jaminan pemulihan (spec §7.4 / AGENTS §5.2): bila sesi sebelumnya tidak
        // bersih, ikon desktop mungkin tertinggal tersembunyi. Pulihkan dan lewati
        // auto-sembunyikan agar pengguna tidak kehilangan ikon.
        if state.icons_hidden_by_us {
            log_warn!(
                "sesi sebelumnya tidak bersih: ikon desktop tertinggal tersembunyi; memulihkan"
            );
            let _ = platform::icons::set_native_icons(true);
            state.icons_hidden_by_us = false;
            self.restored_stale_icons = true;
            self.state_dirty = true;
            self.last_state_change = Instant::now();
        }

        // Scan desktop
        let disk_items = self.scan_desktop_now();
        log_info!("scan desktop: {} item ditemukan", disk_items.len());

        // Rekonsiliasi
        let reconciled = scanner::reconcile(state.items, disk_items);
        state.items = reconciled;

        // Simpan rekonsiliasi awal
        if let Err(e) = store::save(&self.app_data_dir, &state) {
            log_warn!("gagal menyimpan state awal: {e}");
        }

        // Bangun world fisika dari item
        self.rebuild_world(state.items);

        // Terapkan opsi sembunyikan ikon desktop bawaan (spec §7.4).
        self.apply_native_icons_setting();
    }

    fn scan_desktop_now(&self) -> Vec<Item> {
        let mut all = Vec::new();
        for src in &self.config.sources.include {
            let dir = match src.as_str() {
                "desktop" => scanner::user_desktop(),
                "public_desktop" => scanner::public_desktop(),
                _ => Some(PathBuf::from(src)),
            };
            if let Some(d) = dir {
                let mut items = scanner::scan_dir(&d, &self.config.sources);
                all.append(&mut items);
            }
        }
        all
    }

    /// Membangun ulang PhysicsWorld dan bubble_states dari daftar item.
    /// Item diurutkan: terbesar dulu (scoring §9 → radius).
    fn rebuild_world(&mut self, items: Vec<Item>) {
        let now_secs = unix_now();
        let cfg_bub = &self.config.bubbles;

        // Hitung radius tiap item aktif (abaikan yang tersembunyi atau hilang/dihapus)
        let mut id_radius: Vec<(String, f32)> = items
            .iter()
            .filter(|i| !i.hidden && !i.missing)
            .map(|i| {
                let r = scoring::score_to_radius(i.score, i.score_t0, now_secs, cfg_bub);
                (i.id.clone(), r)
            })
            .collect();

        // Urutkan terbesar dulu (spec §10 aturan 8)
        id_radius.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Skala adaptif agar kumpulan bubble memenuhi area kerja dan menyesuaikan jumlah item
        let radii: Vec<f32> = id_radius.iter().map(|(_, r)| *r).collect();
        let work_w = (self.work_max[0] - self.work_min[0]).max(1.0);
        let work_h = (self.work_max[1] - self.work_min[1]).max(1.0);
        let work_area = work_w as f64 * work_h as f64;
        let (k, max_display_r) = scoring::adaptive_scale_factor(
            &radii,
            work_area,
            work_w,
            work_h,
            cfg_bub.crowd_budget,
            cfg_bub.r_max,
        );

        // Posisi awal spiral (spec §10 aturan 8)
        let scaled_radii: Vec<f32> = radii.iter().map(|&r| (r * k).min(max_display_r)).collect();
        let positions = initial_positions_in_rect(
            &scaled_radii,
            self.work_min,
            self.work_max,
            cfg_bub.gap as f32,
        );

        // Simpan items ke HashMap
        self.items.clear();
        for item in items {
            self.items.insert(item.id.clone(), item);
        }

        // Buat bodies
        self.world = PhysicsWorld::new(self.screen_width as f32, self.screen_height as f32);
        self.world.set_work_bounds(self.work_min, self.work_max);
        self.bubble_states.clear();
        self.bubble_order.clear();

        for (idx, ((id, _r), pos)) in id_radius.iter().zip(positions.iter()).enumerate() {
            let r = scaled_radii[idx];
            let mut body = Body::new(*pos, r, id.clone());
            if let Some(norm) = self.items.get(id).and_then(|i| i.pinned_pos) {
                let pin_px = self
                    .world
                    .clamp_pos(denormalize_pos(norm, self.work_min, self.work_max), r);
                body.pos = pin_px;
                body.pinned = true;
                body.pinned_pos = Some(pin_px);
            }
            self.world.add_body(body);
            self.bubble_states
                .insert(id.clone(), BubbleState::new(None));
            self.bubble_order.push(id.clone());
        }
        self.world.wake();

        log_info!(
            "world dibangun: {} bubble, skala adaptif k={:.2}",
            self.bubble_order.len(),
            k
        );

        // Mulai ekstraksi ikon di background
        self.extract_icons_background();
    }

    /// Menghitung ulang `radius_target` semua bubble aktif agar ukuran seluruh bubble
    /// otomatis mengecil saat item bertambah dan membesar saat item berkurang.
    fn update_target_radii(&mut self) {
        if self.world.bodies.is_empty() {
            return;
        }
        let now_secs = unix_now();
        let cfg_bub = &self.config.bubbles;
        let base_radii: Vec<f32> = self
            .world
            .bodies
            .iter()
            .map(|b| {
                self.items
                    .get(&b.item_id)
                    .map(|i| scoring::score_to_radius(i.score, i.score_t0, now_secs, cfg_bub))
                    .unwrap_or(cfg_bub.r_min as f32)
            })
            .collect();

        let work_w = (self.work_max[0] - self.work_min[0]).max(1.0);
        let work_h = (self.work_max[1] - self.work_min[1]).max(1.0);
        let work_area = work_w as f64 * work_h as f64;
        let (k, max_display_r) = scoring::adaptive_scale_factor(
            &base_radii,
            work_area,
            work_w,
            work_h,
            cfg_bub.crowd_budget,
            cfg_bub.r_max,
        );

        for (body, &base_r) in self.world.bodies.iter_mut().zip(base_radii.iter()) {
            let target = (base_r * k).min(max_display_r);
            if (body.radius_target - target).abs() > 0.25 {
                body.radius_target = target;
                body.radius_stable_ms = 0.0;
            }
        }
        self.world.wake();
    }

    // ─── Ekstraksi ikon ───────────────────────────────────────────────────────

    /// Memulai worker thread yang mengekstrak ikon dan mengirim hasilnya
    /// kembali via channel, lalu membangunkan loop event lewat `EventLoopProxy`.
    fn extract_icons_background(&mut self) {
        // Kumpulkan (id, path) untuk semua bubble yang belum punya ikon
        let pending: Vec<(String, PathBuf, [f32; 4])> = self
            .bubble_order
            .iter()
            .filter(|id| {
                self.bubble_states
                    .get(*id)
                    .map(|s| s.icon_slot.is_none())
                    .unwrap_or(false)
            })
            .filter_map(|id| {
                let item = self.items.get(id)?;
                let kind = item.kind;
                Some((
                    id.clone(),
                    item.path.clone(),
                    item_kind_to_bubble_kind(kind).default_ring_color(),
                ))
            })
            .collect();

        if pending.is_empty() {
            return;
        }

        // Channel icon results: (id, rgba Vec<u8>)
        let (tx, rx) = mpsc::channel::<(String, Vec<u8>)>();
        let proxy = self.event_proxy.clone();

        std::thread::spawn(move || {
            #[cfg(windows)]
            {
                use crate::platform::icons::{extract_icon, placeholder_icon, ComGuard};
                let _com = match ComGuard::init() {
                    Ok(g) => g,
                    Err(e) => {
                        log_warn!("COM init gagal di icon worker: {e}");
                        return;
                    }
                };
                for (id, path, ring_color) in &pending {
                    let icon = match extract_icon(path) {
                        Ok(i) => i,
                        Err(e) => {
                            log_warn!("ekstraksi ikon gagal {}: {e}", path.display());
                            placeholder_icon(
                                path.file_stem().and_then(|s| s.to_str()).unwrap_or("?"),
                                *ring_color,
                            )
                        }
                    };
                    if tx.send((id.clone(), icon.rgba)).is_err() {
                        break; // receiver sudah drop
                    }
                    let _ = proxy.send_event(SystemEvent::WakeUp);
                }
            }
            #[cfg(not(windows))]
            {
                use crate::platform::icons::placeholder_icon;
                for (id, path, ring_color) in &pending {
                    let icon = placeholder_icon(
                        path.file_stem().and_then(|s| s.to_str()).unwrap_or("?"),
                        *ring_color,
                    );
                    if tx.send((id.clone(), icon.rgba)).is_err() {
                        break;
                    }
                    let _ = proxy.send_event(SystemEvent::WakeUp);
                }
            }
        });

        // Simpan receiver agar bisa di-poll
        self.icon_rx = Some(rx);
    }

    /// Poll icon extraction results dan masukkan ke atlas.
    fn poll_icon_results(&mut self) {
        // Ambil receiver sementara agar tidak borrow self dua kali
        let Some(rx) = self.icon_rx.take() else {
            return;
        };
        let mut keep = false;
        loop {
            match rx.try_recv() {
                Ok((id, rgba)) => {
                    if let Some(loc) = self.atlas.add_icon(&rgba) {
                        if let Some(bs) = self.bubble_states.get_mut(&id) {
                            bs.icon_slot = Some(loc);
                        }
                        // Tandai halaman GPU perlu update
                        while self.atlas_gpu_dirty.len() <= loc.page {
                            self.atlas_gpu_dirty.push(false);
                        }
                        self.atlas_gpu_dirty[loc.page] = true;
                    } else {
                        log_warn!("atlas penuh, ikon {} dibuang", id);
                    }
                    keep = true;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    keep = true;
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        if keep {
            self.icon_rx = Some(rx);
        }
    }

    // ─── Watcher ─────────────────────────────────────────────────────────────

    fn start_watcher(&mut self) {
        let mut dirs = Vec::new();
        for src in &self.config.sources.include {
            let dir = match src.as_str() {
                "desktop" => scanner::user_desktop(),
                "public_desktop" => scanner::public_desktop(),
                _ => Some(PathBuf::from(src)),
            };
            if let Some(d) = dir {
                dirs.push(d);
            }
        }
        let (tx, rx) = mpsc::sync_channel(64);
        let proxy = self.event_proxy.clone();
        match scanner::start_watcher(dirs, tx, move || {
            let _ = proxy.send_event(SystemEvent::WakeUp);
        }) {
            Some(w) => {
                log_info!("watcher file desktop aktif");
                self._watcher = Some(w);
                self.scan_rx = Some(rx);
            }
            None => {
                log_warn!("watcher file desktop tidak aktif");
            }
        }
    }

    /// Poll scan events dari watcher dan rekonsiliasi.
    fn poll_scan_events(&mut self) {
        let now = Instant::now();
        // Drain semua event yang pending
        let mut changed = false;
        // Ambil receiver sementara
        let Some(rx) = self.scan_rx.take() else {
            return;
        };
        loop {
            match rx.try_recv() {
                Ok(ev) => {
                    self.handle_scan_event(ev);
                    changed = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        self.scan_rx = Some(rx);

        if changed {
            self.state_dirty = true;
            self.last_state_change = now;
        }
    }

    fn handle_scan_event(&mut self, ev: ScanEvent) {
        match ev {
            ScanEvent::Added(path) => {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_owned();
                if name.is_empty() || scanner::should_ignore(&name, &self.config.sources) {
                    return;
                }
                if !path.exists() {
                    return;
                }
                let id = Item::make_id(&path);
                if self.bubble_states.contains_key(&id) {
                    return;
                }
                log_info!("file baru di desktop: {}", name);
                let now = unix_now();
                let kind = ItemKind::from_path(&path);
                let label = scanner::resolve_display_name(&path);
                let existing_score = self.items.get(&id).map(|i| {
                    (
                        i.score,
                        i.score_t0,
                        i.total_launches,
                        i.last_launch,
                        i.first_seen,
                        i.pinned_pos,
                    )
                });
                let (score, score_t0, total_launches, last_launch, first_seen, pinned_pos) =
                    existing_score.unwrap_or((0.0, now, 0, None, now, None));
                let base_r = scoring::score_to_radius(score, score_t0, now, &self.config.bubbles);
                let pos = [
                    self.screen_width as f32 * 0.5,
                    self.screen_height as f32 * 0.5,
                ];
                let mut body = Body::new(pos, (base_r * 0.5).max(16.0), id.clone());
                body.radius_target = base_r;
                if let Some(norm) = pinned_pos {
                    let pin_px = self
                        .world
                        .clamp_pos(denormalize_pos(norm, self.work_min, self.work_max), base_r);
                    body.pos = pin_px;
                    body.pinned = true;
                    body.pinned_pos = Some(pin_px);
                }
                self.world.add_body(body);
                self.bubble_states
                    .insert(id.clone(), BubbleState::new(None));
                self.bubble_order.push(id.clone());
                self.items.insert(
                    id,
                    Item {
                        id: Item::make_id(&path),
                        path,
                        label,
                        kind,
                        score,
                        score_t0,
                        total_launches,
                        last_launch,
                        first_seen,
                        pinned_pos,
                        hidden: false,
                        missing: false,
                        missing_since: None,
                    },
                );
                self.update_target_radii();
                self.extract_icons_background();
            }
            ScanEvent::Removed(path) => {
                let id = Item::make_id(&path);
                if self.bubble_states.remove(&id).is_none() {
                    return;
                }
                log_info!(
                    "file dihapus dari desktop: {}",
                    path.file_name().and_then(|n| n.to_str()).unwrap_or("?")
                );
                if let Some(item) = self.items.get_mut(&id) {
                    item.missing = true;
                    item.missing_since = Some(unix_now());
                }
                if self.hovered_id.as_deref() == Some(id.as_str()) {
                    self.hovered_id = None;
                    self.hover_since = None;
                    self.tooltip_ready_drawn = false;
                }
                if self.left_press.as_ref().is_some_and(|p| p.id == id) {
                    self.left_press = None;
                }
                if self
                    .pending_double_click
                    .as_ref()
                    .is_some_and(|(pid, _)| pid == &id)
                {
                    self.pending_double_click = None;
                }
                self.world.remove_body(&id);
                self.bubble_order.retain(|i| i != &id);
                self.update_target_radii();
            }
            ScanEvent::Renamed { from, to } => {
                let old_id = Item::make_id(&from);
                let new_id = Item::make_id(&to);
                log_info!("rename: {} → {}", from.display(), to.display());
                let new_name = to.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if new_name.is_empty() || scanner::should_ignore(new_name, &self.config.sources) {
                    self.handle_scan_event(ScanEvent::Removed(from));
                    return;
                }
                if let Some(mut item) = self.items.remove(&old_id) {
                    item.id = new_id.clone();
                    item.kind = ItemKind::from_path(&to);
                    item.label = scanner::resolve_display_name(&to);
                    item.path = to;
                    item.missing = false;
                    item.missing_since = None;
                    self.items.insert(new_id.clone(), item);
                    if let Some(bs) = self.bubble_states.remove(&old_id) {
                        self.bubble_states.insert(new_id.clone(), bs);
                        if let Some(b) = self.world.bodies.iter_mut().find(|b| b.item_id == old_id)
                        {
                            b.item_id = new_id.clone();
                        }
                    }
                    if let Some(o) = self.bubble_order.iter_mut().find(|i| *i == &old_id) {
                        *o = new_id;
                    }
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                } else {
                    self.handle_scan_event(ScanEvent::Added(to));
                }
            }
        }
    }

    // ─── Interaksi M4 ─────────────────────────────────────────────────────────

    fn request_redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// Mencari bubble teratas pada koordinat lokal `(x, y)`.
    /// Jika bertumpuk, memilih bubble dengan rasio `jarak / radius` terkecil (spec §12).
    fn hit_test(&self, x: f32, y: f32) -> Option<String> {
        let min_hit_r = self.config.bubbles.min_hit_radius as f32;
        let mut best: Option<(f32, &str)> = None;
        for body in &self.world.bodies {
            let scale = self
                .bubble_states
                .get(&body.item_id)
                .map(|s| s.visual_scale)
                .unwrap_or(1.0);
            let hit_r = (body.radius * scale).max(min_hit_r);
            if hit_r <= 0.0 {
                continue;
            }
            let dx = x - body.pos[0];
            let dy = y - body.pos[1];
            let dist = dx.hypot(dy);
            if dist <= hit_r {
                let ratio = dist / hit_r;
                if best.is_none_or(|(best_r, _)| ratio < best_r) {
                    best = Some((ratio, body.item_id.as_str()));
                }
            }
        }
        best.map(|(_, id)| id.to_owned())
    }

    fn poll_input_events(&mut self) {
        let mut events = std::mem::take(&mut self.scratch_mouse_events);
        events.clear();
        if let Some(hook) = &self.input_hook {
            while let Ok(ev) = hook.rx.try_recv() {
                events.push(ev);
            }
        }
        for ev in events.drain(..) {
            self.handle_mouse_event(ev);
        }
        self.scratch_mouse_events = events;
    }

    fn handle_mouse_event(&mut self, ev: MouseEvent) {
        match ev {
            MouseEvent::Leave => {
                if self.hovered_id.is_some() {
                    self.hovered_id = None;
                    self.hover_since = None;
                    self.tooltip_ready_drawn = false;
                    self.request_redraw();
                }
            }
            MouseEvent::Move { x, y } => {
                if let Some(ref mut lp) = self.left_press {
                    if !lp.dragging {
                        let dist = (x - lp.start_pos[0]).hypot(y - lp.start_pos[1]);
                        if dist >= DRAG_THRESHOLD_PX {
                            lp.dragging = true;
                            let drag_id = lp.id.clone();
                            self.hovered_id = Some(drag_id.clone());
                            self.hover_since = None;
                            self.tooltip_ready_drawn = false;
                            self.world.set_dragging(&drag_id, [x, y]);
                            self.request_redraw();
                        }
                    } else {
                        let drag_id = lp.id.clone();
                        self.world.set_dragging(&drag_id, [x, y]);
                        self.request_redraw();
                    }
                    return;
                }

                let hit = self.hit_test(x, y);
                if hit != self.hovered_id {
                    self.hovered_id = hit.clone();
                    self.hover_since = hit.map(|_| Instant::now());
                    self.tooltip_ready_drawn = false;
                    self.request_redraw();
                }
            }
            MouseEvent::Down {
                button: MouseButton::Left,
                x,
                y,
                ctrl,
            } => {
                let hit = self.hit_test(x, y);
                if hit != self.hovered_id {
                    self.hovered_id = hit.clone();
                    self.hover_since = hit.as_ref().map(|_| Instant::now());
                    self.tooltip_ready_drawn = false;
                    self.request_redraw();
                }
                if let Some(id) = hit {
                    self.left_press = Some(PressState {
                        id,
                        start_pos: [x, y],
                        dragging: false,
                        ctrl,
                    });
                }
            }
            MouseEvent::Up {
                button: MouseButton::Left,
                x,
                y,
                ctrl,
            } => {
                if let Some(lp) = self.left_press.take() {
                    if lp.dragging {
                        if let Some(final_px) = self.world.end_drag(&lp.id) {
                            if let Some(item) = self.items.get_mut(&lp.id) {
                                item.pinned_pos =
                                    Some(normalize_pos(final_px, self.work_min, self.work_max));
                            }
                            self.state_dirty = true;
                            self.last_state_change = Instant::now();
                        }
                        let hit = self.hit_test(x, y);
                        self.hovered_id = hit.clone();
                        self.hover_since = hit.map(|_| Instant::now());
                        self.tooltip_ready_drawn = false;
                        self.request_redraw();
                    } else {
                        let dist = (x - lp.start_pos[0]).hypot(y - lp.start_pos[1]);
                        if dist < DRAG_THRESHOLD_PX
                            && self.hit_test(x, y).as_deref() == Some(lp.id.as_str())
                        {
                            if lp.ctrl || ctrl {
                                self.handle_context_menu(&lp.id, x, y);
                            } else {
                                self.handle_bubble_click(&lp.id);
                            }
                        }
                    }
                }
            }
        }
    }

    fn handle_bubble_click(&mut self, id: &str) {
        match self.config.general.click_mode {
            ClickMode::Single => self.launch_item(id),
            ClickMode::Double => {
                let threshold = Duration::from_millis(u64::from(double_click_time_ms()));
                let is_second = match self.pending_double_click.take() {
                    Some((prev_id, prev_time)) => prev_id == id && prev_time.elapsed() <= threshold,
                    None => false,
                };
                if is_second {
                    self.launch_item(id);
                } else {
                    self.pending_double_click = Some((id.to_owned(), Instant::now()));
                }
            }
        }
    }

    fn launch_item(&mut self, id: &str) {
        let Some(path) = self.items.get(id).map(|i| i.path.clone()) else {
            return;
        };
        if let Some(bs) = self.bubble_states.get_mut(id) {
            bs.error_msg = None;
        }
        if let Some(launcher) = &self.launcher {
            launcher.launch(id.to_owned(), path);
        }
    }

    fn poll_launch_results(&mut self) {
        let Some(launcher) = self.launcher.take() else {
            return;
        };
        let mut results = Vec::new();
        while let Ok(res) = launcher.rx.try_recv() {
            results.push(res);
        }
        self.launcher = Some(launcher);
        for res in results {
            self.apply_launch_result(res);
        }
    }

    fn apply_launch_result(&mut self, res: LaunchResult) {
        if res.ok {
            let now = unix_now();
            if let Some(item) = self.items.get_mut(&res.id) {
                let (new_score, new_t0) = scoring::add_launch(
                    item.score,
                    item.score_t0,
                    now,
                    self.config.bubbles.score_half_life_days,
                );
                item.score = new_score;
                item.score_t0 = new_t0;
                item.total_launches = item.total_launches.saturating_add(1);
                item.last_launch = Some(now);
            }
            if let Some(bs) = self.bubble_states.get_mut(&res.id) {
                bs.pop_elapsed = Some(0.0);
                bs.error_msg = None;
            }
            self.visual_anim_active = true;
            self.update_target_radii();
            self.state_dirty = true;
            self.last_state_change = Instant::now();
            self.request_redraw();
        } else if !res.cancelled {
            let msg = res
                .error_msg
                .unwrap_or_else(|| strings::ERR_FILE_NOT_FOUND.to_owned());
            if let Some(bs) = self.bubble_states.get_mut(&res.id) {
                bs.shake_elapsed = Some(0.0);
                bs.error_msg = Some((msg, Instant::now() + ERROR_TOOLTIP_DURATION));
            }
            self.visual_anim_active = true;
            self.request_redraw();
        }
    }

    fn handle_context_menu(&mut self, id: &str, local_x: f32, local_y: f32) {
        let is_pinned = self.items.get(id).and_then(|i| i.pinned_pos).is_some()
            || self
                .world
                .bodies
                .iter()
                .any(|b| b.item_id == id && b.pinned);
        let screen_x = self.monitor_origin.0 + local_x.round() as i32;
        let screen_y = self.monitor_origin.1 + local_y.round() as i32;

        let action = self
            .menu_helper
            .as_ref()
            .and_then(|mh| mh.show_context_menu(screen_x, screen_y, is_pinned));

        match action {
            Some(ContextAction::Open) => self.launch_item(id),
            Some(ContextAction::OpenLocation) => {
                if let Some(item) = self.items.get(id) {
                    if let Err(e) = platform::launch::open_file_location(&item.path) {
                        log_warn!("buka lokasi file gagal: {e}");
                    }
                }
            }
            Some(ContextAction::CopyPath) => {
                if let Some(item) = self.items.get(id) {
                    let owner = self.menu_helper.as_ref().map(|m| m.hwnd());
                    let text = item.path.to_string_lossy();
                    if let Err(e) = platform::launch::copy_text_to_clipboard(owner, &text) {
                        log_warn!("salin path gagal: {e}");
                    }
                }
            }
            Some(ContextAction::RenameLabel) => {
                if let Some((current, old_path)) = self
                    .items
                    .get(id)
                    .map(|i| (i.display_name().to_owned(), i.path.clone()))
                {
                    let owner = self.menu_helper.as_ref().map(|m| m.hwnd());
                    if let Some(Some(new_name_raw)) =
                        prompt_rename_label(owner, &current, screen_x, screen_y)
                    {
                        self.rename_desktop_item(id, &old_path, &new_name_raw);
                    }
                }
            }
            Some(ContextAction::ResetScore) => {
                if let Some(item) = self.items.get_mut(id) {
                    item.score = 0.0;
                    item.score_t0 = unix_now();
                    item.total_launches = 0;
                    item.last_launch = None;
                }
                self.update_target_radii();
                self.state_dirty = true;
                self.last_state_change = Instant::now();
                self.request_redraw();
            }
            Some(ContextAction::Unpin) => {
                if let Some(item) = self.items.get_mut(id) {
                    item.pinned_pos = None;
                }
                self.world.unpin(id);
                self.state_dirty = true;
                self.last_state_change = Instant::now();
                self.request_redraw();
            }
            Some(ContextAction::Hide) => {
                if let Some(item) = self.items.get_mut(id) {
                    item.hidden = true;
                }
                if self.hovered_id.as_deref() == Some(id) {
                    self.hovered_id = None;
                    self.hover_since = None;
                    self.tooltip_ready_drawn = false;
                }
                if self.left_press.as_ref().is_some_and(|p| p.id == id) {
                    self.left_press = None;
                }
                if self
                    .pending_double_click
                    .as_ref()
                    .is_some_and(|(pid, _)| pid == id)
                {
                    self.pending_double_click = None;
                }
                self.bubble_states.remove(id);
                self.bubble_order.retain(|i| i != id);
                self.world.remove_body(id);
                self.update_target_radii();
                self.state_dirty = true;
                self.last_state_change = Instant::now();
                self.request_redraw();
            }
            None => {}
        }

        // Loop modal TrackPopupMenu / dialog memblokir thread UI: reset timestamp frame
        // agar delta waktu frame berikutnya tidak melonjak.
        self.last_frame_instant = Instant::now();
    }

    fn rename_desktop_item(&mut self, id: &str, old_path: &std::path::Path, new_name_raw: &str) {
        let new_path = match compute_rename_path(old_path, new_name_raw) {
            Ok(p) => p,
            Err(RenameError::InvalidName) => {
                self.trigger_bubble_error(id, strings::ERR_RENAME_INVALID_NAME.to_owned());
                return;
            }
            Err(RenameError::Unchanged) => return,
            Err(RenameError::NoParent) => return,
        };

        let new_id = Item::make_id(&new_path);
        if new_id != id && new_path.exists() {
            self.trigger_bubble_error(id, strings::ERR_RENAME_EXISTS.to_owned());
            return;
        }

        if let Err(e) = std::fs::rename(old_path, &new_path) {
            log_warn!(
                "gagal mengganti nama {} → {}: {e}",
                old_path.display(),
                new_path.display()
            );
            self.trigger_bubble_error(id, strings::format_rename_failed(&e.to_string()));
            return;
        }

        let resolved_label = scanner::resolve_display_name(&new_path);
        if let Some(mut item) = self.items.remove(id) {
            item.id = new_id.clone();
            item.kind = ItemKind::from_path(&new_path);
            item.label = resolved_label;
            item.path = new_path;
            self.items.insert(new_id.clone(), item);
        }
        if let Some(bs) = self.bubble_states.remove(id) {
            self.bubble_states.insert(new_id.clone(), bs);
        }
        if let Some(b) = self.world.bodies.iter_mut().find(|b| b.item_id == id) {
            b.item_id = new_id.clone();
        }
        if let Some(o) = self.bubble_order.iter_mut().find(|i| *i == id) {
            *o = new_id.clone();
        }
        if self.hovered_id.as_deref() == Some(id) {
            self.hovered_id = Some(new_id.clone());
        }
        if let Some((pid, t)) = &self.pending_double_click {
            if pid == id {
                self.pending_double_click = Some((new_id, *t));
            }
        }

        self.state_dirty = true;
        self.last_state_change = Instant::now();
        self.request_redraw();
    }

    fn trigger_bubble_error(&mut self, id: &str, msg: String) {
        if let Some(bs) = self.bubble_states.get_mut(id) {
            bs.shake_elapsed = Some(0.0);
            bs.error_msg = Some((msg, Instant::now() + ERROR_TOOLTIP_DURATION));
        }
        self.visual_anim_active = true;
        self.request_redraw();
    }

    fn paused(&self) -> bool {
        self.paused_user || self.paused_power
    }

    fn apply_pause_state(&mut self) {
        if let Some(hook) = &self.input_hook {
            hook.set_active(!self.paused());
        }
        self.sync_tray_state();
        if self.paused() {
            log_info!("render/input dijeda oleh status daya atau pengguna");
        } else {
            self.request_redraw();
        }
    }

    fn sync_tray_state(&self) {
        if let Some(tray) = &self.tray {
            tray.set_state(
                self.paused(),
                platform::icons::icons_hidden_flag(),
                self.overlay_active,
            );
        }
    }

    fn evaluate_power_pause(&mut self) {
        self.cached_on_battery = platform::power::on_battery();
        let paused = platform::power::is_paused(
            self.config.power.pause_on_fullscreen,
            self.config.power.pause_on_lock,
        );
        if paused != self.paused_power {
            self.paused_power = paused;
            self.apply_pause_state();
        }
    }

    fn apply_native_icons_setting(&mut self) {
        let visible = !self.config.general.hide_native_icons || self.restored_stale_icons;
        if let Err(e) = platform::icons::set_native_icons(visible) {
            log_warn!("gagal mengatur ikon desktop asli: {e}");
        }
        self.sync_tray_state();
    }

    fn handle_tray_action(&mut self, event_loop: &ActiveEventLoop, action: TrayAction) {
        match action {
            TrayAction::ToggleNativeIcons => {
                let visible = platform::icons::icons_hidden_flag();
                if let Err(e) = platform::icons::set_native_icons(visible) {
                    log_warn!("gagal mengubah visibilitas ikon desktop: {e}");
                } else {
                    self.state_dirty = true;
                    self.last_state_change = Instant::now();
                }
                self.sync_tray_state();
            }
            TrayAction::TogglePause => {
                self.paused_user = !self.paused_user;
                self.apply_pause_state();
            }
            TrayAction::ToggleOverlay => {
                self.overlay_active = !self.overlay_active;
                self.sync_tray_state();
                self.request_redraw();
            }
            TrayAction::OpenDataFolder => {
                if let Err(e) = platform::launch::open_folder(&self.app_data_dir) {
                    log_warn!("gagal membuka folder data: {e}");
                }
            }
            TrayAction::Quit => self.shutdown(event_loop),
        }
    }

    fn start_config_watcher(&mut self) {
        use notify::{RecursiveMode, Watcher};

        let proxy = self.event_proxy.clone();
        let watcher = notify::RecommendedWatcher::new(
            move |result: notify::Result<notify::Event>| {
                let Ok(event) = result else {
                    return;
                };
                if event.paths.iter().any(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.eq_ignore_ascii_case("config.toml"))
                }) {
                    let _ = proxy.send_event(SystemEvent::ConfigChanged);
                }
            },
            notify::Config::default().with_poll_interval(Duration::from_millis(300)),
        );
        let Ok(mut watcher) = watcher else {
            log_warn!("watcher config.toml tidak aktif");
            return;
        };
        if let Err(e) = watcher.watch(&self.app_data_dir, RecursiveMode::NonRecursive) {
            log_warn!("gagal memonitor config.toml: {e}");
            return;
        }
        self.config_watcher = Some(watcher);
    }

    fn reload_config(&mut self) {
        log_info!("konfigurasi dimuat ulang dari config.toml");
        let old_monitor = self.config.general.monitor.clone();
        let old_startup = self.config.general.start_with_windows;
        self.config = Config::load(&self.app_data_dir);
        self.overlay_active =
            self.config.debug.overlay || std::env::var_os("BUBBLEDESK_DEBUG_OVERLAY").is_some();
        self.update_target_radii();
        self.apply_native_icons_setting();
        self.evaluate_power_pause();
        if old_monitor != self.config.general.monitor
            || old_startup != self.config.general.start_with_windows
        {
            log_warn!("perubahan monitor/autostart berlaku setelah restart");
        }
        self.request_redraw();
    }

    fn shutdown(&mut self, event_loop: &ActiveEventLoop) {
        if platform::icons::icons_hidden_flag() {
            let _ = platform::icons::set_native_icons(true);
        }
        self.save_state_now();
        if let Some(renderer) = &mut self.renderer {
            renderer.shutdown();
        }
        event_loop.exit();
    }

    fn tick_timers_and_cleanup(&mut self) {
        let now = Instant::now();

        if let Some((_, t)) = &self.pending_double_click {
            let threshold = Duration::from_millis(u64::from(double_click_time_ms()));
            if now.duration_since(*t) > threshold {
                self.pending_double_click = None;
            }
        }

        let mut error_expired = false;
        for bs in self.bubble_states.values_mut() {
            if let Some((_, deadline)) = &bs.error_msg {
                if now >= *deadline {
                    bs.error_msg = None;
                    error_expired = true;
                }
            }
        }
        if error_expired {
            self.request_redraw();
        }

        if self.hovered_id.is_some()
            && !self.tooltip_ready_drawn
            && self
                .hover_since
                .is_some_and(|t| now.duration_since(t) >= HOVER_TOOLTIP_DELAY)
        {
            self.request_redraw();
        }
    }

    // ─── Persist ─────────────────────────────────────────────────────────────

    fn save_state_now(&mut self) {
        let items: Vec<Item> = self.items.values().cloned().collect();
        let state = store::State {
            schema: 1,
            items,
            icons_hidden_by_us: platform::icons::icons_hidden_flag(),
        };
        if let Err(e) = store::save(&self.app_data_dir, &state) {
            log_warn!("gagal menyimpan state: {e}");
        } else {
            self.state_dirty = false;
        }
    }

    fn maybe_save_state(&mut self) {
        if !self.state_dirty {
            return;
        }
        if Instant::now().duration_since(self.last_state_change) < SAVE_DEBOUNCE {
            return;
        }
        self.save_state_now();
    }

    // ─── Update & metrics ─────────────────────────────────────────────────────

    fn flush_atlas_to_gpu(&mut self) {
        if !self.atlas_gpu_dirty.iter().any(|&d| d) {
            return;
        }
        if let Some(renderer) = &mut self.renderer {
            self.atlas.flush_dirty(|page, pixels| {
                renderer.update_atlas(page, pixels);
            });
            self.atlas_gpu_dirty.fill(false);
        }
    }

    fn update(&mut self, dt: f32) {
        self.poll_input_events();
        self.poll_launch_results();
        self.tick_timers_and_cleanup();

        let was_sleeping = self.world.sleeping;
        self.world
            .step(dt, &self.config.bubbles, &self.config.physics);

        // Animasi visual per-bubble: pegas hover, pop peluncuran, getar gagal buka.
        let mut any_visual_anim = false;
        let hovered_ref = self.hovered_id.as_deref();
        let reduce_motion = self.config.general.reduce_motion;
        for (id, bs) in &mut self.bubble_states {
            let target_scale = if hovered_ref == Some(id.as_str()) {
                HOVER_TARGET_SCALE
            } else {
                1.0
            };
            if reduce_motion {
                bs.visual_scale = target_scale;
                bs.visual_scale_vel = 0.0;
            } else {
                let diff = bs.visual_scale - target_scale;
                if diff.abs() > 0.001 || bs.visual_scale_vel.abs() > 0.005 {
                    let accel = -HOVER_SPRING_K * diff - HOVER_SPRING_C * bs.visual_scale_vel;
                    bs.visual_scale_vel += accel * dt;
                    bs.visual_scale += bs.visual_scale_vel * dt;
                    any_visual_anim = true;
                } else {
                    bs.visual_scale = target_scale;
                    bs.visual_scale_vel = 0.0;
                }
            }

            if let Some(t) = bs.pop_elapsed {
                let next = t + dt;
                if next >= POP_DURATION_SECS {
                    bs.pop_elapsed = None;
                } else {
                    bs.pop_elapsed = Some(next);
                    any_visual_anim = true;
                }
            }

            if let Some(t) = bs.shake_elapsed {
                let next = t + dt;
                if next >= SHAKE_DURATION_SECS {
                    bs.shake_elapsed = None;
                } else {
                    bs.shake_elapsed = Some(next);
                    any_visual_anim = true;
                }
            }
        }
        self.visual_anim_active = any_visual_anim;

        self.poll_icon_results();
        self.flush_atlas_to_gpu();
        self.poll_scan_events();
        self.maybe_save_state();

        // Metrik
        let now = Instant::now();
        if now.duration_since(self.last_mem_sample) >= Duration::from_millis(500) {
            self.last_mem_sample = now;
            self.cached_memory = platform::current_process_memory();
        }
        let entered_sleep = !was_sleeping && self.world.sleeping;
        if entered_sleep || now.duration_since(self.last_perf_log) >= Duration::from_secs(2) {
            self.last_perf_log = now;
            let ws = self.cached_memory.working_set_bytes as f64 / (1024.0 * 1024.0);
            let pb = self.cached_memory.private_bytes as f64 / (1024.0 * 1024.0);
            log_info!(
                "perf: FPS={:.1} frame={:.2}ms WS={:.1}MB PB={:.1}MB bodies={} sleep={}",
                self.last_fps,
                self.last_frame_time_ms,
                ws,
                pb,
                self.world.bodies.len(),
                self.world.sleeping
            );
        }
    }

    // ─── Render data ──────────────────────────────────────────────────────────

    fn prepare_render_data(&mut self) {
        self.scratch_bubbles.clear();
        self.scratch_labels.clear();
        self.tooltip_data = None;

        let now_inst = Instant::now();
        let now_secs = unix_now();
        let reduce_motion = self.config.general.reduce_motion;
        let label_min_r = self.config.bubbles.label_always_min_radius as f32;

        // Pilih target tooltip: prioritas error peluncuran, lalu hover >= 400 ms.
        let mut tooltip_id: Option<String> = None;
        let mut tooltip_error_msg: Option<String> = None;

        if let Some(hid) = &self.hovered_id {
            if let Some(bs) = self.bubble_states.get(hid) {
                if let Some((msg, _)) = &bs.error_msg {
                    tooltip_id = Some(hid.clone());
                    tooltip_error_msg = Some(msg.clone());
                }
            }
        }
        if tooltip_id.is_none() {
            for body in &self.world.bodies {
                if let Some(bs) = self.bubble_states.get(&body.item_id) {
                    if let Some((msg, _)) = &bs.error_msg {
                        tooltip_id = Some(body.item_id.clone());
                        tooltip_error_msg = Some(msg.clone());
                        break;
                    }
                }
            }
        }
        if tooltip_id.is_none() {
            if let Some(hid) = &self.hovered_id {
                if self
                    .hover_since
                    .is_some_and(|t| now_inst.duration_since(t) >= HOVER_TOOLTIP_DELAY)
                {
                    tooltip_id = Some(hid.clone());
                }
            }
        }

        for body in &self.world.bodies {
            let Some(bs) = self.bubble_states.get(&body.item_id) else {
                continue;
            };
            let Some(item) = self.items.get(&body.item_id) else {
                continue;
            };
            let kind = item_kind_to_bubble_kind(item.kind);
            let (icon_uv, icon_page) = match bs.icon_slot {
                Some(loc) => {
                    let r = loc.uv_rect();
                    ([r.u_min, r.v_min, r.u_max, r.v_max], loc.page as i32)
                }
                None => ([0.0; 4], -1),
            };

            let (pop_scale, pop_glow) = match bs.pop_elapsed {
                Some(t) if !reduce_motion => {
                    let p = (t / POP_DURATION_SECS).clamp(0.0, 1.0);
                    let wave = (p * std::f32::consts::PI).sin();
                    (1.0 + POP_AMPLITUDE * wave, POP_GLOW_MAX * wave)
                }
                _ => (1.0, 0.0),
            };
            let shake_x = match bs.shake_elapsed {
                Some(t) if !reduce_motion => {
                    let p = (t / SHAKE_DURATION_SECS).clamp(0.0, 1.0);
                    let decay = 1.0 - p;
                    let osc = (p * 4.0 * std::f32::consts::TAU).sin();
                    SHAKE_AMPLITUDE_PX * decay * osc
                }
                _ => 0.0,
            };

            let is_hovered = self.hovered_id.as_deref() == Some(body.item_id.as_str());
            let has_error = bs.error_msg.is_some();
            let mut flags = 0u32;
            if is_hovered {
                flags |= BUBBLE_FLAG_HOVER;
            }
            if has_error {
                flags |= BUBBLE_FLAG_ERROR;
            }
            let glow = pop_glow.max(if has_error {
                ERROR_GLOW
            } else if is_hovered {
                HOVER_GLOW
            } else {
                0.0
            });
            let effective_scale = bs.visual_scale * pop_scale;
            let center = [body.pos[0] + shake_x, body.pos[1]];

            self.scratch_bubbles.push(BubbleRenderData {
                center,
                radius: body.radius,
                visual_scale: effective_scale,
                ring_color: kind.default_ring_color(),
                ring_width: (body.radius * 0.04).clamp(2.5, 5.0),
                glow,
                flags,
                icon_uv,
                icon_page,
            });

            let is_tooltip_target = tooltip_id.as_deref() == Some(body.item_id.as_str());
            if is_tooltip_target {
                let is_err = tooltip_error_msg.is_some();
                let subtitle = tooltip_error_msg.clone().unwrap_or_else(|| {
                    strings::format_tooltip_stats(item.total_launches, item.last_launch, now_secs)
                });
                self.tooltip_data = Some(TooltipRenderData {
                    anchor_x: center[0],
                    bubble_center_y: center[1],
                    bubble_radius: body.radius * effective_scale,
                    title: item.display_name().to_owned(),
                    subtitle,
                    detail: item.path.to_string_lossy().into_owned(),
                    is_error: is_err,
                });
                if !is_err {
                    self.tooltip_ready_drawn = true;
                }
            } else if is_hovered || body.radius >= label_min_r {
                self.scratch_labels.push(LabelRenderData {
                    anchor: [center[0], center[1] + body.radius * effective_scale],
                    text: item.display_name().to_owned(),
                    max_width: (body.radius * 2.6).max(110.0),
                });
            }
        }
    }
}

// ─── Helper ───────────────────────────────────────────────────────────────────

/// Alasan `compute_rename_path` menolak permintaan ganti nama.
#[derive(Debug, PartialEq, Eq)]
enum RenameError {
    /// Nama kosong atau mengandung karakter terlarang Windows.
    InvalidName,
    /// Nama hasil sama dengan nama lama (tidak ada perubahan).
    Unchanged,
    /// Path lama tidak punya direktori induk.
    NoParent,
}

/// Menghitung path tujuan ganti nama item desktop dari masukan mentah pengguna.
///
/// - Menolak nama kosong atau berisi `< > : " / \ | ? *`.
/// - Untuk berkas (bukan folder), mempertahankan ekstensi asli bila pengguna
///   tidak menuliskannya (mis. `laporan` → `laporan.pdf`, `shortcut` → `shortcut.lnk`).
fn compute_rename_path(
    old_path: &std::path::Path,
    new_name_raw: &str,
) -> Result<PathBuf, RenameError> {
    let trimmed = new_name_raw.trim().trim_end_matches('.');
    if trimmed.is_empty()
        || trimmed
            .chars()
            .any(|c| matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
    {
        return Err(RenameError::InvalidName);
    }

    let parent = old_path.parent().ok_or(RenameError::NoParent)?;

    let mut final_name = trimmed.to_owned();
    if !old_path.is_dir() {
        if let Some(orig_ext) = old_path.extension().and_then(|e| e.to_str()) {
            let suffix = format!(".{orig_ext}");
            if !final_name
                .to_ascii_lowercase()
                .ends_with(&suffix.to_ascii_lowercase())
            {
                final_name.push_str(&suffix);
            }
        }
    }

    let new_path = parent.join(&final_name);
    if new_path == old_path {
        return Err(RenameError::Unchanged);
    }
    Ok(new_path)
}

fn normalize_pos(px: [f32; 2], work_min: [f32; 2], work_max: [f32; 2]) -> [f32; 2] {
    let w = (work_max[0] - work_min[0]).max(1.0);
    let h = (work_max[1] - work_min[1]).max(1.0);
    [
        ((px[0] - work_min[0]) / w).clamp(0.0, 1.0),
        ((px[1] - work_min[1]) / h).clamp(0.0, 1.0),
    ]
}

fn denormalize_pos(norm: [f32; 2], work_min: [f32; 2], work_max: [f32; 2]) -> [f32; 2] {
    let w = (work_max[0] - work_min[0]).max(1.0);
    let h = (work_max[1] - work_min[1]).max(1.0);
    [
        work_min[0] + norm[0].clamp(0.0, 1.0) * w,
        work_min[1] + norm[1].clamp(0.0, 1.0) * h,
    ]
}

fn item_kind_to_bubble_kind(kind: ItemKind) -> BubbleKind {
    match kind {
        ItemKind::App => BubbleKind::App,
        ItemKind::Folder => BubbleKind::Folder,
        ItemKind::Document => BubbleKind::Document,
        ItemKind::Media => BubbleKind::Media,
        ItemKind::Other => BubbleKind::Other,
    }
}

fn force_compat() -> bool {
    std::env::var_os("BUBBLEDESK_FORCE_COMPAT").is_some()
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ─── ApplicationHandler ───────────────────────────────────────────────────────

impl ApplicationHandler<SystemEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.ensure_attached(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.shutdown(event_loop);
            }
            WindowEvent::RedrawRequested => {
                if self.paused() {
                    self.last_frame_instant = Instant::now();
                    return;
                }
                let now = Instant::now();
                let dt = now
                    .duration_since(self.last_frame_instant)
                    .as_secs_f32()
                    .min(0.1);
                self.last_frame_instant = now;

                // Hitung FPS (rolling)
                self.last_fps = if dt > 0.0 { 1.0 / dt } else { 60.0 };
                self.last_frame_time_ms = dt * 1000.0;

                self.update(dt);

                // Bangun data render sebelum borrow renderer
                self.prepare_render_data();
                let overlay = OverlayRenderData {
                    show: self.overlay_active,
                    fps: self.last_fps,
                    frame_time_ms: self.last_frame_time_ms,
                    working_set_bytes: self.cached_memory.working_set_bytes,
                    private_bytes: self.cached_memory.private_bytes,
                    body_count: self.world.bodies.len(),
                    is_asleep: self.world.sleeping,
                };
                let frame = FrameData {
                    screen_width: self.screen_width,
                    screen_height: self.screen_height,
                    background_dim: self.config.render.background_dim as f32,
                    time: self.started.elapsed().as_secs_f32(),
                    bubbles: &self.scratch_bubbles,
                    labels: &self.scratch_labels,
                    tooltip: self.tooltip_data.as_ref(),
                    overlay,
                };

                if let Some(renderer) = &mut self.renderer {
                    match renderer.draw(&frame) {
                        Ok(()) => {}
                        Err(RenderError::SurfaceLost) => {
                            log_warn!("Surface lost, reconfigure...");
                            renderer.resize(frame.screen_width, frame.screen_height);
                        }
                        Err(e) => log_error!("render error: {e}"),
                    }
                }

                // Lanjut render hanya saat fisika aktif atau animasi visual berjalan
                if !self.world.sleeping || self.visual_anim_active {
                    self.request_redraw();
                }
            }
            WindowEvent::Resized(size) => {
                self.screen_width = size.width.max(1);
                self.screen_height = size.height.max(1);
                self.monitor_origin = platform::workerw::primary_monitor_origin();
                let (work_min, work_max) = platform::workerw::primary_work_area_bounds();
                self.work_min = work_min;
                self.work_max = work_max;
                self.world.area_w = self.screen_width as f32;
                self.world.area_h = self.screen_height as f32;
                for body in &mut self.world.bodies {
                    if body.pinned {
                        if let Some(norm) = self.items.get(&body.item_id).and_then(|i| i.pinned_pos)
                        {
                            body.pinned_pos = Some(denormalize_pos(norm, work_min, work_max));
                        }
                    }
                }
                self.world.set_work_bounds(work_min, work_max);
                if let Some(hook) = &self.input_hook {
                    hook.update_target(self.hwnd, self.monitor_rect());
                }
                if let Some(r) = &mut self.renderer {
                    r.resize(self.screen_width, self.screen_height);
                }
                self.update_target_radii();
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(hwnd) = self.hwnd {
                    self.reparent(hwnd);
                }
            }
            _ => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: SystemEvent) {
        match event {
            SystemEvent::TaskbarCreated => {
                log_info!("TaskbarCreated: re-attach ke desktop");
                if let Some(tray) = &self.tray {
                    tray.recreate();
                }
                match self.hwnd {
                    Some(hwnd) if platform::workerw::is_alive(hwnd) => {
                        self.reparent(hwnd);
                    }
                    _ => {
                        log_warn!("jendela lama tidak hidup; membuat ulang");
                        self.window = None;
                        self.hwnd = None;
                        self.renderer = None;
                        self.ensure_attached(event_loop);
                    }
                }
            }
            SystemEvent::WakeUp => {
                // Bangun dari ControlFlow::Wait; `about_to_wait` akan mem-poll channel.
            }
            SystemEvent::Tray(action) => self.handle_tray_action(event_loop, action),
            SystemEvent::PowerChanged => self.evaluate_power_pause(),
            SystemEvent::ConfigChanged => self.reload_config(),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Poll input, peluncuran, ikon & watcher bahkan saat fisika sedang tidur
        self.poll_input_events();
        self.poll_launch_results();
        self.poll_icon_results();
        self.poll_scan_events();
        self.tick_timers_and_cleanup();
        self.maybe_save_state();

        if Instant::now().duration_since(self.last_power_poll) >= POWER_POLL_INTERVAL {
            self.last_power_poll = Instant::now();
            self.evaluate_power_pause();
        }

        let has_dirty_atlas = self.atlas_gpu_dirty.iter().any(|&d| d);
        let needs_render =
            !self.paused() && (!self.world.sleeping || self.visual_anim_active || has_dirty_atlas);

        // Gambar ulang hanya bila render tidak sedang dijeda.
        if needs_render {
            self.request_redraw();
        }

        match self.auto_exit_ms {
            Some(ms) => {
                let deadline = self.started + Duration::from_millis(ms);
                if Instant::now() >= deadline {
                    self.shutdown(event_loop);
                } else {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
                }
            }
            None => {
                if !needs_render {
                    let mut earliest: Option<Instant> = None;
                    let mut push_deadline = |d: Instant| {
                        earliest = Some(match earliest {
                            Some(cur) if cur <= d => cur,
                            _ => d,
                        });
                    };

                    if self.state_dirty {
                        push_deadline(self.last_state_change + SAVE_DEBOUNCE);
                    }
                    push_deadline(self.last_power_poll + POWER_POLL_INTERVAL);
                    if self.hovered_id.is_some() && !self.tooltip_ready_drawn {
                        if let Some(t) = self.hover_since {
                            push_deadline(t + HOVER_TOOLTIP_DELAY);
                        }
                    }
                    for bs in self.bubble_states.values() {
                        if let Some((_, deadline)) = &bs.error_msg {
                            push_deadline(*deadline);
                        }
                    }

                    if let Some(d) = earliest {
                        event_loop.set_control_flow(ControlFlow::WaitUntil(d));
                    } else {
                        // Event-driven: tunggu hingga ada event (CPU ≈ 0)
                        event_loop.set_control_flow(ControlFlow::Wait);
                    }
                } else {
                    let fps_cap = if self.cached_on_battery {
                        self.config.render.fps_cap_on_battery
                    } else {
                        self.config.render.fps_cap
                    };
                    let fps = fps_cap.max(1) as u64;
                    let frame_interval = Duration::from_micros(1_000_000 / fps);
                    let next_frame = self.last_frame_instant + frame_interval;
                    event_loop.set_control_flow(ControlFlow::WaitUntil(next_frame));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_dan_denormalize_roundtrip() {
        let wmin = [0.0, 40.0];
        let wmax = [1600.0, 860.0];
        let px = [480.0, 450.0];
        let norm = normalize_pos(px, wmin, wmax);
        assert!((norm[0] - 0.3).abs() < 1e-5);
        assert!((norm[1] - 0.5).abs() < 1e-5);
        let back = denormalize_pos(norm, wmin, wmax);
        assert!((back[0] - px[0]).abs() < 1e-3);
        assert!((back[1] - px[1]).abs() < 1e-3);
    }

    #[test]
    fn rename_pertahankan_ekstensi_dan_tolak_nama_tidak_valid() {
        let dir = std::env::temp_dir();
        let file = dir.join("laporan.pdf");
        let folder = dir.join("proyek");

        assert_eq!(
            compute_rename_path(&file, "ringkasan").ok(),
            Some(dir.join("ringkasan.pdf"))
        );
        assert_eq!(
            compute_rename_path(&file, "ringkasan.PDF").ok(),
            Some(dir.join("ringkasan.PDF"))
        );
        assert_eq!(
            compute_rename_path(&folder, "arsip").ok(),
            Some(dir.join("arsip"))
        );
        assert_eq!(
            compute_rename_path(&file, "  "),
            Err(RenameError::InvalidName)
        );
        assert_eq!(
            compute_rename_path(&file, "a/b"),
            Err(RenameError::InvalidName)
        );
        assert_eq!(
            compute_rename_path(&file, "laporan.pdf"),
            Err(RenameError::Unchanged)
        );
    }
}
