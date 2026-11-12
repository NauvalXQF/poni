#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod geometry;
mod island;
mod network;
mod surface;

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use chrono::Local;
use fontdue::{Font, FontSettings};
use geometry::{geometry_with_notice, WINDOW_HEIGHT, WINDOW_WIDTH};
use island::{draw as draw_island, IslandView, NoticeKind};
use network::{spawn_worker as spawn_network_worker, NetworkState};
use serde::{Deserialize, Serialize};
use surface::LayeredSurface;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, BOOL, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Dwm::{DwmGetCompositionTimingInfo, DWM_TIMING_INFO};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, ScreenToClient, UpdateWindow,
    MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};
use windows::Win32::System::Threading::{
    CreateWaitableTimerExW, SetWaitableTimer, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, INFINITE,
    TIMER_ALL_ACCESS,
};
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
};
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD,
    NIM_DELETE, NOTIFYICONDATAW, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetClassNameW, GetCursorPos, GetForegroundWindow, GetWindowLongPtrW,
    GetWindowRect, IsWindowVisible, IsZoomed, LoadCursorW, LoadIconW, MsgWaitForMultipleObjectsEx,
    PeekMessageW, PostMessageW, PostQuitMessage, RegisterClassW, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, TrackPopupMenu, TranslateMessage, CREATESTRUCTW,
    CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HTCLIENT, HTTRANSPARENT, HWND_TOPMOST, IDC_ARROW,
    IDI_APPLICATION, MA_NOACTIVATE, MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG,
    MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE,
    SW_SHOWNOACTIVATE, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_CREATE, WM_DESTROY,
    WM_DISPLAYCHANGE, WM_DPICHANGED, WM_HOTKEY, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
    WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST, WM_NULL, WM_QUIT, WM_RBUTTONUP, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

const WINDOW_CLASS: PCWSTR = w!("PoniIslandWindow");
const WINDOW_TITLE: PCWSTR = w!("Poni");
const HOTKEY_PIN: i32 = 1;
const HOTKEY_QUIT: i32 = 2;
/// One shared rate for shape and notice motion so every layer settles
/// together instead of racing each other.
const MOTION_RATE: f32 = 16.0;
const HOVER_INTENT: Duration = Duration::from_millis(140);
const NOTICE_DURATION: Duration = Duration::from_secs(3);
const NOTICE_QUICK_EXIT: Duration = Duration::from_millis(350);
const TRAY_MESSAGE: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;
const CMD_PIN: usize = 100;
const CMD_SCALE_75: usize = 110;
const CMD_SCALE_100: usize = 111;
const CMD_SCALE_125: usize = 112;
const CMD_SCALE_130: usize = 113;
const CMD_AUTOSTART: usize = 120;
const CMD_QUIT: usize = 130;
const SCALE_OPTIONS: [f32; 4] = [0.75, 1.0, 1.25, 1.3];
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("Poni");

fn default_shape_scale() -> f32 {
    1.0
}

#[derive(Debug, Serialize, Deserialize)]
struct Settings {
    #[serde(default = "default_shape_scale")]
    shape_scale: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { shape_scale: 1.0 }
    }
}

struct Fonts {
    regular: Option<Font>,
    semibold: Option<Font>,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
struct BatteryStatus {
    percent: Option<u8>,
    charging: bool,
}

#[derive(Clone)]
struct Notice {
    kind: NoticeKind,
    title: String,
    subtitle: String,
    battery: Option<u8>,
    until: Instant,
}

unsafe fn foreground_is_fullscreen(our_hwnd: HWND) -> bool {
    if let Ok(state) = SHQueryUserNotificationState() {
        if [
            QUNS_BUSY.0,
            QUNS_PRESENTATION_MODE.0,
            QUNS_RUNNING_D3D_FULL_SCREEN.0,
        ]
        .contains(&state.0)
        {
            return true;
        }
    }

    let foreground = GetForegroundWindow();
    if foreground.0.is_null()
        || foreground == our_hwnd
        || !IsWindowVisible(foreground).as_bool()
        || IsZoomed(foreground).as_bool()
    {
        return false;
    }

    let mut class_buffer = [0u16; 64];
    let class_len = GetClassNameW(foreground, &mut class_buffer);
    if class_len > 0 {
        let class_name = String::from_utf16_lossy(&class_buffer[..class_len as usize]);
        if matches!(class_name.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd") {
            return false;
        }
    }

    let mut rect = RECT::default();
    if GetWindowRect(foreground, &mut rect).is_err() {
        return false;
    }

    let monitor = MonitorFromWindow(foreground, MONITOR_DEFAULTTONEAREST);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(monitor, &mut info).as_bool() {
        return false;
    }

    let screen = info.rcMonitor;
    let tolerance = 2;
    rect.left <= screen.left + tolerance
        && rect.top <= screen.top + tolerance
        && rect.right >= screen.right - tolerance
        && rect.bottom >= screen.bottom - tolerance
}

fn battery_status() -> BatteryStatus {
    let mut status = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut status) }.is_err() {
        return BatteryStatus::default();
    }

    let percent = if status.BatteryFlag & 128 != 0 || status.BatteryLifePercent == 255 {
        None
    } else {
        Some(status.BatteryLifePercent.min(100))
    };

    BatteryStatus {
        percent,
        charging: status.BatteryFlag & 8 != 0,
    }
}

