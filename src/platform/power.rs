//! Pemicu jeda daya (spec §14, M5): layar penuh/presentasi, lock sesi, layar mati,
//! dan aplikasi foreground maximized. Notifikasi sesi & daya memakai satu jendela
//! helper tersembunyi; status layar-fullscreen dipoll oleh pemanggil.

#![cfg(windows)]

use crate::platform::SystemEvent;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use windows::core::w;
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{
    GetSystemPowerStatus, RegisterPowerSettingNotification, UnregisterPowerSettingNotification,
    HPOWERNOTIFY, POWERBROADCAST_SETTING, SYSTEM_POWER_STATUS,
};
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
use windows::Win32::UI::Shell::SHQueryUserNotificationState;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, PBT_POWERSETTINGCHANGE,
    REGISTER_NOTIFICATION_FLAGS, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSEXW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};
use winit::event_loop::EventLoopProxy;

/// Kode `SHQueryUserNotificationState` yang menandakan layar penuh/presentasi.
/// `QUNS_BUSY` = aplikasi layar penuh/presentasi aktif di depan.
const QUNS_BUSY: i32 = 2;
const QUNS_RUNNING_D3D_FULL_SCREEN: i32 = 3;
const QUNS_PRESENTATION_MODE: i32 = 4;
/// Nilai `Data[0]` untuk `GUID_CONSOLE_DISPLAY_STATE`: monitor mati.
const DISPLAY_STATE_OFF: u8 = 0;

static PROXY: OnceLock<EventLoopProxy<SystemEvent>> = OnceLock::new();
static SESSION_LOCKED: AtomicBool = AtomicBool::new(false);
static DISPLAY_OFF: AtomicBool = AtomicBool::new(false);

/// Pegangan jendela notifikasi daya/sesi. Melepas notifikasi saat di-drop.
pub struct PowerMonitor {
    hwnd: HWND,
    power: HPOWERNOTIFY,
}

impl Drop for PowerMonitor {
    fn drop(&mut self) {
        // SAFETY: hwnd & power dimiliki monitor ini dan dilepas sekali.
        unsafe {
            let _ = UnregisterPowerSettingNotification(self.power);
            let _ = WTSUnRegisterSessionNotification(self.hwnd);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// Memasang jendela helper dan mendaftarkan notifikasi lock sesi & layar mati.
pub fn install(proxy: EventLoopProxy<SystemEvent>) -> Result<PowerMonitor, String> {
    let _ = PROXY.set(proxy);
    SESSION_LOCKED.store(false, Ordering::Relaxed);
    DISPLAY_OFF.store(false, Ordering::Relaxed);

    // SAFETY: pendaftaran kelas + pembuatan satu jendela tersembunyi milik proses ini.
    unsafe {
        let hinst = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW: {e}"))?;
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(power_wndproc),
            hInstance: windows::Win32::Foundation::HINSTANCE(hinst.0),
            lpszClassName: w!("BubbleDeskPower"),
            ..Default::default()
        };
        let _ = RegisterClassExW(&class);
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("BubbleDeskPower"),
            w!("BubbleDeskPower"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(windows::Win32::Foundation::HINSTANCE(hinst.0)),
            None,
        )
        .map_err(|e| format!("CreateWindowExW(power): {e}"))?;

        WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)
            .map_err(|e| format!("WTSRegisterSessionNotification: {e}"))?;

        let power = RegisterPowerSettingNotification(
            HANDLE(hwnd.0),
            &GUID_CONSOLE_DISPLAY_STATE,
            REGISTER_NOTIFICATION_FLAGS(0), // DEVICE_NOTIFY_WINDOW_HANDLE
        )
        .map_err(|e| format!("RegisterPowerSettingNotification: {e}"))?;

        Ok(PowerMonitor { hwnd, power })
    }
}

/// Memutuskan apakah render/simulasi harus dijeda saat ini.
pub fn is_paused(pause_on_fullscreen: bool, pause_on_lock: bool) -> bool {
    if pause_on_lock && SESSION_LOCKED.load(Ordering::Relaxed) {
        return true;
    }
    if DISPLAY_OFF.load(Ordering::Relaxed) {
        return true;
    }
    pause_on_fullscreen && fullscreen_or_presenting()
}

/// True bila proses berjalan dari baterai (`ACLineStatus == 0`).
pub fn on_battery() -> bool {
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: pointer ke struct stack yang valid.
    if unsafe { GetSystemPowerStatus(&mut status) }.is_err() {
        return false;
    }
    status.ACLineStatus == 0
}

fn fullscreen_or_presenting() -> bool {
    // SAFETY: SHQueryUserNotificationState tidak mengambil pointer.
    if let Ok(state) = unsafe { SHQueryUserNotificationState() } {
        if matches!(
            state.0,
            QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE
        ) {
            return true;
        }
    }
    false
}

fn notify_change() {
    if let Some(proxy) = PROXY.get() {
        let _ = proxy.send_event(SystemEvent::PowerChanged);
    }
}

unsafe extern "system" fn power_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_WTSSESSION_CHANGE => {
            let code = wparam.0 as u32;
            if code == WTS_SESSION_LOCK {
                SESSION_LOCKED.store(true, Ordering::Relaxed);
                notify_change();
            } else if code == WTS_SESSION_UNLOCK {
                SESSION_LOCKED.store(false, Ordering::Relaxed);
                notify_change();
            }
        }
        WM_POWERBROADCAST if wparam.0 as u32 == PBT_POWERSETTINGCHANGE => {
            // SAFETY: untuk PBT_POWERSETTINGCHANGE, lparam menunjuk POWERBROADCAST_SETTING valid.
            let setting = unsafe { &*(lparam.0 as *const POWERBROADCAST_SETTING) };
            if setting.PowerSetting == GUID_CONSOLE_DISPLAY_STATE && setting.DataLength >= 1 {
                DISPLAY_OFF.store(setting.Data[0] == DISPLAY_STATE_OFF, Ordering::Relaxed);
                notify_change();
            }
        }
        _ => {}
    }
    // SAFETY: penerusan standar pesan Win32.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
