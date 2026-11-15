//! Tauri shell: exposes the meeting commands and forwards events to the UI.
//! This is the composition root where concrete providers are chosen.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use notetaker_core::llm::{LlmClient, LlmError, Prompt};
use notetaker_core::notes::{MeetingNotes, NotesSnapshot, PeriodicMom};
use notetaker_core::openai_llm::{OpenAiLlm, OpenAiLlmSettings};
use notetaker_core::openai_transcriber::{OpenAiSettings, OpenAiTranscriber};
use notetaker_core::polyvoice_diarizer::PolyvoiceDiarizer;
use notetaker_core::session::{recording_path, EventSink, Session, SessionEvent};
use notetaker_core::speakers::Diarizer;
use notetaker_core::store::{MeetingSummary, Store};
use notetaker_core::transcript::{Source, TranscriptSegment};
use tauri::async_runtime::spawn_blocking;
use tauri::{AppHandle, Emitter, Manager, State};

const SESSION_EVENT: &str = "session-event";
const NOTES_EVENT: &str = "notes-event";
const MOM_INTERVAL: Duration = Duration::from_secs(120);

/// The meeting database. SQLite connections are used from one thread at a time.
struct Db(Mutex<Store>);

impl Db {
    fn store(&self) -> MutexGuard<'_, Store> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[derive(Default)]
struct Meeting(Mutex<MeetingState>);

#[derive(Default)]
struct MeetingState {
    session: Option<Session>,
    periodic_mom: Option<PeriodicMom>,
    /// Kept after the recording stops so that questions can still be asked.
    notes: Option<Arc<MeetingNotes>>,
    /// The database row of the meeting that `notes` belongs to.
    meeting_id: Option<i64>,
    /// Where the audio of the current meeting is written.
    audio_dir: Option<PathBuf>,
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
    /// The whole transcript again, now with final speaker labels.
    Transcript(Vec<TranscriptSegment>),
    /// The minutes could not be updated; the reason is shown as "MoM tertunda".
    Pending(String),
}

fn emit_error(app: &AppHandle, error: impl std::fmt::Display) {
    let _ = app.emit(SESSION_EVENT, &SessionEvent::Error(error.to_string()));
}

/// Writes the current meeting to disk. Called whenever something worth keeping changed.
fn save_meeting(app: &AppHandle) {
    let (meeting_id, notes) = {
        let meeting = app.state::<Meeting>();
        let state = meeting.state();
        (state.meeting_id, state.notes.clone())
    };
    let (Some(meeting_id), Some(notes)) = (meeting_id, notes) else {
        return;
    };
    if let Err(error) = app.state::<Db>().store().save(meeting_id, &notes.snapshot()) {
        emit_error(app, error);
    }
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
fn start_recording(title: String, app: AppHandle, meeting: State<Meeting>, db: State<Db>) -> Result<(), String> {
    let mut state = meeting.state();
    if state.session.is_some() {
        return Err("Rekaman sudah berjalan.".into());
    }
    let transcriber = OpenAiTranscriber::new(stt_settings_from_env()?).map_err(|error| error.to_string())?;
    let notes = Arc::new(MeetingNotes::new(llm_from_env()));
    let started_at = unix_now()?;
    let audio_dir = new_audio_dir(&app, started_at)?;
    let title = match title.trim() {
        "" => "Rapat tanpa judul",
        trimmed => trimmed,
    };
    let meeting_id = db.store().create_meeting(title, started_at).map_err(|error| error.to_string())?;

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
        Some(audio_dir.clone()),
    ));
    state.audio_dir = Some(audio_dir);
    state.periodic_mom = Some(PeriodicMom::start(notes.clone(), MOM_INTERVAL, move |result| {
        emit_mom(&app, result);
        save_meeting(&app);
    }));
    state.notes = Some(notes);
    state.meeting_id = Some(meeting_id);
    Ok(())
}

/// One folder per meeting under the app data folder.
fn new_audio_dir(app: &AppHandle, started_at: i64) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("recordings")
        .join(started_at.to_string());
    std::fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    Ok(dir)
}

fn unix_now() -> Result<i64, String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?;
    Ok(now.as_secs() as i64)
}