struct App {
    hwnd: HWND,
    progress: f32,
    hovered: bool,
    hover_since: Option<Instant>,
    pinned: bool,
    scale: f32,
    shape_scale: f32,
    tray: Option<NOTIFYICONDATAW>,
    window_width: i32,
    window_height: i32,
    surface: Option<LayeredSurface>,
    fonts: Fonts,
    hit_mask: Vec<u8>,
    time: String,
    date: String,
    battery: BatteryStatus,
    last_status_update: Instant,
    network_rx: Receiver<NetworkState>,
    network: NetworkState,
    network_initialized: bool,
    notice: Option<Notice>,
    notice_closing: bool,
    pending_notice: Option<Notice>,
    notice_mix: f32,
    hidden_for_fullscreen: bool,
    last_fullscreen_check: Instant,
    timer: HANDLE,
    refresh_hz: f32,
    last_tick: Instant,
}

impl App {
    fn new(timer: HANDLE) -> Self {
        let settings = load_settings();
        let network_rx = spawn_network_worker();
        Self {
            hwnd: HWND::default(),
            progress: 0.0,
            hovered: false,
            hover_since: None,
            pinned: false,
            scale: 1.0,
            shape_scale: settings
                .shape_scale
                .clamp(SCALE_OPTIONS[0], SCALE_OPTIONS[3]),
            tray: None,
            window_width: 0,
            window_height: 0,
            surface: None,
            fonts: load_fonts(),
            hit_mask: Vec::new(),
            time: Local::now().format("%H:%M").to_string(),
            date: Local::now().format("%a, %d %b").to_string(),
            battery: BatteryStatus::default(),
            last_status_update: Instant::now(),
            network_rx,
            network: NetworkState::default(),
            network_initialized: false,
            notice: None,
            notice_closing: false,
            pending_notice: None,
            notice_mix: 0.0,
            hidden_for_fullscreen: false,
            last_fullscreen_check: Instant::now(),
            timer,
            refresh_hz: detect_refresh_rate(),
            last_tick: Instant::now(),
        }
    }

    unsafe fn attach(&mut self, hwnd: HWND) {
        self.hwnd = hwnd;
        self.scale = dpi_scale(hwnd);
        self.refresh_hz = detect_refresh_rate();
        self.update_system_status(true);
        self.resize_window();
        self.update_fullscreen_visibility(true);
        log_line(&format!("Poni started at {:.1} Hz", self.refresh_hz));

        let _ = RegisterHotKey(
            hwnd,
            HOTKEY_PIN,
            MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
            'N' as u32,
        );
        let _ = RegisterHotKey(
            hwnd,
            HOTKEY_QUIT,
            MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
            'Q' as u32,
        );

        self.add_tray_icon();
        self.redraw();
    }

    fn effective_scale(&self) -> f32 {
        self.scale * self.shape_scale
    }

