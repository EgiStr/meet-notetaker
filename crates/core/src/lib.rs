//! Meeting capture and transcription pipeline:
//! capture -> downsample -> chunker -> Transcriber -> SessionEvent.

pub mod audio;
pub mod capture;
pub mod chunker;
pub mod openai_transcriber;
pub mod session;
pub mod transcript;
