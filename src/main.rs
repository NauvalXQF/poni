#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod geometry;
mod island;

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::null_mut;

use chrono::Local;
use fontdue::{Font, FontSettings};
use geometry::{geometry, IslandMode, WINDOW_HEIGHT, WINDOW_WIDTH};
use island::{draw as draw_island, IslandView};
use serde::{Deserialize, Serialize};
use tiny_skia::Pixmap;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW,
    MonitorFromPoint, ReleaseDC, ScreenToClient, SelectObject, UpdateWindow, AC_SRC_ALPHA,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HDC, HGDIOBJ,
    MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    GetWindowLongPtrW, KillTimer, LoadCursorW, PostQuitMessage, RegisterClassW, SetTimer,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, UpdateLayeredWindow,
    CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HTCLIENT, HTTRANSPARENT, HWND_TOPMOST,
    IDC_ARROW, MA_NOACTIVATE, MSG, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_SHOWNOACTIVATE, ULW_ALPHA,
    WM_CREATE, WM_DESTROY, WM_DPICHANGED, WM_HOTKEY, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
    WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST, WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

const WINDOW_CLASS: PCWSTR = w!("PoniIslandWindow");
const WINDOW_TITLE: PCWSTR = w!("Poni");
const HOTKEY_MODE: i32 = 1;
const HOTKEY_QUIT: i32 = 2;
const TIMER_ID: usize = 1;

#[derive(Debug, Serialize, Deserialize)]
struct Settings {
    mode: IslandMode,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: IslandMode::Notch,
        }
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
    window_width: i32,
    window_height: i32,
    surface: Option<LayeredSurface>,
    font: Option<Font>,
    time: String,
}

impl App {
    fn new() -> Self {
        let mode = load_settings().mode;
        Self {
            hwnd: HWND::default(),
            mode,
            mode_mix: mode.mix(),
            progress: 0.0,
            hovered: false,
            pinned: false,
            scale: 1.0,
            window_width: 0,
            window_height: 0,
            surface: None,
            font: load_font(),
            time: Local::now().format("%H:%M").to_string(),
        }
    }

    unsafe fn attach(&mut self, hwnd: HWND) {
        self.hwnd = hwnd;
        self.scale = dpi_scale(hwnd);
        self.resize_window();

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

        let _ = SetTimer(hwnd, TIMER_ID, 33, None);
        self.redraw();
    }

    unsafe fn resize_window(&mut self) {
        self.window_width = (WINDOW_WIDTH * self.scale).round() as i32;
        self.window_height = (WINDOW_HEIGHT * self.scale).round() as i32;

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
            self.scale,
            self.window_width as f32,
        )
    }

    fn hit_test(&self, px: f32, py: f32) -> bool {
        self.geometry().contains(px, py)
    }

    unsafe fn tick(&mut self) {
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
        let now = Local::now().format("%H:%M").to_string();

        let mut changed = false;
        if self.time != now {
            self.time = now;
            changed = true;
        }
        if approach(&mut self.progress, target, 0.22) {
            changed = true;
        }
        if approach(&mut self.mode_mix, target_mode, 0.18) {
            changed = true;
        }

        if changed {
            self.redraw();
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
            scale: self.scale,
            pinned: self.pinned,
            time: self.time.clone(),
            font: self.font.as_ref(),
        };

        if let Some(pixmap) = draw_island(surface.width as u32, surface.height as u32, &view) {
            surface.present(self.hwnd, &pixmap);
        }
        self.surface = Some(surface);
    }

    unsafe fn toggle_mode(&mut self) {
        self.mode = self.mode.toggle();
        save_settings(self.mode);
        let _ = SetTimer(self.hwnd, TIMER_ID, 24, None);
        self.redraw();
    }

    unsafe fn toggle_pin(&mut self) {
        self.pinned = !self.pinned;
        let _ = SetTimer(self.hwnd, TIMER_ID, 24, None);
    }
}

fn approach(current: &mut f32, target: f32, factor: f32) -> bool {
    let delta = target - *current;
    if delta.abs() < 0.002 {
        *current = target;
        return false;
    }
    *current += delta * factor;
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

fn save_settings(mode: IslandMode) {
    let Some(path) = settings_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(contents) = serde_json::to_string_pretty(&Settings { mode }) {
        let _ = std::fs::write(path, contents);
    }
}

fn load_font() -> Option<Font> {
    for name in ["segoeuib.ttf", "segoeui.ttf", "arialbd.ttf", "arial.ttf"] {
        let path = PathBuf::from(r"C:\Windows\Fonts").join(name);
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                return Some(font);
            }
        }
    }
    None
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
                let _ = SetTimer(hwnd, TIMER_ID, 24, None);
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
        WM_TIMER if wparam.0 == TIMER_ID => {
            if let Some(app) = app_mut(hwnd) {
                app.tick();
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
                app.resize_window();
                app.redraw();
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                let _ = KillTimer(hwnd, TIMER_ID);
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

        let app = Box::into_raw(Box::new(App::new()));
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
                log_line(&format!("CreateWindowExW failed: {err}"));
                return;
            }
        };

        let _ = UpdateWindow(hwnd);

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}
