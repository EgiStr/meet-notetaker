//! Tauri shell: exposes the meeting commands and forwards events to the UI.
//! This is the composition root where concrete providers are chosen.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use notetaker_core::audio::WavRecorder;
use notetaker_core::deepgram_transcriber::{DeepgramSettings, DeepgramTranscriber};
use notetaker_core::export;
use notetaker_core::report;
use notetaker_core::import::{load_recording, refine_stream, transcribe_recording};
use notetaker_core::library;
use notetaker_core::llm::LlmError;
use notetaker_core::live_speakers::LiveSpeakers;
use notetaker_core::notes::{Bookmark, LiveBrief, MeetingNotes, NotesSnapshot, PeriodicMom};
use notetaker_core::openai_transcriber::{OpenAiSettings, OpenAiTranscriber};
use notetaker_core::compress::compress_recordings;
use notetaker_core::polyvoice_diarizer::PolyvoiceDiarizer;
use notetaker_core::prompts::MomTemplate;
use notetaker_core::semantic;
use notetaker_core::session::{recording_path, EventSink, Session, SessionEvent};
use notetaker_core::speakers::Diarizer;
use notetaker_core::store::{MeetingSummary, Store};
use notetaker_core::cleanup::{without_speaker_bleed, CleaningTranscriber};
use notetaker_core::transcript::{Source, Transcriber, TranscriptSegment};
use tauri::async_runtime::spawn_blocking;
use tauri::{AppHandle, Emitter, Manager, State};

mod models;
mod stt;
mod desktop;
mod tray;

const SESSION_EVENT: &str = "session-event";
const NOTES_EVENT: &str = "notes-event";
/// How often the voices heard so far are told apart again during a recording.
const SPEAKERS_INTERVAL: Duration = Duration::from_secs(45);
const MOM_INTERVAL: Duration = Duration::from_secs(120);
/// How often the live brief is refreshed and how far back it looks; with the minutes and the
/// question suggestions this stays under the free Gemini limit of 5 requests a minute.
const BRIEF_INTERVAL: Duration = Duration::from_secs(60);
const BRIEF_WINDOW_SECS: f64 = 300.0;
const DEFAULT_TITLE: &str = "Rapat tanpa judul";
/// Passages found by meaning that go into the answer to a question, and that are listed in the history search.
const SEMANTIC_QUOTES: usize = 8;
const SEMANTIC_RESULTS: usize = 30;
const SNIPPET_CHARS: usize = 220;

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
    live_brief: Option<LiveBrief>,
    live_speakers: Option<LiveSpeakers>,
    /// The running recording was started without a title and gets one when it ends.
    untitled: bool,
    /// Kept after the recording stops so that questions can still be asked.
    notes: Option<Arc<MeetingNotes>>,
    /// The database row of the meeting that `notes` belongs to.
    meeting_id: Option<i64>,
    /// Where the audio of the current meeting is written.
    audio_dir: Option<PathBuf>,
    /// When the running recording started; bookmarks are placed on its timeline.
    started: Option<Instant>,
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
    /// Questions worth asking the assistant about the meeting so far.
    Questions(Vec<String>),
    /// The latest recap of what is being discussed, refreshed while recording.
    Brief(String),
    /// The title chosen for a meeting that was started without one.
    Title(String),
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

/// A failure is not reported: the suggestions are a convenience, and the minutes already show LLM problems.
fn emit_questions(app: &AppHandle, notes: &MeetingNotes) {
    if let Ok(questions) = notes.suggest_questions() {
        if !questions.is_empty() {
            let _ = app.emit(NOTES_EVENT, &NotesEvent::Questions(questions));
        }
    }
}

#[tauri::command]
fn start_recording(
    title: String,
    template: Option<String>,
    app: AppHandle,
    meeting: State<Meeting>,
    db: State<Db>,
) -> Result<(), String> {
    let mut state = meeting.state();
    if state.session.is_some() {
        return Err("Rekaman sudah berjalan.".into());
    }
    let template = parse_template(template.as_deref().unwrap_or("general"))?;
    let transcriber = transcriber_from_env(&app)?;
    let notes = Arc::new(MeetingNotes::new(models::llm(&app)));
    notes.set_template(template);
    let started_at = unix_now()?;
    let audio_dir = new_audio_dir(&app, started_at)?;
    let untitled = title.trim().is_empty();
    let title = if untitled { DEFAULT_TITLE } else { title.trim() };
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
        transcriber,
        sink,
        Some(audio_dir.clone()),
    ));
    state.audio_dir = Some(audio_dir);
    state.started = Some(Instant::now());
    let (suggesting, brief_app, speakers_app) = (notes.clone(), app.clone(), app.clone());
    state.periodic_mom = Some(PeriodicMom::start(notes.clone(), MOM_INTERVAL, move |result| {
        let updated = result.is_ok();
        emit_mom(&app, result);
        save_meeting(&app);
        if updated {
            emit_questions(&app, &suggesting);
        }
    }));
    state.live_brief = Some(LiveBrief::start(notes.clone(), BRIEF_INTERVAL, BRIEF_WINDOW_SECS, move |result| {
        // A failure is not shown: the minutes already report LLM problems, and the next round tries again.
        if let Ok(brief) = result {
            let _ = brief_app.emit(NOTES_EVENT, &NotesEvent::Brief(brief));
        }
    }));
    let speaker_notes = notes.clone();
    state.live_speakers = state.audio_dir.as_ref().map(|dir| {
        LiveSpeakers::start(
            notes.clone(),
            recording_path(dir, Source::System),
            Arc::new(PolyvoiceDiarizer),
            SPEAKERS_INTERVAL,
            move || {
                let _ = speakers_app.emit(NOTES_EVENT, &NotesEvent::Transcript(speaker_notes.transcript()));
            },
        )
    });
    state.notes = Some(notes);
    state.meeting_id = Some(meeting_id);
    state.untitled = untitled;
    Ok(())
}