    unsafe fn resize_window(&mut self) {
        let scale = self.effective_scale();
        self.window_width = (WINDOW_WIDTH * scale).round() as i32;
        self.window_height = (WINDOW_HEIGHT * scale).round() as i32;
        self.hit_mask = vec![0; (self.window_width * self.window_height) as usize];

        self.surface = LayeredSurface::new(self.window_width, self.window_height)
            .map_err(|err| {
                log_line(&format!("surface creation failed: {err}"));
            })
            .ok();

        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let monitor_rect = if GetMonitorInfoW(monitor, &mut info).as_bool() {
            info.rcMonitor
        } else {
            windows::Win32::Foundation::RECT {
                left: 0,
                top: 0,
                right: self.window_width,
                bottom: self.window_height,
            }
        };

        let x =
            monitor_rect.left + (monitor_rect.right - monitor_rect.left - self.window_width) / 2;
        let y = monitor_rect.top;

        let mut flags = SWP_NOACTIVATE;
        if !self.hidden_for_fullscreen {
            flags |= SWP_SHOWWINDOW;
        }
        let _ = SetWindowPos(
            self.hwnd,
            HWND_TOPMOST,
            x,
            y,
            self.window_width,
            self.window_height,
            flags,
        );
        if !self.hidden_for_fullscreen {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
    }

    fn geometry(&self) -> geometry::Geometry {
        geometry_with_notice(
            self.progress,
            self.notice_mix,
            self.effective_scale(),
            self.window_width as f32,
        )
    }

    fn hit_test(&self, px: f32, py: f32) -> bool {
        let x = px.floor() as i32;
        let y = py.floor() as i32;
        if x < 0 || y < 0 || x >= self.window_width || y >= self.window_height {
            return false;
        }
        let index = y as usize * self.window_width as usize + x as usize;
        self.hit_mask.get(index).copied().unwrap_or(0) > 127
    }

    fn update_fullscreen_visibility(&mut self, force: bool) {
        let now = Instant::now();
        if !force && now.duration_since(self.last_fullscreen_check) < Duration::from_millis(500) {
            return;
        }
        self.last_fullscreen_check = now;

        let fullscreen = unsafe { foreground_is_fullscreen(self.hwnd) };
        if fullscreen == self.hidden_for_fullscreen {
            return;
        }

        self.hidden_for_fullscreen = fullscreen;
        unsafe {
            if fullscreen {
                self.pinned = false;
                self.hovered = false;
                self.hover_since = None;
                self.progress = 0.0;
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            } else {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                self.redraw();
                self.request_frame();
            }
        }
    }

    fn update_system_status(&mut self, force: bool) -> bool {
        let now = Instant::now();
        if !force && now.duration_since(self.last_status_update) < Duration::from_secs(1) {
            return false;
        }
        self.last_status_update = now;

        let mut changed = false;
        let battery = battery_status();
        if self.battery != battery {
            self.battery = battery;
            changed = true;
        }

        self.update_network_state();
        changed
    }

    fn show_notice(&mut self, kind: NoticeKind, title: &str, subtitle: String) {
        self.show_notice_with_battery(kind, title, subtitle, None);
    }

    fn show_notice_with_battery(
        &mut self,
        kind: NoticeKind,
        title: &str,
        subtitle: String,
        battery: Option<u8>,
    ) {
        if self.hidden_for_fullscreen {
            return;
        }
        let now = Instant::now();
        let notice = Notice {
            kind,
            title: title.to_string(),
            subtitle,
            battery,
            until: now + NOTICE_DURATION,
        };

        if self.notice.is_some() {
            // Never swap text while a notice is fully visible. Queue the new
            // one and let the current notice leave first.
            self.pending_notice = Some(notice);
            if !self.notice_closing {
                if let Some(current) = self.notice.as_mut() {
                    let quick = now + NOTICE_QUICK_EXIT;
                    if current.until > quick {
                        current.until = quick;
                    }
                }
            }
        } else {
            self.notice = Some(notice);
            self.notice_closing = false;
        }
        self.request_frame();
    }

    fn update_network_state(&mut self) {
        while let Ok(current) = self.network_rx.try_recv() {
            if !self.network_initialized {
                self.network = current;
                self.network_initialized = true;
                continue;
            }

            let new_wifi = if current.wifi != self.network.wifi {
                current.wifi.clone()
            } else {
                None
            };
            let new_bluetooth = current
                .bluetooth
                .iter()
                .find(|device| !self.network.bluetooth.contains(device))
                .cloned();
            let airplane_changed = current.airplane != self.network.airplane;

            if let Some(ssid) = new_wifi {
                self.show_notice(NoticeKind::Wifi, "Wi-Fi terhubung", ssid);
            } else if let Some(device) = new_bluetooth {
                self.show_notice(NoticeKind::Bluetooth, "Bluetooth terhubung", device);
            } else if airplane_changed {
                self.show_notice(
                    NoticeKind::Airplane,
                    "Mode pesawat",
                    if current.airplane {
                        "Aktif"
                    } else {
                        "Nonaktif"
                    }
                    .to_string(),
                );
            }

            self.network = current;
        }
    }

    unsafe fn tick(&mut self) {
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_tick)
            .as_secs_f32()
            .clamp(0.001, 0.05);
        self.last_tick = now;
        self.update_fullscreen_visibility(false);

        let mut point = POINT::default();
        if GetCursorPos(&mut point).is_ok() {
            let _ = ScreenToClient(self.hwnd, &mut point);
            self.hovered = self.hit_test(point.x as f32, point.y as f32);
        }
        if self.hovered && self.notice.is_none() {
            self.hover_since.get_or_insert(now);
        } else {
            self.hover_since = None;
        }

        // Wait out a short hover intent before expanding. Collapsing stays
        // immediate so the island never lingers open after the pointer leaves.
        let hover_ready = self
            .hover_since
            .is_some_and(|since| now.duration_since(since) >= HOVER_INTENT);
        let notice_active = self.notice.is_some();
        let target = if notice_active {
            0.0
        } else if self.pinned || hover_ready {
            1.0
        } else {
            0.0
        };
        let clock = Local::now().format("%H:%M").to_string();
        let date = Local::now().format("%a, %d %b").to_string();

        let mut changed = self.update_system_status(false);
        if self.time != clock {
            self.time = clock;
            changed = true;
        }
        if self.date != date {
            self.date = date;
            changed = true;
        }
        if !self.notice_closing
            && self
                .notice
                .as_ref()
                .is_some_and(|notice| now >= notice.until)
        {
            // Keep the notice content alive while the shape fades out.
            self.notice_closing = true;
            changed = true;
        }
        let notice_target = if notice_active && !self.notice_closing {
            1.0
        } else {
            0.0
        };
        if approach(&mut self.progress, target, dt, MOTION_RATE) {
            changed = true;
        }
        if approach(&mut self.notice_mix, notice_target, dt, MOTION_RATE) {
            changed = true;
        }

        if self.notice_closing && self.notice_mix <= 0.001 {
            self.notice_mix = 0.0;
            self.notice = None;
            self.notice_closing = false;
            changed = true;
            if let Some(mut next) = self.pending_notice.take() {
                next.until = Instant::now() + NOTICE_DURATION;
                self.notice = Some(next);
                self.request_frame();
            }
        }

        if changed {
            self.redraw();
        }
    }

