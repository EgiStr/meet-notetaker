//! Records the microphone and system audio and prints the transcript as it arrives.
//!
//! Usage: live [seconds=60] [language=id]
//! Env:   STT_API_KEY (required)
//!        STT_BASE_URL (default https://api.groq.com/openai/v1)
//!        STT_MODEL    (default whisper-large-v3-turbo)

use std::error::Error;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use notetaker_core::openai_transcriber::{OpenAiSettings, OpenAiTranscriber};
use notetaker_core::session::{Session, SessionEvent};
use notetaker_core::transcript::Source;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let seconds: u64 = args.get(1).map_or(Ok(60), |raw| raw.parse())?;
    let language = args.get(2).cloned().unwrap_or_else(|| "id".into());

    let transcriber = OpenAiTranscriber::new(OpenAiSettings {
        base_url: env_or("STT_BASE_URL", "https://api.groq.com/openai/v1"),
        api_key: std::env::var("STT_API_KEY").map_err(|_| "set STT_API_KEY first")?,
        model: env_or("STT_MODEL", "whisper-large-v3-turbo"),
        language,
    })?;

    let session = Session::start(
        &[Source::Microphone, Source::System],
        Arc::new(transcriber),
        Arc::new(print_event),
        None,
    );
    eprintln!("recording for {seconds}s...");
    thread::sleep(Duration::from_secs(seconds));
    session.stop();
    Ok(())
}

fn print_event(event: SessionEvent) {
    match event {
        SessionEvent::Segment(segment) => {
            let speaker = match segment.source {
                Source::Microphone => "Saya",
                Source::System => "Lainnya",
            };
            println!("[{:>6.1} - {:>6.1}] {speaker}: {}", segment.start_secs, segment.end_secs, segment.text);
        }
        SessionEvent::Error(error) => eprintln!("error: {error}"),
    }
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.into())
}
