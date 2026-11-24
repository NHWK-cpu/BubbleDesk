//! Hook mouse global `WH_MOUSE_LL` di thread khusus (spec §6, §7.5, §12).
//!
//! Aturan keselamatan (`AGENTS.md` §5.3):
//! - Hook proc HARUS selalu memanggil `CallNextHookEx` dan kembali secepat mungkin.
//! - Jangan pernah menelan input pengguna.
//! - Hanya meneruskan event jika kursor di atas desktop kosong dan aplikasi tidak dijeda.

#![cfg(windows)]

use crate::platform::{Hwnd, SystemEvent};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::OnceLock;
use std::thread::JoinHandle;
use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetDoubleClickTime, VK_CONTROL,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetClassNameW, GetMessageW, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WindowFromPoint, HHOOK, MSG,
    MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_APP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_QUIT,
};
use winit::event_loop::EventLoopProxy;

const MSG_INSTALL_HOOK: u32 = WM_APP + 1;
const MSG_UNHOOK: u32 = WM_APP + 2;

/// Tombol mouse yang dipantau.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
}

/// Event mouse yang dikirim dari thread hook ke main thread.
#[derive(Debug, Clone, Copy)]
pub enum MouseEvent {
    Move {
        x: f32,
        y: f32,
    },
    Down {
        button: MouseButton,
        x: f32,
        y: f32,
        ctrl: bool,
    },
    Up {
        button: MouseButton,
        x: f32,
        y: f32,
        ctrl: bool,
    },
    /// Kursor keluar dari area desktop kosong menuju jendela lain atau luar monitor.
    Leave,
}

struct HookShared {
    tx: SyncSender<MouseEvent>,
    proxy: EventLoopProxy<SystemEvent>,
    enabled: AtomicBool,
    our_hwnd: AtomicIsize,
    mon_left: AtomicI32,
    mon_top: AtomicI32,
    mon_right: AtomicI32,
    mon_bottom: AtomicI32,
    left_pressed: AtomicBool,
    over_desktop: AtomicBool,
}

static HOOK_SHARED: OnceLock<HookShared> = OnceLock::new();

/// Mengembalikan waktu double-click sistem Windows (ms).
pub fn double_click_time_ms() -> u32 {
    // SAFETY: GetDoubleClickTime tidak mengambil pointer dan aman dipanggil kapan saja.
    let ms = unsafe { GetDoubleClickTime() };
    ms.max(150)
}

/// Mengelola thread `WH_MOUSE_LL` dan receiver event mouse.
pub struct InputHook {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
    pub rx: Receiver<MouseEvent>,
}

impl InputHook {
    /// Memulai thread khusus `WH_MOUSE_LL`.
    pub fn start(
        proxy: EventLoopProxy<SystemEvent>,
        our_hwnd: Option<Hwnd>,
        monitor_rect: (i32, i32, i32, i32),
    ) -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel::<MouseEvent>(256);
        let shared = HOOK_SHARED.get_or_init(|| HookShared {
            tx: tx.clone(),
            proxy: proxy.clone(),
            enabled: AtomicBool::new(true),
            our_hwnd: AtomicIsize::new(our_hwnd.map(|h| h.0).unwrap_or(0)),
            mon_left: AtomicI32::new(monitor_rect.0),
            mon_top: AtomicI32::new(monitor_rect.1),
            mon_right: AtomicI32::new(monitor_rect.2),
            mon_bottom: AtomicI32::new(monitor_rect.3),
            left_pressed: AtomicBool::new(false),
            over_desktop: AtomicBool::new(false),
        });

        shared.enabled.store(true, Ordering::Release);
        shared
            .our_hwnd
            .store(our_hwnd.map(|h| h.0).unwrap_or(0), Ordering::Release);
        shared.mon_left.store(monitor_rect.0, Ordering::Release);
        shared.mon_top.store(monitor_rect.1, Ordering::Release);
        shared.mon_right.store(monitor_rect.2, Ordering::Release);
        shared.mon_bottom.store(monitor_rect.3, Ordering::Release);

        let (tid_tx, tid_rx) = mpsc::sync_channel::<Result<u32, String>>(1);