    fn request_frame(&self) {
        unsafe {
            set_frame_timer(self.timer, 1.0);
        }
    }

    fn next_interval_ms(&self) -> f32 {
        let target = if self.notice.is_some() {
            0.0
        } else if self.pinned
            || self
                .hover_since
                .is_some_and(|since| since.elapsed() >= HOVER_INTENT)
        {
            1.0
        } else {
            0.0
        };
        let notice_target = if self.notice.is_some() && !self.notice_closing {
            1.0
        } else {
            0.0
        };
        let animating = (self.progress - target).abs() > 0.002
            || (self.notice_mix - notice_target).abs() > 0.002;

        if animating {
            1000.0 / self.refresh_hz.max(30.0)
        } else if self.progress > 0.001 || self.notice_mix > 0.001 {
            33.0
        } else {
            35.0
        }
    }

    fn redraw(&mut self) {
        if self.hidden_for_fullscreen {
            return;
        }
        let Some(surface) = self.surface.take() else {
            return;
        };
        let view = IslandView {
            progress: self.progress,
            geometry: self.geometry(),
            scale: self.effective_scale(),
            time: self.time.clone(),
            date: self.date.clone(),
            battery_percent: self.battery.percent,
            battery_charging: self.battery.charging,
            wifi: self.network.wifi.as_deref(),
            bluetooth: self
                .network
                .bluetooth
                .first()
                .map(std::string::String::as_str),
            notice_mix: self.notice_mix,
            notice_kind: self.notice.as_ref().map(|notice| notice.kind),
            notice_title: self.notice.as_ref().map(|notice| notice.title.as_str()),
            notice_subtitle: self.notice.as_ref().map(|notice| notice.subtitle.as_str()),
            notice_battery: self.notice.as_ref().and_then(|notice| notice.battery),
            notice_closing: self.notice_closing,
            regular: self.fonts.regular.as_ref(),
            semibold: self.fonts.semibold.as_ref(),
        };

        if let Some(pixmap) = draw_island(surface.width as u32, surface.height as u32, &view) {
            if self.hit_mask.len() == (pixmap.width() * pixmap.height()) as usize {
                for (dst, src) in self.hit_mask.iter_mut().zip(pixmap.data().chunks_exact(4)) {
                    *dst = if src[3] > 127 { 255 } else { 0 };
                }
            }
            surface.present(self.hwnd, &pixmap);
        }
        self.surface = Some(surface);
    }

