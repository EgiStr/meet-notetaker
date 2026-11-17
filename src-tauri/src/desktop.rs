//! What makes the app useful while the window is hidden: a global hotkey to start and stop the
//! recording, desktop notifications, and noticing that an online meeting has opened.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tauri_plugin_notification::NotificationExt;
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, IsWindowVisible};

use crate::Meeting;

/// Same event as the tray's record item: the window owns the title and template to start with.
const RECORD_EVENT: &str = "tray-record";
const MEETING_EVENT: &str = "meeting-detected";
const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// A meeting window that is gone for this many polls counts as a new meeting when it returns.
const GONE_POLLS: u32 = 3;

pub const HOTKEY_LABEL: &str = "Ctrl+Alt+R";

fn hotkey() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyR)
}

/// The plugin that turns the hotkey into the record event.
pub fn hotkey_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if shortcut == &hotkey() && event.state == ShortcutState::Pressed {
                let _ = app.emit(RECORD_EVENT, ());
            }
        })
        .build()
}

pub fn setup(app: &tauri::App) {
    // Another program may already own the key; the tray menu still works then.
    if let Err(error) = app.global_shortcut().register(hotkey()) {
        eprintln!("hotkey {HOTKEY_LABEL} not registered: {error}");
    }
    let handle = app.handle().clone();
    std::thread::spawn(move || watch_for_meetings(&handle));
}

/// A native notification, for things the user should see while working in another window.
#[tauri::command]
pub fn notify(title: String, body: String, app: AppHandle) -> Result<(), String> {
    app.notification().builder().title(title).body(body).show().map_err(|error| error.to_string())
}

/// The online meeting a window title belongs to, if any.
fn meeting_in(title: &str) -> Option<&'static str> {
    let lower = title.to_lowercase();
    if lower.starts_with("meet - ") || lower.contains("- google meet") {
        Some("Google Meet")
    } else if lower.contains("zoom meeting") {
        Some("Zoom")
    } else if lower.contains("| microsoft teams") && (lower.contains("meeting") || lower.contains("call")) {
        Some("Microsoft Teams")
    } else {
        None
    }
}

fn visible_window_titles() -> Vec<String> {
    unsafe extern "system" fn collect(window: HWND, titles: LPARAM) -> BOOL {
        // SAFETY: `titles` is the address of the Vec that `EnumWindows` was called with below.
        let titles = unsafe { &mut *(titles as *mut Vec<String>) };
        if unsafe { IsWindowVisible(window) } != 0 {
            let mut buffer = [0u16; 256];
            let length = unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
            if length > 0 {
                titles.push(String::from_utf16_lossy(&buffer[..length as usize]));
            }
        }
        1
    }
    let mut titles: Vec<String> = Vec::new();
    // SAFETY: the callback only runs during this call, and `titles` outlives it.
    unsafe { EnumWindows(Some(collect), &mut titles as *mut Vec<String> as LPARAM) };
    titles
}

/// Tells the user once per meeting that one has opened and is not being recorded.
fn watch_for_meetings(app: &AppHandle) {
    let mut announced = false;
    let mut gone = 0;
    loop {
        std::thread::sleep(POLL_INTERVAL);
        let found = visible_window_titles().iter().find_map(|title| meeting_in(title));
        match found {
            Some(name) => {
                gone = 0;
                let recording = app.state::<Meeting>().state().session.is_some();
                if !announced && !recording {
                    announced = true;
                    let _ = app.emit(MEETING_EVENT, name);
                    let _ = app
                        .notification()
                        .builder()
                        .title(format!("{name} terdeteksi"))
                        .body(format!("Tekan {HOTKEY_LABEL} atau pilih Mulai merekam di ikon tray untuk mencatat rapat ini."))
                        .show();
                }
            }
            None => {
                gone += 1;
                if gone >= GONE_POLLS {
                    announced = false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meeting_windows_are_recognised_and_other_windows_are_not() {
        assert_eq!(meeting_in("Meet - abc-defg-hij - Google Chrome"), Some("Google Meet"));
        assert_eq!(meeting_in("Weekly sync - Google Meet - Microsoft Edge"), Some("Google Meet"));
        assert_eq!(meeting_in("Zoom Meeting"), Some("Zoom"));
        assert_eq!(meeting_in("Standup | Microsoft Teams meeting"), Some("Microsoft Teams"));
        assert_eq!(meeting_in("Chat | Microsoft Teams"), None);
        assert_eq!(meeting_in("Meet the team - Wikipedia"), None);
    }
}