        let join = std::thread::Builder::new()
            .name("BubbleDeskInput".to_owned())
            .spawn(move || input_thread_main(tid_tx))
            .map_err(|e| format!("gagal membuat thread input: {e}"))?;

        let thread_id = tid_rx
            .recv()
            .map_err(|_| "thread input berhenti sebelum siap".to_owned())??;

        Ok(Self {
            thread_id,
            join: Some(join),
            rx,
        })
    }

    /// Memperbarui handle jendela BubbleDesk dan batas monitor target.
    pub fn update_target(&self, our_hwnd: Option<Hwnd>, monitor_rect: (i32, i32, i32, i32)) {
        if let Some(shared) = HOOK_SHARED.get() {
            shared
                .our_hwnd
                .store(our_hwnd.map(|h| h.0).unwrap_or(0), Ordering::Release);
            shared.mon_left.store(monitor_rect.0, Ordering::Release);
            shared.mon_top.store(monitor_rect.1, Ordering::Release);
            shared.mon_right.store(monitor_rect.2, Ordering::Release);
            shared.mon_bottom.store(monitor_rect.3, Ordering::Release);
        }
    }

    /// Mengaktifkan atau menjeda hook mouse (melepas `WH_MOUSE_LL` saat paused, spec §14).
    #[allow(dead_code)]
    pub fn set_active(&self, active: bool) {
        if let Some(shared) = HOOK_SHARED.get() {
            shared.enabled.store(active, Ordering::Release);
            if !active {
                shared.left_pressed.store(false, Ordering::Release);
                shared.over_desktop.store(false, Ordering::Release);
            }
        }
        let msg = if active { MSG_INSTALL_HOOK } else { MSG_UNHOOK };
        // SAFETY: thread_id valid milik thread input yang berjalan dengan message queue.
        let _ = unsafe { PostThreadMessageW(self.thread_id, msg, WPARAM(0), LPARAM(0)) };
    }
}

impl Drop for InputHook {
    fn drop(&mut self) {
        // SAFETY: kirim WM_QUIT agar loop GetMessageW pada thread input keluar bersih.
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn input_thread_main(tid_tx: SyncSender<Result<u32, String>>) {
    // SAFETY: GetCurrentThreadId aman di thread ini.
    let tid = unsafe { GetCurrentThreadId() };

    // Pastikan message queue thread sudah terbentuk sebelum memasang hook & mengirim tid.
    let mut msg = MSG::default();
    let mut hook = match install_ll_hook() {
        Ok(h) => Some(h),
        Err(e) => {
            let _ = tid_tx.send(Err(e));
            return;
        }
    };

    let _ = tid_tx.send(Ok(tid));

    // SAFETY: loop pesan Win32 standar milik thread input khusus.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            match msg.message {
                MSG_INSTALL_HOOK => {
                    if hook.is_none() {
                        match install_ll_hook() {
                            Ok(h) => hook = Some(h),
                            Err(e) => log_warn!("gagal memasang ulang mouse hook: {e}"),
                        }
                    }
                }
                MSG_UNHOOK => {
                    if let Some(h) = hook.take() {
                        let _ = UnhookWindowsHookEx(h);
                    }
                }
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
        if let Some(h) = hook.take() {
            let _ = UnhookWindowsHookEx(h);
        }
    }
}

fn install_ll_hook() -> Result<HHOOK, String> {
    // SAFETY: mouse_hook_proc adalah callback `extern "system"` dengan signature Win32 tepat
    // dan selalu memanggil CallNextHookEx.
    unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), None, 0) }
        .map_err(|e| format!("SetWindowsHookExW(WH_MOUSE_LL) gagal: {e}"))
}

/// Callback `WH_MOUSE_LL`: tidak boleh memblokir atau mengalokasi heap.
unsafe extern "system" fn mouse_hook_proc(ncode: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if ncode >= 0 {
        if let Some(shared) = HOOK_SHARED.get() {
            if shared.enabled.load(Ordering::Relaxed) {
                // SAFETY: ketika ncode >= 0, lparam menunjuk ke MSLLHOOKSTRUCT yang valid.
                let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
                process_ll_mouse_event(shared, wparam.0 as u32, info.pt);
            }
        }
    }
    // SAFETY: AGENTS §5.3 — wajib selalu memanggil CallNextHookEx tanpa menelan input.
    unsafe { CallNextHookEx(None, ncode, wparam, lparam) }
}