/// Gives a meeting that was started without a title one from its minutes. A failure keeps the placeholder.
fn auto_title(app: &AppHandle, notes: &MeetingNotes, meeting_id: i64) {
    let Ok(Some(title)) = notes.suggest_title() else {
        return;
    };
    if matches!(app.state::<Db>().store().rename(meeting_id, &title), Ok(true)) {
        let _ = app.emit(NOTES_EVENT, &NotesEvent::Title(title));
    }
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
    let (session, periodic_mom, live_brief, live_speakers, notes, audio_dir, untitled_id) = {
        let mut state = meeting.state();
        state.started = None;
        let untitled_id = if std::mem::take(&mut state.untitled) { state.meeting_id } else { None };
        (
            state.session.take(),
            state.periodic_mom.take(),
            state.live_brief.take(),
            state.live_speakers.take(),
            state.notes.clone(),
            state.audio_dir.clone(),
            untitled_id,
        )
    };
    spawn_blocking(move || {
        if let Some(session) = session {
            session.stop();
        }
        if let Some(periodic_mom) = periodic_mom {
            periodic_mom.stop();
        }
        if let Some(live_brief) = live_brief {
            live_brief.stop();
        }
        if let Some(live_speakers) = live_speakers {
            live_speakers.stop();
        }
        if let Some(notes) = notes {
            if let Some(audio_dir) = audio_dir {
                refine_transcript(&app, &notes, &audio_dir);
                label_speakers(&app, &notes, &PolyvoiceDiarizer, &audio_dir);
                compress_audio(&app, &audio_dir);
            }
            rebuild_and_emit(&app, &notes);
            save_meeting(&app);
            if let Some(meeting_id) = untitled_id {
                auto_title(&app, &notes, meeting_id);
            }
            collect_actions(&app, &notes, true);
            announce_finished(&app, &notes);
            index_in_background(&app);
        }
    })
    .await
    .map_err(|error| error.to_string())
}

/// Tells the voices of the current (finished) meeting apart again, for meetings recorded before
/// the labels were reliable. The recording is kept, so this can be repeated.
#[tauri::command]
async fn relabel_speakers(app: AppHandle, meeting: State<'_, Meeting>) -> Result<(), String> {
    let (notes, audio_dir) = {
        let state = meeting.state();
        if state.session.is_some() {
            return Err("Hentikan rekaman dulu.".into());
        }
        (state.notes.clone(), state.audio_dir.clone())
    };
    let (Some(notes), Some(audio_dir)) = (notes, audio_dir) else {
        return Err("Belum ada rapat.".into());
    };
    spawn_blocking(move || {
        label_speakers(&app, &notes, &PolyvoiceDiarizer, &audio_dir);
        save_meeting(&app);
    })
    .await
    .map_err(|error| error.to_string())
}

/// Notifies the user that the minutes of the current meeting are ready.
fn announce_finished(app: &AppHandle, notes: &MeetingNotes) {
    let Some(id) = app.state::<Meeting>().state().meeting_id else { return };
    let title = match app.state::<Db>().store().load(id) {
        Ok(Some((summary, _))) if !summary.title.trim().is_empty() => summary.title,
        _ => "Rapat".to_owned(),
    };
    desktop::announce_summary(app, &title, &notes.snapshot().mom);
}

/// Processes a recording file like a meeting that just ended: transcript,
/// speaker labels, minutes. The result becomes the current meeting.
///
/// Everything runs on a blocking thread: the providers own blocking HTTP
/// clients, which must not be created or dropped on the async runtime.
#[tauri::command]
async fn import_recording(path: String, app: AppHandle) -> Result<(), String> {
    spawn_blocking(move || import_blocking(&app, &PathBuf::from(path)))
        .await
        .map_err(|error| error.to_string())?
}

fn import_blocking(app: &AppHandle, path: &Path) -> Result<(), String> {
    if app.state::<Meeting>().state().session.is_some() {
        return Err("Hentikan rekaman sebelum mengimpor file.".into());
    }
    // Decode first, so that an unreadable file leaves no empty meeting behind.
    let samples = load_recording(path).map_err(|error| error.to_string())?;
    let transcriber = transcriber_from_env(app)?;
    let notes = Arc::new(MeetingNotes::new(models::llm(app)));

    let started_at = unix_now()?;
    let audio_dir = new_audio_dir(app, started_at)?;
    // The diarizer reads the same file a live session would have written.
    let recording = recording_path(&audio_dir, Source::System);
    WavRecorder::create(&recording)
        .and_then(|mut recorder| recorder.write(&samples).and_then(|()| recorder.finish()))
        .map_err(|error| format!("{}: {error}", recording.display()))?;

    let title = path.file_stem().map_or("Rekaman impor".into(), |stem| stem.to_string_lossy().into_owned());
    let meeting_id = app.state::<Db>().store().create_meeting(&title, started_at).map_err(|error| error.to_string())?;
    {
        let meeting = app.state::<Meeting>();
        let mut state = meeting.state();
        state.notes = Some(notes.clone());
        state.meeting_id = Some(meeting_id);
        state.audio_dir = Some(audio_dir.clone());
    }

    let transcribed = transcribe_recording(&samples, transcriber.as_ref(), &std::thread::sleep, &mut |segment| {
        notes.add_segment(segment.clone());
        let _ = app.emit(SESSION_EVENT, &SessionEvent::Segment(segment));
    });
    // What was transcribed before a failure is still labelled, summarized and kept.
    label_speakers(app, &notes, &PolyvoiceDiarizer, &audio_dir);
    compress_audio(app, &audio_dir);
    rebuild_and_emit(app, &notes);
    save_meeting(app);
    collect_actions(app, &notes, true);
    announce_finished(app, &notes);
    index_in_background(app);
    transcribed.map_err(|error| error.to_string())
}

