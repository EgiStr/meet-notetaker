//! Meeting capture and transcription pipeline:
//! capture -> downsample -> chunker -> Transcriber -> SessionEvent,
//! and MeetingNotes, which turns the transcript into minutes and answers.

pub mod audio;
pub mod capture;
pub mod chunker;
pub mod cleanup;
pub mod compress;
pub mod deepgram_transcriber;
pub mod embed;
pub mod export;
pub mod import;
pub mod library;
pub mod live_speakers;
pub mod llm;
pub mod notes;
pub mod openai_llm;
pub mod openai_transcriber;
pub mod audio_file;
pub mod polyvoice_diarizer;
pub mod report;
pub mod prompts;
pub mod providers;
pub mod semantic;
pub mod session;
pub mod speakers;
pub mod store;
pub mod stt_settings;
pub mod transcript;
#[cfg(feature = "local-whisper")]
pub mod whisper_transcriber;
