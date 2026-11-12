#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod geometry;
mod island;

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::time::{Duration, Instant};

use chrono::Local;
use fontdue::{Font, FontSettings};
use geometry::{geometry, IslandMode, WINDOW_HEIGHT, WINDOW_WIDTH};
use island::{draw as draw_island, IslandView};
use serde::{Deserialize, Serialize};
use tiny_skia::Pixmap;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, BOOL, COLORREF, FILETIME, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE,
    WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Dwm::{DwmGetCompositionTimingInfo, DWM_TIMING_INFO};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW,
    MonitorFromPoint, ReleaseDC, ScreenToClient, SelectObject, UpdateWindow, AC_SRC_ALPHA,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HDC, HGDIOBJ,
    MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};
use windows::Win32::System::Threading::{
    CreateWaitableTimerExW, GetSystemTimes, SetWaitableTimer,
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, INFINITE, TIMER_ALL_ACCESS,
};
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetWindowLongPtrW, LoadCursorW, LoadIconW,
    MsgWaitForMultipleObjectsEx, PeekMessageW, PostMessageW, PostQuitMessage, RegisterClassW,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, TrackPopupMenu,
    TranslateMessage, UpdateLayeredWindow, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA,
    HTCLIENT, HTTRANSPARENT, HWND_TOPMOST, IDC_ARROW, IDI_APPLICATION, MA_NOACTIVATE, MF_CHECKED,
    MF_POPUP, MF_SEPARATOR, MF_STRING, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_SHOWNOACTIVATE, TPM_RETURNCMD, TPM_RIGHTBUTTON, ULW_ALPHA,
    WM_APP, WM_CREATE, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_HOTKEY, WM_LBUTTONUP,
    WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST, WM_NULL, WM_QUIT,
    WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};

const WINDOW_CLASS: PCWSTR = w!("PoniIslandWindow");
const WINDOW_TITLE: PCWSTR = w!("Poni");
const HOTKEY_MODE: i32 = 1;
const HOTKEY_QUIT: i32 = 2;
const ANIMATION_RATE: f32 = 18.0;
const MODE_RATE: f32 = 13.0;
const TRAY_MESSAGE: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;
const CMD_NOTCH: usize = 100;
const CMD_PILL: usize = 101;
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
    mode: IslandMode,
    #[serde(default = "default_shape_scale")]
    shape_scale: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: IslandMode::Notch,
            shape_scale: 1.0,
        }
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
    plugged: bool,
}

#[derive(Default)]
struct CpuSampler {
    idle: u64,
    kernel: u64,
    user: u64,
    initialized: bool,
}

impl CpuSampler {
    fn sample(&mut self) -> Option<u8> {
        let mut idle = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        unsafe {
            GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).ok()?;
        }

        let idle = filetime_value(idle);
        let kernel = filetime_value(kernel);
        let user = filetime_value(user);

        if !self.initialized {
            self.idle = idle;
            self.kernel = kernel;
            self.user = user;
            self.initialized = true;
            return None;
        }

        let idle_delta = idle.saturating_sub(self.idle);
        let kernel_delta = kernel.saturating_sub(self.kernel);
        let user_delta = user.saturating_sub(self.user);
        self.idle = idle;
        self.kernel = kernel;
        self.user = user;

        let total = kernel_delta.saturating_add(user_delta);
        if total == 0 {
            return None;
        }
        let busy = total.saturating_sub(idle_delta);
        Some(
            ((busy as f64 / total as f64) * 100.0)
                .round()
                .clamp(0.0, 100.0) as u8,
        )
    }
}

fn filetime_value(time: FILETIME) -> u64 {
    ((time.dwHighDateTime as u64) << 32) | time.dwLowDateTime as u64
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
        plugged: status.ACLineStatus == 1,
    }
}

struct App {
    hwnd: HWND,
    mode: IslandMode,
    mode_mix: f32,
    progress: f32,
    hovered: bool,
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
    cpu_percent: Option<u8>,
    battery: BatteryStatus,
    cpu_sampler: CpuSampler,
    last_status_update: Instant,
    timer: HANDLE,
    refresh_hz: f32,
    last_tick: Instant,
}

