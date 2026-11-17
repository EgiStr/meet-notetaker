//! Types shared by every transcription provider.

use serde::Serialize;

/// Which captured stream a piece of audio came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// The user's own voice.
    Microphone,
    /// Everything the computer plays: the other participants.
    System,
}

/// 16 kHz mono audio for one utterance.
#[derive(Debug, Clone)]
pub struct AudioChunk {
    pub source: Source,
    /// Seconds since the session started.
    pub start_secs: f64,
    pub samples: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TranscriptSegment {
    pub source: Source,
    /// Seconds since the session started.
    pub start_secs: f64,
    pub end_secs: f64,
    pub text: String,
    /// Set by the final diarization for system audio: 1 for the first voice
    /// heard, 2 for the second, and so on. `None` until then, and always for
    /// the microphone.
    pub speaker: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum TranscribeError {
    #[error("could not encode audio: {0}")]
    Encode(#[from] hound::Error),
    #[error("request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("provider rejected the request ({status}): {body}")]
    Rejected { status: u16, body: String },
    #[error("unexpected response: {0}")]
    Malformed(#[from] serde_json::Error),
    #[error("local transcription failed: {0}")]
    Local(String),
}

pub trait Transcriber: Send + Sync {
    fn transcribe(&self, chunk: &AudioChunk) -> Result<Vec<TranscriptSegment>, TranscribeError>;

    /// Like `transcribe`, with names, terms and the text just before the chunk as a hint.
    /// Providers that cannot use a hint ignore it.
    fn transcribe_with_prompt(&self, chunk: &AudioChunk, _prompt: &str) -> Result<Vec<TranscriptSegment>, TranscribeError> {
        self.transcribe(chunk)
    }
}
