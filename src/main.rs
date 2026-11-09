#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use launcher95::app::Launcher95App;
use launcher95::data::{load_settings, resolve_data_dir};

fn main() -> eframe::Result<()> {
    // A global-hotkey launcher should only have one instance. If another copy is
    // already running, bring its window to the front instead of starting a second one.
    #[cfg(windows)]
    if !acquire_single_instance() {
        focus_existing_window();
        return Ok(());
    }

    let data_dir = resolve_data_dir();
    let settings = load_settings(&data_dir);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Launcher95")
        .with_inner_size([settings.window_width, settings.window_height])
        .with_min_inner_size([460.0, 340.0])
        .with_decorations(false)
        .with_resizable(true);
    if settings.open_at_mouse {
        if let Some(position) = cursor_anchored_position(settings.window_width, settings.window_height)
        {
            viewport = viewport.with_position(position);
        }
    } else if settings.remember_window_position {
        if let (Some(x), Some(y)) = (settings.window_x, settings.window_y) {
            viewport = viewport.with_position([x, y]);
        }
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Launcher95",
        options,
        Box::new(move |cc| Ok(Box::new(Launcher95App::new(cc, data_dir)))),
    )
}

#[cfg(windows)]
/// Centers the window on the mouse pointer, keeping it inside the monitor's
/// work area. Returns egui points, which is what `ViewportBuilder` expects.
fn cursor_anchored_position(width: f32, height: f32) -> Option<[f32; 2]> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    unsafe {
        let mut cursor = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut cursor) == 0 {
            return None;
        }

        let dpi = GetDpiForSystem();
        let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };

        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        let work = info.rcWork;

        let width_px = (width * scale).round() as i32;
        let height_px = (height * scale).round() as i32;

        // Horizontally centered on the pointer, title bar just below it.
        let mut x = cursor.x - width_px / 2;
        let mut y = cursor.y - (24.0 * scale).round() as i32;
        let max_x = (work.right - width_px).max(work.left);
        let max_y = (work.bottom - height_px).max(work.top);
        x = x.clamp(work.left, max_x);
        y = y.clamp(work.top, max_y);

        Some([x as f32 / scale, y as f32 / scale])
    }
}

#[cfg(not(windows))]
fn cursor_anchored_position(_width: f32, _height: f32) -> Option<[f32; 2]> {
    None
}

#[cfg(windows)]
fn acquire_single_instance() -> bool {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    let name: Vec<u16> = "Local\\Launcher95.SingleInstance\0".encode_utf16().collect();
    unsafe {
        // The handle is intentionally kept alive for the lifetime of the process.
        let handle = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
        if handle.is_null() {
            return true;
        }
        GetLastError() != ERROR_ALREADY_EXISTS
    }
}

#[cfg(windows)]
fn focus_existing_window() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    let title: Vec<u16> = "Launcher95\0".encode_utf16().collect();
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if !hwnd.is_null() {
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
        }
    }
}