    unsafe fn set_shape_scale(&mut self, shape_scale: f32) {
        let shape_scale = shape_scale.clamp(SCALE_OPTIONS[0], SCALE_OPTIONS[3]);
        if (self.shape_scale - shape_scale).abs() < f32::EPSILON {
            return;
        }
        self.shape_scale = shape_scale;
        save_settings(self.shape_scale);
        self.resize_window();
        self.request_frame();
    }

    unsafe fn toggle_pin(&mut self) {
        self.pinned = !self.pinned;
        self.request_frame();
    }

    unsafe fn add_tray_icon(&mut self) {
        if self.tray.is_some() {
            return;
        }

        let icon = match LoadIconW(None, IDI_APPLICATION) {
            Ok(icon) => icon,
            Err(err) => {
                log_line(&format!("LoadIconW failed: {err}"));
                return;
            }
        };

        let mut data = NOTIFYICONDATAW::default();
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.hwnd;
        data.uID = TRAY_ID;
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = TRAY_MESSAGE;
        data.hIcon = icon;
        let tip: Vec<u16> = "Poni".encode_utf16().chain(std::iter::once(0)).collect();
        data.szTip[..tip.len()].copy_from_slice(&tip);

        if Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
            self.tray = Some(data);
        } else {
            log_line("Shell_NotifyIconW(NIM_ADD) failed");
        }
    }

    unsafe fn remove_tray_icon(&mut self) {
        if let Some(data) = self.tray.take() {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        }
    }

    unsafe fn show_tray_menu(&mut self) {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };

        let _ = AppendMenuW(
            menu,
            if self.pinned {
                MF_STRING | MF_CHECKED
            } else {
                MF_STRING
            },
            CMD_PIN,
            w!("Pin expanded"),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, w!(""));

        if let Ok(size_menu) = CreatePopupMenu() {
            let options = [
                (CMD_SCALE_75, 0.75, w!("75%")),
                (CMD_SCALE_100, 1.0, w!("100%")),
                (CMD_SCALE_125, 1.25, w!("125%")),
                (CMD_SCALE_130, 1.3, w!("130%")),
            ];
            for (id, value, label) in options {
                let flags = if (self.shape_scale - value).abs() < 0.001 {
                    MF_STRING | MF_CHECKED
                } else {
                    MF_STRING
                };
                let _ = AppendMenuW(size_menu, flags, id, label);
            }
            let _ = AppendMenuW(menu, MF_POPUP, size_menu.0 as usize, w!("Size"));
        }

        let autostart_flags = if autostart_enabled() {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let _ = AppendMenuW(
            menu,
            autostart_flags,
            CMD_AUTOSTART,
            w!("Start with Windows"),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, w!(""));
        let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT, w!("Quit"));

        let _ = SetForegroundWindow(self.hwnd);
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            self.hwnd,
            None,
        )
        .0 as usize;
        let _ = DestroyMenu(menu);
        let _ = PostMessageW(self.hwnd, WM_NULL, WPARAM(0), LPARAM(0));

        match command {
            CMD_PIN => self.toggle_pin(),
            CMD_SCALE_75 => self.set_shape_scale(0.75),
            CMD_SCALE_100 => self.set_shape_scale(1.0),
            CMD_SCALE_125 => self.set_shape_scale(1.25),
            CMD_SCALE_130 => self.set_shape_scale(1.3),
            CMD_AUTOSTART => set_autostart(!autostart_enabled()),
            CMD_QUIT => {
                let _ = DestroyWindow(self.hwnd);
            }
            _ => {}
        }
    }
}

