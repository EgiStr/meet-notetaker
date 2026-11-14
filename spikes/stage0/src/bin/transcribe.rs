//! Usage: transcribe <ggml-model> <audio> [language=id] [threads=6]

use std::error::Error;
use std::path::Path;

use stage0::{audio_seconds, load_mono_16k, print_speed, timed};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (Some(model), Some(audio)) = (args.get(1), args.get(2)) else {
        return Err("usage: transcribe <ggml-model> <audio> [language=id] [threads=6]".into());
    };
    let language = args.get(3).map_or("id", String::as_str);
    let threads: i32 = args.get(4).map_or(Ok(6), |raw| raw.parse())?;

    let samples = load_mono_16k(Path::new(audio))?;
    let context = WhisperContext::new_with_params(model, WhisperContextParameters::default())?;
    let mut state = context.create_state()?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some(language));
    params.set_n_threads(threads);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    let (outcome, elapsed) = timed(|| state.full(params, &samples));
    outcome?;

    for segment in state.as_iter() {
        println!(
            "[{:>7.1} - {:>7.1}] {segment}",
            centiseconds_to_seconds(segment.start_timestamp()),
            centiseconds_to_seconds(segment.end_timestamp()),
        );
    }
    print_speed("whisper", audio_seconds(samples.len()), elapsed);
    Ok(())
}

fn centiseconds_to_seconds(centiseconds: i64) -> f64 {
    centiseconds as f64 / 100.0
}
