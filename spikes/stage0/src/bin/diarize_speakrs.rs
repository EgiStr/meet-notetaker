//! Usage: diarize_speakrs <audio>

use std::error::Error;
use std::path::Path;

use speakrs::{ExecutionMode, OwnedDiarizationPipeline};
use stage0::{audio_seconds, load_mono_16k, print_speed, timed};

fn main() -> Result<(), Box<dyn Error>> {
    let audio = std::env::args().nth(1).ok_or("usage: diarize_speakrs <audio>")?;
    let samples = load_mono_16k(Path::new(&audio))?;

    let mut pipeline = OwnedDiarizationPipeline::from_pretrained(ExecutionMode::Cpu)?;

    let (outcome, elapsed) = timed(|| pipeline.run(&samples));
    let result = outcome?;

    for segment in result.discrete_diarization.to_segments() {
        println!("[{:>7.1} - {:>7.1}] {}", segment.start, segment.end, segment.speaker);
    }
    print_speed("speakrs", audio_seconds(samples.len()), elapsed);
    Ok(())
}