/// Transcribes each finished stream again, in long pieces with the stronger model, and uses that
/// instead of the live text, which is cut into short pieces to appear quickly. A failure keeps the
/// live transcript and is reported.
fn refine_transcript(app: &AppHandle, notes: &MeetingNotes, audio_dir: &Path) {
    let transcriber = match build_transcriber(app, true) {
        Ok(transcriber) => transcriber,
        Err(error) => return emit_error(app, error),
    };
    for source in [Source::System, Source::Microphone] {
        let path = recording_path(audio_dir, source);
        let live = notes.transcript().iter().any(|segment| segment.source == source);
        if !live || !path.exists() {
            continue;
        }
        let outcome = load_recording(&path)
            .map_err(|error| error.to_string())
            .and_then(|samples| {
                refine_stream(&samples, source, transcriber.as_ref(), &std::thread::sleep).map_err(|error| error.to_string())
            });
        match outcome {
            // Nothing found in the system audio means the second pass failed to hear it, not silence.
            Ok(segments) if source == Source::System && segments.is_empty() => {}
            Ok(segments) => {
                let segments = if source == Source::Microphone {
                    let system: Vec<_> = notes.transcript().into_iter().filter(|s| s.source == Source::System).collect();
                    without_speaker_bleed(segments, &system)
                } else {
                    segments
                };
                notes.replace_segments(source, segments);
                let _ = app.emit(NOTES_EVENT, &NotesEvent::Transcript(notes.transcript()));
            }
            Err(error) => emit_error(app, error),
        }
    }
}

/// Runs the final diarization on the system audio. A failure leaves the
/// provisional labels in place and is reported, but does not stop the minutes.
fn label_speakers(app: &AppHandle, notes: &MeetingNotes, diarizer: &dyn Diarizer, audio_dir: &Path) {
    if !notes.transcript().iter().any(|segment| segment.source == Source::System) {
        return;
    }
    match diarizer.diarize(&system_recording(audio_dir)) {
        Ok(turns) => {
            notes.apply_speaker_turns(&turns);
            recognize_voices(app, notes, audio_dir);
            let _ = app.emit(NOTES_EVENT, &NotesEvent::Transcript(notes.transcript()));
        }
        Err(error) => emit_error(app, error),
    }
}

/// After the meeting the recording has been compressed to FLAC.
fn system_recording(audio_dir: &Path) -> PathBuf {
    let wav = recording_path(audio_dir, Source::System);
    if wav.exists() { wav } else { wav.with_extension("flac") }
}

/// Gives the voices that were named in earlier meetings their names. A name the user already
/// gave in this meeting is kept. A failure only means the voices stay "Pembicara n".
fn recognize_voices(app: &AppHandle, notes: &MeetingNotes, audio_dir: &Path) {
    let known = match app.state::<Db>().store().voiceprints() {
        Ok(known) if !known.is_empty() => known,
        _ => return,
    };
    let Some(prints) = voice_prints(notes, audio_dir) else { return };
    let snapshot = notes.snapshot();
    for (speaker, name) in notetaker_core::voiceprint::recognize(&prints, &known) {
        if snapshot.names.given().all(|(given, _)| given != speaker) {
            notes.rename_speaker(speaker, &name);
        }
    }
}

/// The voiceprint of each numbered voice of the current meeting.
fn voice_prints(notes: &MeetingNotes, audio_dir: &Path) -> Option<std::collections::BTreeMap<u32, Vec<f32>>> {
    let embedder = notetaker_core::voiceprint::Resnet::load().ok()?;
    let samples = notetaker_core::audio_file::read_samples(&system_recording(audio_dir)).ok()?;
    let prints =
        notetaker_core::voiceprint::speaker_prints(&embedder, &samples, &notes.transcript(), notetaker_core::audio::SAMPLE_RATE);
    (!prints.is_empty()).then_some(prints)
}

/// Shrinks the finished recordings to FLAC. A failure leaves the WAV files in place and is reported.
fn compress_audio(app: &AppHandle, audio_dir: &Path) {
    if let Err(error) = compress_recordings(audio_dir) {
        emit_error(app, error);
    }
}

