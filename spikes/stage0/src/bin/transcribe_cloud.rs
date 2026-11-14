//! Sends one audio file to an OpenAI-compatible transcription endpoint.
//!
//! Usage: transcribe_cloud <audio> [language=id]
//! Env:   STT_API_KEY (required)
//!        STT_BASE_URL (default https://api.groq.com/openai/v1)
//!        STT_MODEL    (default whisper-large-v3-turbo)

use std::error::Error;
use std::path::Path;
use std::time::Duration;

use reqwest::blocking::{multipart, Client};
use serde::Deserialize;
use stage0::{print_speed, timed};

const DEFAULT_BASE_URL: &str = "https://api.groq.com/openai/v1";
const DEFAULT_MODEL: &str = "whisper-large-v3-turbo";
/// Whole recordings take longer to upload than reqwest's 30 s default allows.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Deserialize)]
struct Transcription {
    duration: f64,
    segments: Vec<Segment>,
}

#[derive(Deserialize)]
struct Segment {
    start: f64,
    end: f64,
    text: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let audio = args.get(1).ok_or("usage: transcribe_cloud <audio> [language=id]")?;
    let language = args.get(2).map_or("id", String::as_str);

    let api_key = std::env::var("STT_API_KEY").map_err(|_| "set STT_API_KEY first")?;
    let base_url = std::env::var("STT_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.into());
    let model = std::env::var("STT_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.into());

    let (outcome, elapsed) = timed(|| request_transcription(&base_url, &api_key, &model, language, Path::new(audio)));
    let transcription = outcome?;

    for segment in &transcription.segments {
        println!("[{:>7.1} - {:>7.1}] {}", segment.start, segment.end, segment.text.trim());
    }
    print_speed(&model, transcription.duration, elapsed);
    Ok(())
}

fn request_transcription(
    base_url: &str,
    api_key: &str,
    model: &str,
    language: &str,
    audio: &Path,
) -> Result<Transcription, Box<dyn Error>> {
    let form = multipart::Form::new()
        .text("model", model.to_owned())
        .text("language", language.to_owned())
        .text("response_format", "verbose_json")
        .file("file", audio)?;

    let response = Client::builder()
        .timeout(UPLOAD_TIMEOUT)
        .build()?
        .post(format!("{base_url}/audio/transcriptions"))
        .bearer_auth(api_key)
        .multipart(form)
        .send()?;

    let status = response.status();
    let body = response.text()?;
    if !status.is_success() {
        return Err(format!("{status}: {body}").into());
    }
    Ok(serde_json::from_str(&body)?)
}
