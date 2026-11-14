//! Transcriber for any endpoint that implements OpenAI's
//! `POST /audio/transcriptions` (Groq, OpenAI, and compatible servers).

use std::time::Duration;

use reqwest::blocking::{multipart, Client};
use serde::Deserialize;

use crate::audio::encode_wav;
use crate::transcript::{AudioChunk, TranscribeError, Transcriber, TranscriptSegment};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct OpenAiSettings {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// ISO 639-1 code of the main spoken language, for example "id".
    pub language: String,
}

pub struct OpenAiTranscriber {
    client: Client,
    settings: OpenAiSettings,
}

impl OpenAiTranscriber {
    pub fn new(settings: OpenAiSettings) -> Result<Self, TranscribeError> {
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
        Ok(Self { client, settings })
    }
}

impl Transcriber for OpenAiTranscriber {
    fn transcribe(&self, chunk: &AudioChunk) -> Result<Vec<TranscriptSegment>, TranscribeError> {
        let audio = multipart::Part::bytes(encode_wav(&chunk.samples)?)
            .file_name("chunk.wav")
            .mime_str("audio/wav")?;
        let form = multipart::Form::new()
            .text("model", self.settings.model.clone())
            .text("language", self.settings.language.clone())
            .text("response_format", "verbose_json")
            .part("file", audio);

        let response = self
            .client
            .post(format!("{}/audio/transcriptions", self.settings.base_url))
            .bearer_auth(&self.settings.api_key)
            .multipart(form)
            .send()?;

        let status = response.status();
        let body = response.text()?;
        if !status.is_success() {
            return Err(TranscribeError::Rejected { status: status.as_u16(), body });
        }
        parse_segments(&body, chunk)
    }
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    segments: Vec<ResponseSegment>,
}

#[derive(Deserialize)]
struct ResponseSegment {
    start: f64,
    end: f64,
    text: String,
    /// Absent on providers that do not report it; then the segment is kept.
    #[serde(default)]
    no_speech_prob: f64,
    #[serde(default)]
    avg_logprob: f64,
}

/// Whisper invents short phrases ("Thank you.") for noise. These are the
/// thresholds Whisper itself uses to discard such segments.
const MAX_NO_SPEECH_PROB: f64 = 0.6;
const MIN_AVG_LOGPROB: f64 = -1.0;

impl ResponseSegment {
    fn is_speech(&self) -> bool {
        !self.text.trim().is_empty()
            && self.no_speech_prob <= MAX_NO_SPEECH_PROB
            && self.avg_logprob >= MIN_AVG_LOGPROB
    }
}

/// Maps the provider's chunk-relative segments onto the session timeline.
fn parse_segments(body: &str, chunk: &AudioChunk) -> Result<Vec<TranscriptSegment>, TranscribeError> {
    let response: Response = serde_json::from_str(body)?;
    Ok(response
        .segments
        .into_iter()
        .filter(ResponseSegment::is_speech)
        .map(|segment| TranscriptSegment {
            source: chunk.source,
            start_secs: chunk.start_secs + segment.start,
            end_secs: chunk.start_secs + segment.end,
            text: segment.text.trim().to_owned(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::Source;

    fn chunk_at(start_secs: f64) -> AudioChunk {
        AudioChunk { source: Source::System, start_secs, samples: Vec::new() }
    }

    #[test]
    fn segments_are_shifted_to_the_session_timeline() {
        let body = r#"{"segments":[{"start":0.5,"end":2.0,"text":" Selamat pagi. "}]}"#;
        let segments = parse_segments(body, &chunk_at(100.0)).unwrap();
        assert_eq!(
            segments,
            vec![TranscriptSegment {
                source: Source::System,
                start_secs: 100.5,
                end_secs: 102.0,
                text: "Selamat pagi.".into(),
            }]
        );
    }

    #[test]
    fn blank_segments_are_dropped() {
        let body = r#"{"segments":[{"start":0.0,"end":1.0,"text":"  "}]}"#;
        assert!(parse_segments(body, &chunk_at(0.0)).unwrap().is_empty());
    }

    #[test]
    fn segments_the_model_marks_as_non_speech_are_dropped() {
        let body = r#"{"segments":[{"start":0.0,"end":1.0,"text":"Thank you.","no_speech_prob":0.9,"avg_logprob":-0.3}]}"#;
        assert!(parse_segments(body, &chunk_at(0.0)).unwrap().is_empty());
    }

    #[test]
    fn low_confidence_segments_are_dropped() {
        let body = r#"{"segments":[{"start":0.0,"end":1.0,"text":"I love it.","no_speech_prob":0.1,"avg_logprob":-1.4}]}"#;
        assert!(parse_segments(body, &chunk_at(0.0)).unwrap().is_empty());
    }

    #[test]
    fn confident_speech_is_kept() {
        let body = r#"{"segments":[{"start":0.0,"end":1.0,"text":"Halo.","no_speech_prob":0.02,"avg_logprob":-0.2}]}"#;
        assert_eq!(parse_segments(body, &chunk_at(0.0)).unwrap().len(), 1);
    }

    #[test]
    fn response_without_segments_yields_nothing() {
        assert!(parse_segments(r#"{"text":""}"#, &chunk_at(0.0)).unwrap().is_empty());
    }

    #[test]
    fn invalid_json_is_reported_as_malformed() {
        let error = parse_segments("not json", &chunk_at(0.0)).unwrap_err();
        assert!(matches!(error, TranscribeError::Malformed(_)));
    }
}
