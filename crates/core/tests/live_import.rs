//! Imports a recording through the real transcription provider. Skipped by
//! default; run with
//! `cargo test -p notetaker-core --test live_import -- --ignored --nocapture`
//! after setting IMPORT_SAMPLE and STT_API_KEY (optionally STT_BASE_URL,
//! STT_MODEL, STT_LANGUAGE).

use std::path::PathBuf;

use notetaker_core::import::{load_recording, transcribe_recording};
use notetaker_core::openai_transcriber::{OpenAiSettings, OpenAiTranscriber};

#[test]
#[ignore = "calls the real transcription provider"]
fn a_recording_file_is_transcribed_on_its_own_timeline() {
    let env = |name: &str| std::env::var(name).ok();
    let sample = PathBuf::from(env("IMPORT_SAMPLE").expect("IMPORT_SAMPLE is not set"));
    let transcriber = OpenAiTranscriber::new(OpenAiSettings {
        base_url: env("STT_BASE_URL").unwrap_or_else(|| "https://api.groq.com/openai/v1".into()),
        api_key: env("STT_API_KEY").expect("STT_API_KEY is not set"),
        model: env("STT_MODEL").unwrap_or_else(|| "whisper-large-v3-turbo".into()),
        language: env("STT_LANGUAGE").unwrap_or_else(|| "id".into()),
    })
    .unwrap();

    let samples = load_recording(&sample).unwrap();
    let seconds = samples.len() as f64 / 16_000.0;
    let mut segments = Vec::new();
    transcribe_recording(&samples, &transcriber, &std::thread::sleep, &mut |segment| segments.push(segment)).unwrap();

    for segment in &segments {
        println!("[{:>6.1} - {:>6.1}] {}", segment.start_secs, segment.end_secs, segment.text);
    }
    assert!(!segments.is_empty());
    assert!(segments.windows(2).all(|pair| pair[0].start_secs <= pair[1].start_secs));
    assert!(segments.last().unwrap().end_secs <= seconds + 1.0);
    // The text must cover the recording, not only its first piece.
    assert!(segments.last().unwrap().end_secs >= seconds * 0.8);
}