fn process_ll_mouse_event(shared: &HookShared, msg: u32, pt: POINT) {
    let left = shared.mon_left.load(Ordering::Relaxed);
    let top = shared.mon_top.load(Ordering::Relaxed);
    let right = shared.mon_right.load(Ordering::Relaxed);
    let bottom = shared.mon_bottom.load(Ordering::Relaxed);

    let local_x = (pt.x - left) as f32;
    let local_y = (pt.y - top) as f32;
    let in_monitor = pt.x >= left && pt.x < right && pt.y >= top && pt.y < bottom;

    let ev = match msg {
        WM_LBUTTONUP => {
            if shared.left_pressed.swap(false, Ordering::AcqRel) {
                Some(MouseEvent::Up {
                    button: MouseButton::Left,
                    x: local_x,
                    y: local_y,
                    ctrl: is_ctrl_down(),
                })
            } else {
                None
            }
        }
        WM_LBUTTONDOWN => {
            if in_monitor && is_desktop_point(pt, shared.our_hwnd.load(Ordering::Relaxed)) {
                shared.left_pressed.store(true, Ordering::Release);
                shared.over_desktop.store(true, Ordering::Release);
                Some(MouseEvent::Down {
                    button: MouseButton::Left,
                    x: local_x,
                    y: local_y,
                    ctrl: is_ctrl_down(),
                })
            } else {
                None
            }
        }
        WM_MOUSEMOVE => {
            let dragging_left = shared.left_pressed.load(Ordering::Relaxed);
            if dragging_left {
                Some(MouseEvent::Move {
                    x: local_x,
                    y: local_y,
                })
            } else if in_monitor && is_desktop_point(pt, shared.our_hwnd.load(Ordering::Relaxed)) {
                shared.over_desktop.store(true, Ordering::Release);
                Some(MouseEvent::Move {
                    x: local_x,
                    y: local_y,
                })
            } else if shared.over_desktop.swap(false, Ordering::AcqRel) {
                Some(MouseEvent::Leave)
            } else {
                None
            }
        }
        _ => None,
    };

    if let Some(ev) = ev {
        // try_send non-blocking; jika kanal penuh, jatuhkan event agar input thread tak pernah macet.
        if shared.tx.try_send(ev).is_ok() {
            let _ = shared.proxy.send_event(SystemEvent::WakeUp);
        }
    }
}

/// Memeriksa apakah tombol Ctrl sedang ditekan.
#[inline]
fn is_ctrl_down() -> bool {
    // SAFETY: GetAsyncKeyState aman dipanggil dari hook proc tanpa alokasi.
    unsafe { (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 }
}

/// Memeriksa apakah titik kursor `pt` berada di atas desktop kosong (spec §7.5).
fn is_desktop_point(pt: POINT, our_hwnd: isize) -> bool {
    // SAFETY: WindowFromPoint membaca struktur jendela desktop; buffer nama kelas di stack.
    unsafe {
        let hwnd = WindowFromPoint(pt);
        if hwnd.0.is_null() {
            return false;
        }
        if our_hwnd != 0 && hwnd.0 as isize == our_hwnd {
            return true;
        }
        let mut buf = [0u16; 64];
        let len = GetClassNameW(hwnd, &mut buf);
        if len <= 0 {
            return false;
        }
        let cls = &buf[..len as usize];
        is_utf16_eq(cls, "Progman")
            || is_utf16_eq(cls, "WorkerW")
            || is_utf16_eq(cls, "SHELLDLL_DefView")
            || is_utf16_eq(cls, "SysListView32")
    }
}

#[inline]
fn is_utf16_eq(utf16: &[u16], ascii: &str) -> bool {
    let bytes = ascii.as_bytes();
    if utf16.len() != bytes.len() {
        return false;
    }
    utf16
        .iter()
        .zip(bytes.iter())
        .all(|(&a, &b)| a == u16::from(b))
}
