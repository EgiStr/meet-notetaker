//! The app lives in the system tray. Closing the window only hides it, so a recording and the
//! notes keep going while the user works elsewhere; the tray menu opens the window again,
//! starts and stops the recording, and quits.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, Wry};
use tauri_plugin_autostart::ManagerExt;

/// Asks the window to start or stop recording; only it knows the title and template to start with.
const RECORD_EVENT: &str = "tray-record";
/// Asks the window to quit, after it has made sure a recording in progress is not lost.
const QUIT_EVENT: &str = "tray-quit";
/// Passed by the autostart entry, so that the app starts hidden in the tray.
pub const MINIMIZED_ARG: &str = "--minimized";
const IDLE_TOOLTIP: &str = "Meet Notetaker";

pub struct Tray {
    record: MenuItem<Wry>,
    icon: TrayIcon<Wry>,
}

pub fn setup(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Buka Meet Notetaker", true, None::<&str>)?;
    let record = MenuItem::with_id(app, "record", "Mulai merekam", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Keluar", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &record, &PredefinedMenuItem::separator(app)?, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip(IDLE_TOOLTIP)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_window(app),
            "record" => {
                let _ = app.emit(RECORD_EVENT, ());
            }
            "quit" => {
                let _ = app.emit(QUIT_EVENT, ());
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let icon = builder.build(app)?;
    app.manage(Tray { record, icon });

    if std::env::args().any(|argument| argument == MINIMIZED_ARG) {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.hide();
        }
    }
    Ok(())
}

pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    crate::desktop::show_waiting_summary(app);
}

/// Makes the tray menu and tooltip say whether a recording is running.
#[tauri::command]
pub fn set_tray_recording(recording: bool, tray: State<Tray>) -> Result<(), String> {
    tray.record.set_text(if recording { "Berhenti merekam" } else { "Mulai merekam" }).map_err(|error| error.to_string())?;
    let tooltip = if recording { "Meet Notetaker · sedang merekam" } else { IDLE_TOOLTIP };
    tray.icon.set_tooltip(Some(tooltip)).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|error| error.to_string())
}

/// Starts the app, hidden in the tray, when Windows starts.
#[tauri::command]
pub fn set_autostart(enabled: bool, app: AppHandle) -> Result<bool, String> {
    let launcher = app.autolaunch();
    if enabled {
        launcher.enable().map_err(|error| error.to_string())?;
    } else {
        launcher.disable().map_err(|error| error.to_string())?;
    }
    launcher.is_enabled().map_err(|error| error.to_string())
}
