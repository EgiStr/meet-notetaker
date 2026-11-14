//! Shared helpers for the stage 0 benchmarks: load audio, time a run, report speed.

use std::error::Error;
use std::path::Path;
use std::time::{Duration, Instant};

pub const SAMPLE_RATE: u32 = 16_000;

/// Decodes an audio file to mono f32 samples at 16 kHz.
pub fn load_mono_16k(path: &Path) -> Result<Vec<f32>, Box<dyn Error>> {
    let (samples, rate) = polyvoice::wav::load_audio(path)?;
    if rate != SAMPLE_RATE {
        return Err(format!("expected {SAMPLE_RATE} Hz audio, got {rate} Hz").into());
    }
    Ok(samples)
}

pub fn audio_seconds(sample_count: usize) -> f64 {
    sample_count as f64 / f64::from(SAMPLE_RATE)
}

/// How many seconds of audio are processed per second of wall time.
/// Above 1.0 means faster than realtime.
pub fn speed_factor(audio_secs: f64, elapsed: Duration) -> f64 {
    audio_secs / elapsed.as_secs_f64()
}

pub fn timed<T>(work: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let output = work();
    (output, start.elapsed())
}

pub fn print_speed(label: &str, audio_secs: f64, elapsed: Duration) {
    eprintln!(
        "{label}: {audio_secs:.1}s audio in {:.1}s = {:.2}x realtime",
        elapsed.as_secs_f64(),
        speed_factor(audio_secs, elapsed)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_seconds_divides_by_sample_rate() {
        assert_eq!(audio_seconds(32_000), 2.0);
    }

    #[test]
    fn speed_factor_above_one_means_faster_than_realtime() {
        assert_eq!(speed_factor(60.0, Duration::from_secs(20)), 3.0);
    }

    #[test]
    fn speed_factor_below_one_means_slower_than_realtime() {
        assert_eq!(speed_factor(10.0, Duration::from_secs(40)), 0.25);
    }
}