fn approach(current: &mut f32, target: f32, dt: f32, rate: f32) -> bool {
    let delta = target - *current;
    if delta.abs() < 0.002 {
        *current = target;
        return false;
    }
    let alpha = 1.0 - (-rate * dt).exp();
    *current += delta * alpha;
    if (target - *current).abs() < 0.002 {
        *current = target;
    }
    true
}

fn dpi_scale(hwnd: HWND) -> f32 {
    unsafe {
        let dpi = GetDpiForWindow(hwnd);
        if dpi == 0 {
            1.0
        } else {
            dpi as f32 / 96.0
        }
    }
}

fn detect_refresh_rate() -> f32 {
    if let Ok(value) = std::env::var("PONI_REFRESH_HZ") {
        if let Ok(hz) = value.parse::<f32>() {
            if hz.is_finite() && (30.0..=240.0).contains(&hz) {
                return hz;
            }
        }
    }

    unsafe {
        let mut info = DWM_TIMING_INFO::default();
        info.cbSize = std::mem::size_of::<DWM_TIMING_INFO>() as u32;
        if DwmGetCompositionTimingInfo(None, &mut info).is_ok() && info.qpcRefreshPeriod > 0 {
            let hz = 10_000_000.0 / info.qpcRefreshPeriod as f32;
            if hz.is_finite() && (30.0..=240.0).contains(&hz) {
                return hz;
            }
        }
    }
    60.0
}

unsafe fn create_frame_timer() -> windows::core::Result<HANDLE> {
    CreateWaitableTimerExW(
        None,
        w!(""),
        CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
        TIMER_ALL_ACCESS.0,
    )
    .or_else(|_| CreateWaitableTimerExW(None, w!(""), 0, TIMER_ALL_ACCESS.0))
}

unsafe fn set_frame_timer(timer: HANDLE, interval_ms: f32) {
    if timer.0.is_null() {
        return;
    }
    let due = -((interval_ms.max(0.5) * 10_000.0) as i64);
    let _ = SetWaitableTimer(timer, &due, 0, None, None, BOOL(0));
}

fn settings_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join("Poni").join("settings.json"))
}

fn load_settings() -> Settings {
    settings_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default()
}

fn save_settings(shape_scale: f32) {
    let Some(path) = settings_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(contents) = serde_json::to_string_pretty(&Settings { shape_scale }) {
        let _ = std::fs::write(path, contents);
    }
}

fn autostart_enabled() -> bool {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, KEY_QUERY_VALUE, &mut key).0 != 0 {
            return false;
        }
        let mut size = 0u32;
        let status = RegQueryValueExW(key, RUN_VALUE, None, None, None, Some(&mut size));
        let _ = RegCloseKey(key);
        status.0 == 0 && size > 0
    }
}

unsafe fn set_autostart(enabled: bool) {
    let mut key = HKEY::default();
    if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, KEY_SET_VALUE, &mut key).0 != 0 {
        return;
    }

    if enabled {
        let Ok(exe) = std::env::current_exe() else {
            let _ = RegCloseKey(key);
            return;
        };
        let value = format!("\"{}\"", exe.display());
        let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let data = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2);
        let _ = RegSetValueExW(key, RUN_VALUE, 0, REG_SZ, Some(data));
    } else {
        let _ = RegDeleteValueW(key, RUN_VALUE);
    }

    let _ = RegCloseKey(key);
}

fn font_directory() -> PathBuf {
    std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("Fonts")
}

fn load_font_file(names: &[&str]) -> Option<Font> {
    for name in names {
        let path = font_directory().join(name);
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                return Some(font);
            }
        }
    }
    None
}

fn load_fonts() -> Fonts {
    let fonts = Fonts {
        regular: load_font_file(&["segoeui.ttf", "arial.ttf"]),
        semibold: load_font_file(&["seguisb.ttf", "segoeuib.ttf", "arialbd.ttf"]),
    };
    if fonts.regular.is_none() {
        log_line("no regular UI font found; falling back to semibold");
    }
    fonts
}

