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
    if settings.remember_window_position {
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
