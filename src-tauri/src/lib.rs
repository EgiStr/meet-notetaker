//! Tauri shell: exposes the meeting commands and forwards events to the UI.
//! This is the composition root where concrete providers are chosen.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use notetaker_core::llm::{LlmClient, LlmError, Prompt};
use notetaker_core::notes::{MeetingNotes, PeriodicMom};
use notetaker_core::openai_llm::{OpenAiLlm, OpenAiLlmSettings};
use notetaker_core::openai_transcriber::{OpenAiSettings, OpenAiTranscriber};
use notetaker_core::session::{EventSink, Session, SessionEvent};
use notetaker_core::transcript::Source;
use tauri::async_runtime::spawn_blocking;
use tauri::{AppHandle, Emitter, State};

const SESSION_EVENT: &str = "session-event";
const NOTES_EVENT: &str = "notes-event";
const MOM_INTERVAL: Duration = Duration::from_secs(120);

#[derive(Default)]
struct Meeting(Mutex<MeetingState>);

#[derive(Default)]
struct MeetingState {
    session: Option<Session>,
    periodic_mom: Option<PeriodicMom>,
    /// Kept after the recording stops so that questions can still be asked.
    notes: Option<Arc<MeetingNotes>>,
}

impl Meeting {
    fn state(&self) -> MutexGuard<'_, MeetingState> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn notes(&self) -> Result<Arc<MeetingNotes>, String> {
        self.state().notes.clone().ok_or_else(|| "Belum ada rapat.".to_string())
    }
}

#[derive(Clone, serde::Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "lowercase")]
enum NotesEvent {
    Mom(String),
    /// The minutes could not be updated; the reason is shown as "MoM tertunda".
    Pending(String),
}

fn emit_mom(app: &AppHandle, result: Result<String, LlmError>) {
    let event = match result {
        Ok(mom) => NotesEvent::Mom(mom),
        Err(error) => NotesEvent::Pending(error.to_string()),
    };
    // Emitting only fails while the app is closing.
    let _ = app.emit(NOTES_EVENT, &event);
}

#[tauri::command]
fn start_recording(app: AppHandle, meeting: State<Meeting>) -> Result<(), String> {
    let mut state = meeting.state();
    if state.session.is_some() {
        return Err("Rekaman sudah berjalan.".into());
    }
    let transcriber = OpenAiTranscriber::new(stt_settings_from_env()?).map_err(|error| error.to_string())?;
    let notes = Arc::new(MeetingNotes::new(llm_from_env()));

    let sink: EventSink = {
        let (app, notes) = (app.clone(), notes.clone());
        Arc::new(move |event| {
            if let SessionEvent::Segment(segment) = &event {
                notes.add_segment(segment.clone());
            }
            let _ = app.emit(SESSION_EVENT, &event);
        })
    };
    state.session = Some(Session::start(
        &[Source::Microphone, Source::System],
        Arc::new(transcriber),
        sink,
    ));
    state.periodic_mom = Some(PeriodicMom::start(notes.clone(), MOM_INTERVAL, move |result| {
        emit_mom(&app, result)
    }));
    state.notes = Some(notes);
    Ok(())
}

/// Returns once the queued audio is transcribed and the final minutes were attempted.
#[tauri::command]
async fn stop_recording(app: AppHandle, meeting: State<'_, Meeting>) -> Result<(), String> {
    let (session, periodic_mom, notes) = {
        let mut state = meeting.state();
        (state.session.take(), state.periodic_mom.take(), state.notes.clone())
    };
    spawn_blocking(move || {
        if let Some(session) = session {
            session.stop();
        }
        if let Some(periodic_mom) = periodic_mom {
            periodic_mom.stop();
        }
        if let Some(notes) = notes {
            rebuild_and_emit(&app, &notes);
        }
    })
    .await
    .map_err(|error| error.to_string())
}

/// Retries the final minutes after a failure.
#[tauri::command]
async fn rebuild_mom(app: AppHandle, meeting: State<'_, Meeting>) -> Result<(), String> {
    let notes = meeting.notes()?;
    spawn_blocking(move || rebuild_and_emit(&app, &notes))
        .await
        .map_err(|error| error.to_string())
}

fn rebuild_and_emit(app: &AppHandle, notes: &MeetingNotes) {
    match notes.rebuild_mom() {
        Ok(Some(mom)) => emit_mom(app, Ok(mom)),
        Ok(None) => {}
        Err(error) => emit_mom(app, Err(error)),
    }
}

#[tauri::command]
fn set_user_notes(text: String, meeting: State<Meeting>) -> Result<(), String> {
    meeting.notes()?.set_user_notes(text);
    Ok(())
}

#[tauri::command]
async fn ask(question: String, meeting: State<'_, Meeting>) -> Result<String, String> {
    let notes = meeting.notes()?;
    spawn_blocking(move || notes.answer(&question))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

/// Provider settings come from the environment until the Settings screen exists.
fn stt_settings_from_env() -> Result<OpenAiSettings, String> {
    Ok(OpenAiSettings {
        base_url: env_or("STT_BASE_URL", "https://api.groq.com/openai/v1"),
        api_key: std::env::var("STT_API_KEY")
            .map_err(|_| "Variabel lingkungan STT_API_KEY belum diatur.".to_string())?,
        model: env_or("STT_MODEL", "whisper-large-v3-turbo"),
        language: env_or("STT_LANGUAGE", "id"),
    })
}

/// A missing or unusable LLM must not prevent recording, so it is reported
/// later through "MoM tertunda" instead of failing here.
fn llm_from_env() -> Arc<dyn LlmClient> {
    let Ok(api_key) = std::env::var("LLM_API_KEY") else {
        return Arc::new(UnavailableLlm("variabel lingkungan LLM_API_KEY belum diatur".into()));
    };
    let settings = OpenAiLlmSettings {
        base_url: env_or("LLM_BASE_URL", "https://generativelanguage.googleapis.com/v1beta/openai"),
        api_key,
        model: env_or("LLM_MODEL", "gemini-2.5-flash"),
    };
    match OpenAiLlm::new(settings) {
        Ok(llm) => Arc::new(llm),
        Err(error) => Arc::new(UnavailableLlm(error.to_string())),
    }
}

struct UnavailableLlm(String);

impl LlmClient for UnavailableLlm {
    fn complete(&self, _prompt: &Prompt) -> Result<String, LlmError> {
        Err(LlmError::NotConfigured(self.0.clone()))
    }
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.into())
}

pub fn run() {
    tauri::Builder::default()
        .manage(Meeting::default())
        .invoke_handler(tauri::generate_handler![
            start_recording,
            stop_recording,
            rebuild_mom,
            set_user_notes,
            ask
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the application");
}