/// Applies to the transcript labels and to the minutes of the current meeting.
#[tauri::command]
fn rename_speaker(speaker: u32, name: String, app: AppHandle, meeting: State<Meeting>) -> Result<(), String> {
    let notes = meeting.notes()?;
    let mom = notes.rename_speaker(speaker, &name);
    if !mom.is_empty() {
        emit_mom(&app, Ok(mom));
    }
    save_meeting(&app);
    // The voice is remembered so that the next meeting names it by itself.
    let audio_dir = meeting.state().audio_dir.clone();
    let name = name.trim().to_owned();
    if let (Some(audio_dir), false) = (audio_dir, name.is_empty()) {
        std::thread::spawn(move || {
            if let Some(print) = voice_prints(&notes, &audio_dir).and_then(|mut prints| prints.remove(&speaker)) {
                let _ = app.state::<Db>().store().remember_voice(&name, &print);
            }
        });
    }
    Ok(())
}

/// Corrects the words of a transcript line; `source` is "microphone" or "system".
#[tauri::command]
fn edit_segment(source: String, start_secs: f64, text: String, app: AppHandle, meeting: State<Meeting>) -> Result<(), String> {
    let source = match source.as_str() {
        "microphone" => Source::Microphone,
        "system" => Source::System,
        other => return Err(format!("Sumber \"{other}\" tidak dikenal.")),
    };
    if meeting.notes()?.edit_segment(source, start_secs, &text) {
        save_meeting(&app);
    }
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

fn parse_template(id: &str) -> Result<MomTemplate, String> {
    MomTemplate::from_id(id).ok_or_else(|| format!("Template \"{id}\" tidak dikenal."))
}

/// Applies from the next update of the minutes.
#[tauri::command]
fn set_template(template: String, app: AppHandle, meeting: State<Meeting>) -> Result<(), String> {
    meeting.notes()?.set_template(parse_template(&template)?);
    save_meeting(&app);
    Ok(())
}

/// Marks a moment of the meeting. Without `at_secs` it is the moment the running recording has reached.
#[tauri::command]
fn add_bookmark(
    label: String,
    at_secs: Option<f64>,
    app: AppHandle,
    meeting: State<Meeting>,
) -> Result<Vec<Bookmark>, String> {
    let (notes, started) = {
        let state = meeting.state();
        (state.notes.clone(), state.started)
    };
    let notes = notes.ok_or_else(|| "Belum ada rapat.".to_string())?;
    let at_secs = match (at_secs, started) {
        (Some(at_secs), _) => at_secs,
        (None, Some(started)) => started.elapsed().as_secs_f64(),
        (None, None) => return Err("Penanda hanya bisa dibuat saat merekam, atau dari baris transkrip.".into()),
    };
    let bookmarks = notes.add_bookmark(at_secs, &label);
    save_meeting(&app);
    Ok(bookmarks)
}

#[tauri::command]
fn remove_bookmark(index: usize, app: AppHandle, meeting: State<Meeting>) -> Result<Vec<Bookmark>, String> {
    let bookmarks = meeting.notes()?.remove_bookmark(index);
    save_meeting(&app);
    Ok(bookmarks)
}

/// The user's short notes written out with the transcript. `None` while there are no notes.
#[tauri::command]
async fn expand_notes(meeting: State<'_, Meeting>) -> Result<Option<String>, String> {
    let notes = meeting.notes()?;
    spawn_blocking(move || notes.expand_notes())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn suggest_questions(meeting: State<'_, Meeting>) -> Result<Vec<String>, String> {
    let notes = meeting.notes()?;
    spawn_blocking(move || notes.suggest_questions())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

/// A recap of the last `minutes` of the meeting, for someone who just joined or looked away.
#[tauri::command]
async fn catch_up(minutes: u32, meeting: State<'_, Meeting>) -> Result<Option<String>, String> {
    let notes = meeting.notes()?;
    spawn_blocking(move || notes.catch_up(f64::from(minutes.clamp(1, 60)) * 60.0))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

/// A follow-up email for the participants, from the minutes and the transcript.
#[tauri::command]
async fn follow_up_email(meeting: State<'_, Meeting>) -> Result<Option<String>, String> {
    let notes = meeting.notes()?;
    spawn_blocking(move || notes.follow_up_email())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

/// Explains one transcript line using the conversation around it.
#[tauri::command]
async fn explain_line(text: String, at_secs: f64, meeting: State<'_, Meeting>) -> Result<String, String> {
    let notes = meeting.notes()?;
    spawn_blocking(move || notes.explain(&text, at_secs))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[derive(serde::Serialize)]
struct LibraryAnswer {
    answer: String,
    sources: Vec<library::Source>,
}

/// Answers a question from all saved meetings. `utc_offset_secs` is the local time zone, for dates.
#[tauri::command]
async fn ask_library(question: String, utc_offset_secs: i64, app: AppHandle) -> Result<LibraryAnswer, String> {
    spawn_blocking(move || {
        // Without embeddings the question is still answered, from the words in it.
        let passages = models::embedder(&app)
            .map_err(|error| error.to_string())
            .and_then(|embedder| semantic::search(&app.state::<Db>().0, &embedder, &question, SEMANTIC_QUOTES).map_err(|error| error.to_string()))
            .unwrap_or_default();
        let prepared = library::prepare(&app.state::<Db>().store(), &question, utc_offset_secs, &passages)
            .map_err(|error| error.to_string())?;
        let Some(prepared) = prepared else {
            return Ok(LibraryAnswer { answer: "Belum ada rapat tersimpan yang bisa dicari.".into(), sources: Vec::new() });
        };
        let answer = models::llm(&app).complete(&prepared.prompt).map_err(|error| error.to_string())?;
        Ok(LibraryAnswer { answer, sources: prepared.sources })
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Writes the current meeting to `path` as Markdown.
#[tauri::command]
fn export_meeting(path: String, utc_offset_secs: i64, meeting: State<Meeting>, db: State<Db>) -> Result<(), String> {
    let (notes, id) = {
        let state = meeting.state();
        (state.notes.clone(), state.meeting_id)
    };
    let (Some(notes), Some(id)) = (notes, id) else {
        return Err("Belum ada rapat.".into());
    };
    let (summary, _) = db
        .store()
        .load(id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Rapat tidak ditemukan.".to_string())?;
    let snapshot = notes.snapshot();
    // The file type decides what is written: the report, the raw transcript, or all raw data.
    let extension = Path::new(&path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let content = match extension.as_str() {
        "html" | "htm" => report::report_html(&summary, &snapshot, utc_offset_secs),
        "txt" => report::transcript_text(&summary, &snapshot, utc_offset_secs),
        "json" => report::raw_json(&summary, &snapshot, utc_offset_secs),
        _ => export::markdown(&summary, &snapshot, utc_offset_secs),
    };
    std::fs::write(&path, content).map_err(|error| format!("{path}: {error}"))
}

/// Writes the stretch of the current meeting around a bookmark as one HTML file with the audio
/// (when the recording is still there) and the words.
#[tauri::command]
fn export_clip(index: usize, path: String, meeting: State<Meeting>, db: State<Db>) -> Result<(), String> {
    use notetaker_core::clip;
    let (notes, id, audio_dir) = {
        let state = meeting.state();
        (state.notes.clone(), state.meeting_id, state.audio_dir.clone())
    };
    let (Some(notes), Some(id)) = (notes, id) else {
        return Err("Belum ada rapat.".into());
    };
    let snapshot = notes.snapshot();
    let bookmark = snapshot.bookmarks.get(index).ok_or("Penanda tidak ditemukan.")?;
    let title = match db.store().load(id) {
        Ok(Some((summary, _))) => summary.title,
        _ => String::new(),
    };
    let duration = snapshot.segments.iter().map(|segment| segment.end_secs).fold(0.0, f64::max);
    let (start, end) = clip::range(bookmark.at_secs, duration);

    let rate = notetaker_core::audio::SAMPLE_RATE;
    let wav = audio_dir.as_deref().and_then(|dir| {
        let stream = |source: Source| {
            let wav = recording_path(dir, source);
            let file = if wav.exists() { wav } else { wav.with_extension("flac") };
            notetaker_core::audio_file::read_samples(&file).ok()
        };
        let (system, microphone) = (stream(Source::System), stream(Source::Microphone));
        if system.is_none() && microphone.is_none() {
            return None;
        }
        let piece = |samples: Option<Vec<f32>>| {
            let samples = samples.unwrap_or_default();
            let from = ((start * f64::from(rate)) as usize).min(samples.len());
            let to = ((end * f64::from(rate)) as usize).min(samples.len());
            samples[from..to].to_vec()
        };
        clip::wav_bytes(&clip::mix(&piece(system), &piece(microphone)), rate).ok()
    });
    let html = clip::clip_html(&title, &bookmark.label, start, end, &snapshot.segments, &snapshot.names, wav.as_deref());
    std::fs::write(&path, html).map_err(|error| format!("{path}: {error}"))
}

const WEBHOOK_SETTING: &str = "webhook_url";
const CUSTOM_TEMPLATE_SETTING: &str = "custom_template";

/// Makes the saved custom template the one the prompts use. Called once at start-up.
fn load_custom_template(store: &Store) {
    let saved = store.setting(CUSTOM_TEMPLATE_SETTING).ok().flatten();
    notetaker_core::prompts::set_custom_template(saved.and_then(|json| serde_json::from_str(&json).ok()));
}

/// The template the user wrote; empty before they wrote one. `sections` holds one heading per line.
#[derive(serde::Serialize, serde::Deserialize)]
struct CustomTemplateForm {
    sections: String,
    instructions: String,
}

#[tauri::command]
fn get_custom_template() -> CustomTemplateForm {
    let template = notetaker_core::prompts::custom_template().unwrap_or_default();
    CustomTemplateForm { sections: template.sections.join("
"), instructions: template.instructions }
}

/// Applies from the next update or rebuild of minutes written with the custom template.
#[tauri::command]
fn set_custom_template(form: CustomTemplateForm, db: State<Db>) -> Result<(), String> {
    use notetaker_core::prompts::{set_custom_template, CustomTemplate};
    let template = CustomTemplate {
        sections: CustomTemplate::sections_from_text(&form.sections),
        instructions: form.instructions.trim().to_owned(),
    };
    let json = if template == CustomTemplate::default() {
        String::new()
    } else {
        serde_json::to_string(&template).map_err(|error| error.to_string())?
    };
    db.store().set_setting(CUSTOM_TEMPLATE_SETTING, &json).map_err(|error| error.to_string())?;
    set_custom_template((!json.is_empty()).then_some(template));
    Ok(())
}

/// Finds the action items of the current meeting and keeps them, replacing what was kept for it.
/// With `send`, they also go to the webhook when one is set. A failure is reported and leaves
/// the meeting as it was.
fn collect_actions(app: &AppHandle, notes: &MeetingNotes, send: bool) {
    let Some(id) = app.state::<Meeting>().state().meeting_id else { return };
    let items = match notes.action_items() {
        Ok(Some(items)) => items,
        Ok(None) => return,
        Err(error) => return emit_error(app, error),
    };
    let db = app.state::<Db>();
    if let Err(error) = db.store().replace_action_items(id, &items) {
        return emit_error(app, error);
    }
    if send {
        send_to_webhook(app, id, &items);
    }
}

fn send_to_webhook(app: &AppHandle, meeting_id: i64, items: &[notetaker_core::actions::ActionDraft]) {
    let db = app.state::<Db>();
    let url = db.store().setting(WEBHOOK_SETTING).ok().flatten();
    let Some(url) = url else { return };
    let title = match db.store().load(meeting_id) {
        Ok(Some((summary, _))) => summary.title,
        _ => String::new(),
    };
    if let Err(error) = notetaker_core::actions::send_webhook(&url, &title, items) {
        emit_error(app, format!("Webhook: {error}"));
    }
}

/// Reads the action items of the current meeting again from its transcript.
#[tauri::command]
async fn extract_actions(app: AppHandle, meeting: State<'_, Meeting>) -> Result<(), String> {
    let notes = meeting.notes()?;
    spawn_blocking(move || collect_actions(&app, &notes, false)).await.map_err(|error| error.to_string())
}

/// Sends the kept action items of the current meeting to the webhook.
#[tauri::command]
async fn send_actions(app: AppHandle, meeting: State<'_, Meeting>) -> Result<(), String> {
    let id = meeting.state().meeting_id.ok_or("Belum ada rapat.")?;
    spawn_blocking(move || {
        let db = app.state::<Db>();
        let url = db.store().setting(WEBHOOK_SETTING).map_err(|e| e.to_string())?.ok_or("Isi alamat webhook dulu.")?;
        let items: Vec<_> = db
            .store()
            .action_items(false)
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|item| item.meeting_id == id)
            .map(|item| notetaker_core::actions::ActionDraft { task: item.task, owner: item.owner, due: item.due })
            .collect();
        let title = match db.store().load(id) {
            Ok(Some((summary, _))) => summary.title,
            _ => String::new(),
        };
        notetaker_core::actions::send_webhook(&url, &title, &items)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Action items across all meetings, open ones first.
#[tauri::command]
fn list_actions(include_done: bool, db: State<Db>) -> Result<Vec<notetaker_core::actions::ActionItem>, String> {
    db.store().action_items(include_done).map_err(|error| error.to_string())
}

#[tauri::command]
fn set_action_done(id: i64, done: bool, db: State<Db>) -> Result<(), String> {
    db.store().set_action_done(id, done).map(|_| ()).map_err(|error| error.to_string())
}

#[tauri::command]
fn get_webhook(db: State<Db>) -> Result<String, String> {
    db.store().setting(WEBHOOK_SETTING).map(Option::unwrap_or_default).map_err(|error| error.to_string())
}

/// A blank address turns the webhook off.
#[tauri::command]
fn set_webhook(url: String, db: State<Db>) -> Result<(), String> {
    let url = url.trim();
    if !url.is_empty() && !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Alamat webhook harus diawali https://".into());
    }
    db.store().set_setting(WEBHOOK_SETTING, url).map_err(|error| error.to_string())
}

/// Talk time and habits of each speaker of the current meeting.
#[tauri::command]
fn talk_stats(meeting: State<Meeting>) -> Result<Vec<notetaker_core::analytics::TalkStat>, String> {
    let snapshot = meeting.notes()?.snapshot();
    Ok(notetaker_core::analytics::talk_stats(&snapshot.segments, &snapshot.names))
}

/// The current meeting as the report document, for printing to PDF from the window.
#[tauri::command]
fn report_html(utc_offset_secs: i64, meeting: State<Meeting>, db: State<Db>) -> Result<String, String> {
    let (notes, id) = {
        let state = meeting.state();
        (state.notes.clone(), state.meeting_id)
    };
    let (Some(notes), Some(id)) = (notes, id) else {
        return Err("Belum ada rapat.".into());
    };
    let (summary, _) = db
        .store()
        .load(id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Rapat tidak ditemukan.".to_string())?;
    Ok(report::report_html(&summary, &notes.snapshot(), utc_offset_secs))
}

/// Removes a meeting and its recordings for good. Returns whether it was the current meeting.
#[tauri::command]
fn delete_meeting(id: i64, app: AppHandle, meeting: State<Meeting>, db: State<Db>) -> Result<bool, String> {
    let mut state = meeting.state();
    let is_current = state.meeting_id == Some(id);
    if is_current && state.session.is_some() {
        return Err("Hentikan rekaman sebelum menghapus rapat ini.".into());
    }
    let (summary, _) = db
        .store()
        .load(id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Rapat tidak ditemukan.".to_string())?;

    let audio_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("recordings")
        .join(summary.started_at.to_string());
    match std::fs::remove_dir_all(&audio_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{}: {error}", audio_dir.display())),
    }
    db.store().delete(id).map_err(|error| error.to_string())?;

    if is_current {
        state.notes = None;
        state.meeting_id = None;
        state.audio_dir = None;
    }
    Ok(is_current)
}

/// Embeds the passages of meetings that were not embedded yet, so that they can be found by
/// meaning. Failures only mean that search falls back to words, so they are logged, not shown.
fn index_in_background(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let Ok(embedder) = models::embedder(&app) else {
            return;
        };
        if let Err(error) = semantic::index_pending(&app.state::<Db>().0, &embedder) {
            eprintln!("pencarian makna: {error}");
        }
    });
}

/// A meeting found by the history search, with the passage that matched by meaning.
#[derive(serde::Serialize)]
struct HistoryEntry {
    meeting: MeetingSummary,
    snippet: Option<String>,
    at_secs: Option<f64>,
}

#[derive(serde::Serialize)]
struct HistorySearch {
    entries: Vec<HistoryEntry>,
    /// Why the search by meaning was left out, when it was.
    note: Option<String>,
}

/// Meetings that talk about what `query` means come first, then those that contain its words.
#[tauri::command]
async fn search_history(query: String, app: AppHandle) -> Result<HistorySearch, String> {
    spawn_blocking(move || {
        let by_words = app.state::<Db>().store().list(&query).map_err(|error| error.to_string())?;
        let mut entries: Vec<HistoryEntry> = Vec::new();
        let mut note = None;
        match models::embedder(&app)
            .map_err(|error| error.to_string())
            .and_then(|embedder| semantic::search(&app.state::<Db>().0, &embedder, &query, SEMANTIC_RESULTS).map_err(|error| error.to_string()))
        {
            Ok(passages) => {
                for passage in passages {
                    if !entries.iter().any(|entry| entry.meeting.id == passage.meeting.id) {
                        entries.push(HistoryEntry {
                            meeting: passage.meeting,
                            snippet: Some(shortened(&passage.text)),
                            at_secs: Some(passage.start_secs),
                        });
                    }
                }
            }
            Err(error) => note = Some(format!("Pencarian makna tidak aktif: {error}")),
        }
        for meeting in by_words {
            if !entries.iter().any(|entry| entry.meeting.id == meeting.id) {
                entries.push(HistoryEntry { meeting, snippet: None, at_secs: None });
            }
        }
        Ok(HistorySearch { entries, note })
    })
    .await
    .map_err(|error| error.to_string())?
}

fn shortened(text: &str) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(SNIPPET_CHARS).collect();
    if chars.next().is_some() {
        format!("{}…", head.trim_end())
    } else {
        head
    }
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
fn open_meeting(id: i64, app: AppHandle, meeting: State<Meeting>, db: State<Db>) -> Result<MeetingView, String> {
    let mut state = meeting.state();
    if state.session.is_some() {
        return Err("Hentikan rekaman sebelum membuka rapat lain.".into());
    }
    let (summary, notes) = db
        .store()
        .load(id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Rapat tidak ditemukan.".to_string())?;
    state.notes = Some(Arc::new(MeetingNotes::restore(models::llm(&app), notes.clone())));
    state.meeting_id = Some(id);
    // Only read from here (to play back); a new recording sets its own folder.
    state.audio_dir = app
        .path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("recordings").join(summary.started_at.to_string()))
        .filter(|dir| dir.is_dir());
    Ok(MeetingView { summary, notes })
}

/// The saved audio of the current meeting, as paths the window can play: "microphone" and "system".
#[tauri::command]
fn recording_files(meeting: State<Meeting>) -> std::collections::HashMap<String, String> {
    let mut files = std::collections::HashMap::new();
    let Some(dir) = meeting.state().audio_dir.clone() else { return files };
    for (name, source) in [("microphone", Source::Microphone), ("system", Source::System)] {
        let wav = recording_path(&dir, source);
        // Finished recordings are FLAC; one still being written or not yet compressed is WAV.
        let flac = wav.with_extension("flac");
        if let Some(path) = [flac, wav].into_iter().find(|path| path.exists()) {
            files.insert(name.to_owned(), path.to_string_lossy().into_owned());
        }
    }
    files
}

/// Chooses the transcription provider from STT_PROVIDER: "openai" (default,
/// any OpenAI-compatible endpoint), "deepgram" or "whisper" (offline).
/// Settings come from the environment until the Settings screen exists.
fn transcriber_from_env(app: &AppHandle) -> Result<Arc<dyn Transcriber>, String> {
    build_transcriber(app, false)
}

const NO_STT: &str = "Transkripsi belum diatur. Buka Model → Transkripsi, pilih penyedia dan isi kunci API.";

/// `final_pass`: the slower, more accurate model used over a finished recording.
fn build_transcriber(app: &AppHandle, final_pass: bool) -> Result<Arc<dyn Transcriber>, String> {
    let language = spoken_language(app);
    // What was chosen in the app wins over the environment.
    if let Some((settings, api_key)) = stt::chosen(app)? {
        let transcriber: Arc<dyn Transcriber> = if settings.provider == "deepgram" {
            Arc::new(
                DeepgramTranscriber::new(DeepgramSettings { api_key, model: settings.model.clone(), language })
                    .map_err(|error| error.to_string())?,
            )
        } else {
            Arc::new(
                OpenAiTranscriber::new(OpenAiSettings {
                    base_url: settings.base_url.clone(),
                    api_key,
                    model: settings.model_for(final_pass).to_owned(),
                    language,
                })
                .map_err(|error| error.to_string())?,
            )
        };
        return Ok(Arc::new(CleaningTranscriber::new(transcriber, &glossary(app))));
    }
    let provider = env_or("STT_PROVIDER", "openai");
    let transcriber: Arc<dyn Transcriber> = match provider.as_str() {
        "openai" => {
            let base_url = env_or("STT_BASE_URL", "https://api.groq.com/openai/v1");
            let live_model = env_or("STT_MODEL", "whisper-large-v3-turbo");
            let model = match std::env::var("STT_FINAL_MODEL") {
                Ok(model) if final_pass => model,
                // Groq's full model is clearly more accurate than the turbo one, for the cost of time.
                _ if final_pass && base_url.contains("groq.com") && live_model == "whisper-large-v3-turbo" => {
                    "whisper-large-v3".into()
                }
                _ => live_model,
            };
            Arc::new(
                OpenAiTranscriber::new(OpenAiSettings { base_url, api_key: required_env("STT_API_KEY").map_err(|_| NO_STT.to_owned())?, model, language })
                    .map_err(|error| error.to_string())?,
            )
        }
        "deepgram" => Arc::new(
            DeepgramTranscriber::new(DeepgramSettings {
                api_key: required_env("DEEPGRAM_API_KEY").map_err(|_| NO_STT.to_owned())?,
                model: env_or("STT_MODEL", "nova-2"),
                language,
            })
            .map_err(|error| error.to_string())?,
        ),
        "whisper" => local_whisper(language)?,
        other => return Err(format!("STT_PROVIDER \"{other}\" tidak dikenal. Pilihan: openai, deepgram, whisper.")),
    };
    Ok(Arc::new(CleaningTranscriber::new(transcriber, &glossary(app))))
}

#[cfg(feature = "local-whisper")]
fn local_whisper(language: String) -> Result<Arc<dyn Transcriber>, String> {
    use notetaker_core::whisper_transcriber::WhisperTranscriber;
    let model = PathBuf::from(required_env("WHISPER_MODEL_PATH")?);
    Ok(Arc::new(WhisperTranscriber::new(&model, language).map_err(|error| error.to_string())?))
}

#[cfg(not(feature = "local-whisper"))]
fn local_whisper(_language: String) -> Result<Arc<dyn Transcriber>, String> {
    Err("Aplikasi ini dibangun tanpa Whisper lokal. Bangun ulang dengan --features local-whisper.".into())
}

fn language_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|dir| dir.join("language.txt"))
}

/// The language the user chose in the app, else STT_LANGUAGE, else Indonesian.
fn spoken_language(app: &AppHandle) -> String {
    language_file(app)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| text.trim().to_owned())
        .filter(|code| !code.is_empty())
        .unwrap_or_else(|| env_or("STT_LANGUAGE", "id"))
}

fn glossary_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|dir| dir.join("glossary.txt"))
}

/// Names and terms the user wants spelled correctly in the transcript.
fn glossary(app: &AppHandle) -> String {
    glossary_file(app).and_then(|path| std::fs::read_to_string(path).ok()).unwrap_or_default()
}

#[tauri::command]
fn get_glossary(app: AppHandle) -> String {
    glossary(&app)
}

/// Applies to the next recording or import.
#[tauri::command]
fn set_glossary(text: String, app: AppHandle) -> Result<(), String> {
    let path = glossary_file(&app).ok_or("Folder data aplikasi tidak ditemukan.")?;
    std::fs::write(path, text.trim()).map_err(|error| error.to_string())
}

#[tauri::command]
fn get_language(app: AppHandle) -> String {
    spoken_language(&app)
}

/// Applies to the next recording or import.
#[tauri::command]
fn set_language(code: String, app: AppHandle) -> Result<(), String> {
    let path = language_file(&app).ok_or("Folder data aplikasi tidak ditemukan.")?;
    std::fs::write(path, code.trim()).map_err(|error| error.to_string())
}

fn required_env(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("Variabel lingkungan {name} belum diatur."))
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.into())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec![tray::MINIMIZED_ARG])))
        .plugin(tauri_plugin_notification::init())
        .plugin(desktop::hotkey_plugin())
        .manage(Meeting::default())
        // Closing the window hides it; the app keeps running in the tray until it is quit from there.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let store = Store::open(&data_dir.join("meetings.db"))?;
            load_custom_template(&store);
            app.manage(Db(Mutex::new(store)));
            index_in_background(app.handle());
            tray::setup(app)?;
            desktop::setup(app);
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
            tray::set_tray_recording,
            tray::quit_app,
            tray::get_autostart,
            tray::set_autostart,
            desktop::notify,
            edit_segment,
            get_glossary,
            set_glossary,
            get_language,
            set_language,
            search_history,
            import_recording,
            open_meeting,
            recording_files,
            set_template,
            add_bookmark,
            remove_bookmark,
            expand_notes,
            suggest_questions,
            catch_up,
            explain_line,
            follow_up_email,
            ask_library,
            export_meeting,
            report_html,
            talk_stats,
            get_custom_template,
            set_custom_template,
            extract_actions,
            send_actions,
            list_actions,
            set_action_done,
            get_webhook,
            set_webhook,
            export_clip,
            relabel_speakers,
            delete_meeting,
            models::get_models,
            stt::get_stt,
            stt::save_stt,
            stt::reset_stt,
            stt::test_stt,
            models::save_provider,
            models::remove_provider,
            models::add_model,
            models::remove_model,
            models::activate_model,
            models::fetch_models,
            models::test_model
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the application");
}