/// Returns once the queued audio is transcribed, the speakers are labelled,
/// and the final minutes were attempted.
#[tauri::command]
async fn stop_recording(app: AppHandle, meeting: State<'_, Meeting>) -> Result<(), String> {
    let (session, periodic_mom, notes, audio_dir) = {
        let mut state = meeting.state();
        (state.session.take(), state.periodic_mom.take(), state.notes.clone(), state.audio_dir.clone())
    };
    spawn_blocking(move || {
        if let Some(session) = session {
            session.stop();
        }
        if let Some(periodic_mom) = periodic_mom {
            periodic_mom.stop();
        }
        if let Some(notes) = notes {
            if let Some(audio_dir) = audio_dir {
                label_speakers(&app, &notes, &PolyvoiceDiarizer, &audio_dir);
            }
            rebuild_and_emit(&app, &notes);
            save_meeting(&app);
        }
    })
    .await
    .map_err(|error| error.to_string())
}

/// Runs the final diarization on the system audio. A failure leaves the
/// provisional labels in place and is reported, but does not stop the minutes.
fn label_speakers(app: &AppHandle, notes: &MeetingNotes, diarizer: &dyn Diarizer, audio_dir: &Path) {
    if !notes.transcript().iter().any(|segment| segment.source == Source::System) {
        return;
    }
    match diarizer.diarize(&recording_path(audio_dir, Source::System)) {
        Ok(turns) => {
            notes.apply_speaker_turns(&turns);
            let _ = app.emit(NOTES_EVENT, &NotesEvent::Transcript(notes.transcript()));
        }
        Err(error) => emit_error(app, error),
    }
}

/// Applies to the transcript labels and to the minutes of the current meeting.
#[tauri::command]
fn rename_speaker(speaker: u32, name: String, app: AppHandle, meeting: State<Meeting>) -> Result<(), String> {
    let mom = meeting.notes()?.rename_speaker(speaker, &name);
    if !mom.is_empty() {
        emit_mom(&app, Ok(mom));
    }
    save_meeting(&app);
    Ok(())
}

/// Retries the final minutes after a failure.
#[tauri::command]
async fn rebuild_mom(app: AppHandle, meeting: State<'_, Meeting>) -> Result<(), String> {
    let notes = meeting.notes()?;
    spawn_blocking(move || {
        rebuild_and_emit(&app, &notes);
        save_meeting(&app);
    })
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
fn set_user_notes(text: String, app: AppHandle, meeting: State<Meeting>) -> Result<(), String> {
    meeting.notes()?.set_user_notes(text);
    save_meeting(&app);
    Ok(())
}

#[tauri::command]
async fn ask(question: String, app: AppHandle, meeting: State<'_, Meeting>) -> Result<String, String> {
    let notes = meeting.notes()?;
    spawn_blocking(move || {
        let answer = notes.answer(&question);
        save_meeting(&app);
        answer
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

/// Newest first; `query` filters by title and transcript text.
#[tauri::command]
fn list_meetings(query: String, db: State<Db>) -> Result<Vec<MeetingSummary>, String> {
    db.store().list(&query).map_err(|error| error.to_string())
}

#[derive(serde::Serialize)]
struct MeetingView {
    summary: MeetingSummary,
    notes: NotesSnapshot,
}

/// Makes a saved meeting the current one, so it can be read, asked about and edited.
#[tauri::command]
fn open_meeting(id: i64, meeting: State<Meeting>, db: State<Db>) -> Result<MeetingView, String> {
    let mut state = meeting.state();
    if state.session.is_some() {
        return Err("Hentikan rekaman sebelum membuka rapat lain.".into());
    }
    let (summary, notes) = db
        .store()
        .load(id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Rapat tidak ditemukan.".to_string())?;
    state.notes = Some(Arc::new(MeetingNotes::restore(llm_from_env(), notes.clone())));
    state.meeting_id = Some(id);
    state.audio_dir = None;
    Ok(MeetingView { summary, notes })
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
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            app.manage(Db(Mutex::new(Store::open(&data_dir.join("meetings.db"))?)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_recording,
            stop_recording,
            rebuild_mom,
            set_user_notes,
            rename_speaker,
            ask,
            list_meetings,
            open_meeting
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the application");
}