fn log_line(message: &str) {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let path = PathBuf::from(appdata).join("Poni").join("poni.log");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{message}");
        }
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            let app = Box::from_raw(create.lpCreateParams as *mut App);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(app) as isize);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CREATE => {
            if let Some(app) = app_mut(hwnd) {
                app.attach(hwnd);
            }
            LRESULT(0)
        }
        TRAY_MESSAGE => {
            let event = lparam.0 as u32;
            if event == WM_LBUTTONUP || event == WM_RBUTTONUP {
                if let Some(app) = app_mut(hwnd) {
                    app.show_tray_menu();
                }
            }
            LRESULT(0)
        }
        WM_NCHITTEST => {
            let Some(app) = app_ref(hwnd) else {
                return LRESULT(HTTRANSPARENT as isize);
            };
            let x = (lparam.0 & 0xffff) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
            let mut point = POINT { x, y };
            let _ = ScreenToClient(hwnd, &mut point);
            if app.hit_test(point.x as f32, point.y as f32) {
                LRESULT(HTCLIENT as isize)
            } else {
                LRESULT(HTTRANSPARENT as isize)
            }
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            if let Some(app) = app_mut(hwnd) {
                app.hovered = true;
                app.request_frame();
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some(app) = app_mut(hwnd) {
                app.toggle_pin();
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            if let Some(app) = app_mut(hwnd) {
                app.show_tray_menu();
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            match wparam.0 as i32 {
                HOTKEY_PIN => {
                    if let Some(app) = app_mut(hwnd) {
                        app.toggle_pin();
                    }
                }
                HOTKEY_QUIT => {
                    let _ = DestroyWindow(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            if let Some(app) = app_mut(hwnd) {
                app.scale = dpi_scale(hwnd);
                app.refresh_hz = detect_refresh_rate();
                app.resize_window();
                app.request_frame();
            }
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            if let Some(app) = app_mut(hwnd) {
                app.refresh_hz = detect_refresh_rate();
                app.request_frame();
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                if let Some(app) = app_mut(hwnd) {
                    app.remove_tray_icon();
                }
                let _ = UnregisterHotKey(hwnd, HOTKEY_PIN);
                let _ = UnregisterHotKey(hwnd, HOTKEY_QUIT);
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let ptr = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            if ptr != 0 {
                drop(Box::from_raw(ptr as *mut App));
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn app_ref(hwnd: HWND) -> Option<&'static App> {
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const App;
        ptr.as_ref()
    }
}

fn app_mut(hwnd: HWND) -> Option<&'static mut App> {
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
        ptr.as_mut()
    }
}

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = match GetModuleHandleW(None) {
            Ok(instance) => instance,
            Err(err) => {
                log_line(&format!("GetModuleHandleW failed: {err}"));
                return;
            }
        };
        let hinstance = HINSTANCE(instance.0);
        let cursor = match LoadCursorW(None, IDC_ARROW) {
            Ok(cursor) => cursor,
            Err(err) => {
                log_line(&format!("LoadCursorW failed: {err}"));
                return;
            }
        };

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
            hCursor: cursor,
            lpszClassName: WINDOW_CLASS,
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            log_line("RegisterClassW failed");
            return;
        }

        let timer = match create_frame_timer() {
            Ok(timer) => timer,
            Err(err) => {
                log_line(&format!("CreateWaitableTimerExW failed: {err}"));
                return;
            }
        };

        let app = Box::into_raw(Box::new(App::new(timer)));
        let hwnd = match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            WINDOW_CLASS,
            WINDOW_TITLE,
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            hinstance,
            Some(app as *const c_void),
        ) {
            Ok(hwnd) => hwnd,
            Err(err) => {
                drop(Box::from_raw(app));
                let _ = CloseHandle(timer);
                log_line(&format!("CreateWindowExW failed: {err}"));
                return;
            }
        };

        let _ = UpdateWindow(hwnd);
        set_frame_timer(timer, 1.0);

        let mut running = true;
        while running {
            let wait = MsgWaitForMultipleObjectsEx(
                Some(&[timer]),
                INFINITE,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            );

            let mut ticked = false;
            if wait == WAIT_OBJECT_0 {
                if let Some(app) = app_mut(hwnd) {
                    app.tick();
                    ticked = true;
                }
            }

            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if message.message == WM_QUIT {
                    running = false;
                    break;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }

            if !running {
                break;
            }

            if let Some(app) = app_mut(hwnd) {
                if !ticked {
                    app.tick();
                }
                let interval = app.next_interval_ms();
                set_frame_timer(timer, interval);
            } else {
                break;
            }
        }

        let _ = CloseHandle(timer);
    }
}
