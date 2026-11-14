//! Tauri shell: exposes start/stop commands and forwards session events to the UI.

use std::sync::{Arc, Mutex};

use notetaker_core::openai_transcriber::{OpenAiSettings, OpenAiTranscriber};
use notetaker_core::session::{EventSink, Session};
use notetaker_core::transcript::Source;
use tauri::{AppHandle, Emitter, State};

const SESSION_EVENT: &str = "session-event";

#[derive(Default)]
struct Recording(Mutex<Option<Session>>);

#[tauri::command]
fn start_recording(app: AppHandle, recording: State<Recording>) -> Result<(), String> {
    let mut active = recording.0.lock().map_err(|error| error.to_string())?;
    if active.is_some() {
        return Err("Rekaman sudah berjalan.".into());
    }
    let transcriber = OpenAiTranscriber::new(settings_from_env()?).map_err(|error| error.to_string())?;
    let sink: EventSink = Arc::new(move |event| {
        // Emitting only fails while the app is closing.
        let _ = app.emit(SESSION_EVENT, &event);
    });
    *active = Some(Session::start(
        &[Source::Microphone, Source::System],
        Arc::new(transcriber),
        sink,
    ));
    Ok(())
}

/// Returns once the audio still queued has been transcribed.
#[tauri::command(async)]
fn stop_recording(recording: State<Recording>) -> Result<(), String> {
    let session = recording.0.lock().map_err(|error| error.to_string())?.take();
    if let Some(session) = session {
        session.stop();
    }
    Ok(())
}

/// Provider settings come from the environment until the Settings screen exists.
fn settings_from_env() -> Result<OpenAiSettings, String> {
    Ok(OpenAiSettings {
        base_url: env_or("STT_BASE_URL", "https://api.groq.com/openai/v1"),
        api_key: std::env::var("STT_API_KEY")
            .map_err(|_| "Variabel lingkungan STT_API_KEY belum diatur.".to_string())?,
        model: env_or("STT_MODEL", "whisper-large-v3-turbo"),
        language: env_or("STT_LANGUAGE", "id"),
    })
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.into())
}

pub fn run() {
    tauri::Builder::default()
        .manage(Recording::default())
        .invoke_handler(tauri::generate_handler![start_recording, stop_recording])
        .run(tauri::generate_context!())
        .expect("failed to start the application");
}
