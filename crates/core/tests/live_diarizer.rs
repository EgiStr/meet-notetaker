//! Runs the real diarization models on a recording. Skipped by default; run with
//! `cargo test -p notetaker-core --test live_diarizer -- --ignored --nocapture`
//! after setting DIARIZE_SAMPLE to a recording with several speakers.

use std::path::PathBuf;

use notetaker_core::audio::WavRecorder;
use notetaker_core::polyvoice_diarizer::PolyvoiceDiarizer;
use notetaker_core::speakers::{assign_speakers, Diarizer};
use notetaker_core::transcript::{Source, TranscriptSegment};

#[test]
#[ignore = "needs the diarization models and a sample recording"]
fn a_recording_written_by_the_session_is_diarized() {
    let sample = PathBuf::from(std::env::var("DIARIZE_SAMPLE").expect("DIARIZE_SAMPLE is not set"));
    let (samples, rate) = polyvoice::wav::load_audio(&sample).unwrap();
    assert_eq!(rate, 16_000);

    // Same writer the session uses, so this covers the file format end to end.
    let recording = std::env::temp_dir().join("notetaker-live-diarizer.wav");
    let mut recorder = WavRecorder::create(&recording).unwrap();
    recorder.write(&samples).unwrap();
    recorder.finish().unwrap();

    let turns = PolyvoiceDiarizer.diarize(&recording).unwrap();
    std::fs::remove_file(&recording).unwrap();
    assert!(!turns.is_empty());

    // Ten-second windows stand in for transcript segments.
    let seconds = samples.len() as f64 / 16_000.0;
    let mut segments: Vec<TranscriptSegment> = (0..(seconds / 10.0).ceil() as usize)
        .map(|index| TranscriptSegment {
            source: Source::System,
            start_secs: index as f64 * 10.0,
            end_secs: (index as f64 + 1.0) * 10.0,
            text: String::new(),
            speaker: None,
        })
        .collect();
    assign_speakers(&mut segments, &turns);

    for segment in &segments {
        println!("[{:>5.0} - {:>5.0}] {:?}", segment.start_secs, segment.end_secs, segment.speaker);
    }
    let voices = segments.iter().filter_map(|segment| segment.speaker).max().unwrap();
    println!("{} turns, {voices} speakers", turns.len());
    assert!(voices >= 2, "expected several speakers");
}
