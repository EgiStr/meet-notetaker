//! Usage: diarize_polyvoice <audio>

use std::error::Error;
use std::path::Path;

use polyvoice::models::ModelRegistry;
use polyvoice::types::{Profile, SampleRate};
use polyvoice::{Pipeline, PipelineConfig};
use stage0::{audio_seconds, load_mono_16k, print_speed, timed, SAMPLE_RATE};

fn main() -> Result<(), Box<dyn Error>> {
    let audio = std::env::args().nth(1).ok_or("usage: diarize_polyvoice <audio>")?;
    let samples = load_mono_16k(Path::new(&audio))?;

    let mut config = PipelineConfig::default();
    config.profile = Profile::Balanced;
    let pipeline = Pipeline::builder()
        .config(config)
        .with_models_from(ModelRegistry::default()?)
        .build()?;
    let rate = SampleRate::new(SAMPLE_RATE).ok_or("bad sample rate")?;

    let (outcome, elapsed) = timed(|| pipeline.run(&samples, rate));
    let result = outcome?;

    for turn in &result.turns {
        println!("[{:>7.1} - {:>7.1}] {}", turn.time.start, turn.time.end, turn.speaker);
    }
    print_speed("polyvoice", audio_seconds(samples.len()), elapsed);
    Ok(())
}
