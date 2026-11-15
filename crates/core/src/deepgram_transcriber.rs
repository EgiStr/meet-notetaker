//! Transcriber for Deepgram's pre-recorded API (`POST /v1/listen`).
//!
//! Deepgram can label speakers, but only within one request. Chunks are sent
//! one request each, so those labels would not match between chunks; speakers
//! are therefore still labelled by the final diarization.

use std::time::Duration;

use reqwest::blocking::Client;
use serde::Deserialize;

use crate::audio::encode_wav;
use crate::transcript::{AudioChunk, TranscribeError, Transcriber, TranscriptSegment};

const ENDPOINT: &str = "https://api.deepgram.com/v1/listen";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct DeepgramSettings {
    pub api_key: String,
    /// For example "nova-2".
    pub model: String,
    /// ISO 639-1 code of the main spoken language, for example "id".
    pub language: String,
}

pub struct DeepgramTranscriber {
    client: Client,
    settings: DeepgramSettings,
}

impl DeepgramTranscriber {
    pub fn new(settings: DeepgramSettings) -> Result<Self, TranscribeError> {
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
        Ok(Self { client, settings })
    }
}

impl Transcriber for DeepgramTranscriber {
    fn transcribe(&self, chunk: &AudioChunk) -> Result<Vec<TranscriptSegment>, TranscribeError> {
        let response = self
            .client
            .post(ENDPOINT)
            .query(&[
                ("model", self.settings.model.as_str()),
                ("language", self.settings.language.as_str()),
                ("smart_format", "true"),
                ("utterances", "true"),
            ])
            .header("Authorization", format!("Token {}", self.settings.api_key))
            .header("Content-Type", "audio/wav")
            .body(encode_wav(&chunk.samples)?)
            .send()?;

        let status = response.status();
        let body = response.text()?;
        if !status.is_success() {
            return Err(TranscribeError::Rejected { status: status.as_u16(), body });
        }
        parse_utterances(&body, chunk)
    }
}

#[derive(Deserialize)]
struct Response {
    results: Results,
}

#[derive(Deserialize)]
struct Results {
    #[serde(default)]
    utterances: Vec<Utterance>,
}

#[derive(Deserialize)]
struct Utterance {
    start: f64,
    end: f64,
    transcript: String,
}

/// Maps the provider's chunk-relative utterances onto the session timeline.
fn parse_utterances(body: &str, chunk: &AudioChunk) -> Result<Vec<TranscriptSegment>, TranscribeError> {
    let response: Response = serde_json::from_str(body)?;
    Ok(response
        .results
        .utterances
        .into_iter()
        .filter(|utterance| !utterance.transcript.trim().is_empty())
        .map(|utterance| TranscriptSegment {
            source: chunk.source,
            start_secs: chunk.start_secs + utterance.start,
            end_secs: chunk.start_secs + utterance.end,
            text: utterance.transcript.trim().to_owned(),
            speaker: None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::Source;

    fn chunk_at(start_secs: f64) -> AudioChunk {
        AudioChunk { source: Source::Microphone, start_secs, samples: Vec::new() }
    }

    #[test]
    fn utterances_are_shifted_to_the_session_timeline() {
        let body = r#"{"metadata":{},"results":{"channels":[],"utterances":[
            {"start":0.4,"end":2.1,"confidence":0.98,"channel":0,"transcript":"Selamat pagi.","speaker":0,"id":"a"},
            {"start":2.5,"end":4.0,"confidence":0.97,"channel":0,"transcript":" Kita mulai. ","id":"b"}]}}"#;
        let segments = parse_utterances(body, &chunk_at(30.0)).unwrap();
        assert_eq!(
            segments,
            vec![
                TranscriptSegment {
                    source: Source::Microphone,
                    start_secs: 30.4,
                    end_secs: 32.1,
                    text: "Selamat pagi.".into(),
                    speaker: None,
                },
                TranscriptSegment {
                    source: Source::Microphone,
                    start_secs: 32.5,
                    end_secs: 34.0,
                    text: "Kita mulai.".into(),
                    speaker: None,
                },
            ]
        );
    }

    #[test]
    fn blank_utterances_and_missing_utterance_lists_yield_nothing() {
        let blank = r#"{"results":{"utterances":[{"start":0.0,"end":1.0,"transcript":" "}]}}"#;
        assert!(parse_utterances(blank, &chunk_at(0.0)).unwrap().is_empty());
        assert!(parse_utterances(r#"{"results":{"channels":[]}}"#, &chunk_at(0.0)).unwrap().is_empty());
    }

    #[test]
    fn a_body_without_results_is_reported_as_malformed() {
        let error = parse_utterances(r#"{"err_code":"x"}"#, &chunk_at(0.0)).unwrap_err();
        assert!(matches!(error, TranscribeError::Malformed(_)));
    }
}