impl App {
    fn new(timer: HANDLE) -> Self {
        let settings = load_settings();
        let mode = settings.mode;
        Self {
            hwnd: HWND::default(),
            mode,
            mode_mix: mode.mix(),
            progress: 0.0,
            hovered: false,
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
            cpu_percent: None,
            battery: BatteryStatus::default(),
            cpu_sampler: CpuSampler::default(),
            last_status_update: Instant::now(),
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
        log_line(&format!("Poni started at {:.1} Hz", self.refresh_hz));

        let _ = RegisterHotKey(
            hwnd,
            HOTKEY_MODE,
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

        let _ = SetWindowPos(
            self.hwnd,
            HWND_TOPMOST,
            x,
            y,
            self.window_width,
            self.window_height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
    }

    fn geometry(&self) -> geometry::Geometry {
        geometry(
            self.mode_mix,
            self.progress,
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

    fn update_system_status(&mut self, force: bool) -> bool {
        let now = Instant::now();
        if !force && now.duration_since(self.last_status_update) < Duration::from_secs(1) {
            return false;
        }
        self.last_status_update = now;

        let mut changed = false;
        if let Some(cpu) = self.cpu_sampler.sample() {
            if self.cpu_percent != Some(cpu) {
                self.cpu_percent = Some(cpu);
                changed = true;
            }
        }

        let battery = battery_status();
        if self.battery != battery {
            self.battery = battery;
            changed = true;
        }

        changed
    }

    unsafe fn tick(&mut self) {
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_tick)
            .as_secs_f32()
            .clamp(0.001, 0.05);
        self.last_tick = now;

        let mut point = POINT::default();
        if GetCursorPos(&mut point).is_ok() {
            let _ = ScreenToClient(self.hwnd, &mut point);
            self.hovered = self.hit_test(point.x as f32, point.y as f32);
        }

        let target = if self.pinned || self.hovered {
            1.0
        } else {
            0.0
        };
        let target_mode = self.mode.mix();
        let clock = Local::now().format("%H:%M").to_string();

        let mut changed = self.update_system_status(false);
        if self.time != clock {
            self.time = clock;
            changed = true;
        }
        if approach(&mut self.progress, target, dt, ANIMATION_RATE) {
            changed = true;
        }
        if approach(&mut self.mode_mix, target_mode, dt, MODE_RATE) {
            changed = true;
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
        let target = if self.pinned || self.hovered {
            1.0
        } else {
            0.0
        };
        let animating = (self.progress - target).abs() > 0.002
            || (self.mode_mix - self.mode.mix()).abs() > 0.002;

        if animating {
            1000.0 / self.refresh_hz.max(30.0)
        } else if self.progress > 0.001 {
            33.0
        } else {
            35.0
        }
    }

    fn redraw(&mut self) {
        let Some(surface) = self.surface.take() else {
            return;
        };
        let view = IslandView {
            mode: self.mode,
            mode_mix: self.mode_mix,
            progress: self.progress,
            geometry: self.geometry(),
            scale: self.effective_scale(),
            pinned: self.pinned,
            time: self.time.clone(),
            cpu_percent: self.cpu_percent,
            battery_percent: self.battery.percent,
            battery_charging: self.battery.charging,
            battery_plugged: self.battery.plugged,
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

    unsafe fn toggle_mode(&mut self) {
        self.set_mode(self.mode.toggle());
    }

    unsafe fn set_mode(&mut self, mode: IslandMode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        save_settings(self.mode, self.shape_scale);
        self.request_frame();
    }

    unsafe fn set_shape_scale(&mut self, shape_scale: f32) {
        let shape_scale = shape_scale.clamp(SCALE_OPTIONS[0], SCALE_OPTIONS[3]);
        if (self.shape_scale - shape_scale).abs() < f32::EPSILON {
            return;
        }
        self.shape_scale = shape_scale;
        save_settings(self.mode, self.shape_scale);
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

        let shape_flags = |checked: bool| {
            if checked {
                MF_STRING | MF_CHECKED
            } else {
                MF_STRING
            }
        };

        let _ = AppendMenuW(
            menu,
            shape_flags(self.mode == IslandMode::Notch),
            CMD_NOTCH,
            w!("Notch"),
        );
        let _ = AppendMenuW(
            menu,
            shape_flags(self.mode == IslandMode::Pill),
            CMD_PILL,
            w!("Pill"),
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
            CMD_NOTCH => self.set_mode(IslandMode::Notch),
            CMD_PILL => self.set_mode(IslandMode::Pill),
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

fn save_settings(mode: IslandMode, shape_scale: f32) {
    let Some(path) = settings_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(contents) = serde_json::to_string_pretty(&Settings { mode, shape_scale }) {
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

struct LayeredSurface {
    width: i32,
    height: i32,
    screen: HDC,
    memory: HDC,
    bitmap: windows::Win32::Graphics::Gdi::HBITMAP,
    old_bitmap: HGDIOBJ,
    bits: *mut u8,
}

impl LayeredSurface {
    fn new(width: i32, height: i32) -> windows::core::Result<Self> {
        if width <= 0 || height <= 0 {
            return Err(windows::core::Error::from(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        }

        unsafe {
            let screen = GetDC(None);
            if screen.is_invalid() {
                return Err(windows::core::Error::from_win32());
            }
            let memory = CreateCompatibleDC(screen);
            if memory.is_invalid() {
                let _ = ReleaseDC(None, screen);
                return Err(windows::core::Error::from_win32());
            }

            let bitmap_info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = null_mut();
            let bitmap =
                match CreateDIBSection(screen, &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0) {
                    Ok(bitmap) => bitmap,
                    Err(err) => {
                        let _ = DeleteDC(memory);
                        let _ = ReleaseDC(None, screen);
                        return Err(err);
                    }
                };
            let old_bitmap = SelectObject(memory, HGDIOBJ(bitmap.0));

            Ok(Self {
                width,
                height,
                screen,
                memory,
                bitmap,
                old_bitmap,
                bits: bits.cast(),
            })
        }
    }

    fn present(&self, hwnd: HWND, pixmap: &Pixmap) {
        if pixmap.width() != self.width as u32 || pixmap.height() != self.height as u32 {
            return;
        }

        unsafe {
            let data = pixmap.data();
            let target = std::slice::from_raw_parts_mut(
                self.bits,
                (self.width as usize) * (self.height as usize) * 4,
            );
            for (dst, src) in target.chunks_exact_mut(4).zip(data.chunks_exact(4)) {
                dst[0] = src[2];
                dst[1] = src[1];
                dst[2] = src[0];
                dst[3] = src[3];
            }

            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let size = SIZE {
                cx: self.width,
                cy: self.height,
            };
            let src_point = POINT { x: 0, y: 0 };
            let _ = UpdateLayeredWindow(
                hwnd,
                self.screen,
                None,
                Some(&size),
                self.memory,
                Some(&src_point),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }
}

impl Drop for LayeredSurface {
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.memory, self.old_bitmap);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.memory);
            let _ = ReleaseDC(None, self.screen);
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
                app.toggle_mode();
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            match wparam.0 as i32 {
                HOTKEY_MODE => {
                    if let Some(app) = app_mut(hwnd) {
                        app.toggle_mode();
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
                let _ = UnregisterHotKey(hwnd, HOTKEY_MODE);
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
