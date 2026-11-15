//! Meeting capture and transcription pipeline:
//! capture -> downsample -> chunker -> Transcriber -> SessionEvent,
//! and MeetingNotes, which turns the transcript into minutes and answers.

pub mod audio;
pub mod capture;
pub mod chunker;
pub mod llm;
pub mod notes;
pub mod openai_llm;
pub mod openai_transcriber;
pub mod polyvoice_diarizer;
pub mod prompts;
pub mod session;
pub mod speakers;
pub mod store;
pub mod transcript;
