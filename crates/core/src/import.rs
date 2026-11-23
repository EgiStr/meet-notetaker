//! Transcribes a recording file with the same chunker and transcriber as a
//! live session, without the pacing a live session gets from the clock.

use std::path::Path;
use std::time::Duration;

use crate::audio::samples_to_seconds;
use crate::chunker::{Chunk, Chunker, ChunkerConfig};
use crate::transcript::{AudioChunk, Source, TranscribeError, Transcriber, TranscriptSegment};

const TOO_MANY_REQUESTS: u16 = 429;
const MAX_ATTEMPTS: usize = 6;
/// Request limits are counted per minute, so a few of these waits outlast one.
pub const RATE_LIMIT_WAIT: Duration = Duration::from_secs(20);

#[derive(Debug, thiserror::Error)]
#[error("could not read {path}: {reason}")]
pub struct DecodeError {
    path: String,
    reason: String,
}

/// Decodes an audio file (wav, mp3, flac, ogg, m4a) to 16 kHz mono samples.
pub fn load_recording(path: &Path) -> Result<Vec<f32>, DecodeError> {
    let failed = |reason: String| DecodeError { path: path.display().to_string(), reason };
    crate::audio_file::read_samples(path).map_err(failed)
}

/// A file is sent in pieces of up to a minute: few enough requests to stay
/// under per-minute limits, small enough to upload within the request timeout.
fn chunker_config() -> ChunkerConfig {
    ChunkerConfig {
        min_chunk_secs: 30.0,
        max_chunk_secs: 60.0,
        // Long pauses must not cut short chunks here; nobody is waiting for a live line.
        long_pause_secs: 60.0,
        ..ChunkerConfig::default()
    }
}

/// Transcribes the whole recording in order. All of it is treated as system
/// audio, so that every voice in it is diarized. `wait` is called before a
/// request refused for rate limiting is repeated.
pub fn transcribe_recording(
    samples: &[f32],
    transcriber: &dyn Transcriber,
    wait: &dyn Fn(Duration),
    on_segment: &mut dyn FnMut(TranscriptSegment),
) -> Result<(), TranscribeError> {
    let mut chunker = Chunker::new(chunker_config());
    let mut chunks = chunker.push(samples);
    chunks.extend(chunker.flush());

    for Chunk { start_sample, samples } in chunks {
        let chunk = AudioChunk { source: Source::System, start_secs: samples_to_seconds(start_sample), samples };
        transcribe_with_retry(&chunk, transcriber, wait)?.into_iter().for_each(&mut *on_segment);
    }
    Ok(())
}

/// Speech must be this many times louder than the stream's quiet level.
const SPEECH_OVER_NOISE: f32 = 2.5;
const MIN_SPEECH_RMS: f32 = 0.01;
const MAX_SPEECH_RMS: f32 = 0.08;
const FRAME: usize = 480;

/// The loudness that separates speech from the background of this stream. A microphone in a noisy
/// room never goes quiet, so a fixed threshold would send noise to the transcriber, which then
/// invents words for it.
fn speech_threshold(samples: &[f32]) -> f32 {
    let mut levels: Vec<f32> = samples
        .chunks_exact(FRAME)
        .map(|frame| (frame.iter().map(|x| x * x).sum::<f32>() / FRAME as f32).sqrt())
        .collect();
    if levels.is_empty() {
        return MIN_SPEECH_RMS;
    }
    levels.sort_by(f32::total_cmp);
    let quiet = levels[levels.len() / 5];
    (quiet * SPEECH_OVER_NOISE).clamp(MIN_SPEECH_RMS, MAX_SPEECH_RMS)
}

/// A finished stream is transcribed again in long pieces, cut at pauses: the model sees whole
/// sentences instead of the 6 s fragments a live session needs, which is much more accurate.
fn refine_config(samples: &[f32]) -> ChunkerConfig {
    ChunkerConfig {
        min_chunk_secs: 15.0,
        max_chunk_secs: 30.0,
        pause_secs: 0.6,
        long_pause_secs: 60.0,
        speech_rms: speech_threshold(samples),
    }
}

/// Transcribes one finished stream in long pieces. The segments are returned, not delivered, so the
/// caller can keep the live transcript when this fails.
pub fn refine_stream(
    samples: &[f32],
    source: Source,
    transcriber: &dyn Transcriber,
    wait: &dyn Fn(Duration),
) -> Result<Vec<TranscriptSegment>, TranscribeError> {
    let mut chunker = Chunker::new(refine_config(samples));
    let mut chunks = chunker.push(samples);
    chunks.extend(chunker.flush());

    let mut segments = Vec::new();
    for Chunk { start_sample, samples } in chunks {
        let chunk = AudioChunk { source, start_secs: samples_to_seconds(start_sample), samples };
        segments.extend(transcribe_with_retry(&chunk, transcriber, wait)?);
    }
    Ok(segments)
}

fn transcribe_with_retry(
    chunk: &AudioChunk,
    transcriber: &dyn Transcriber,
    wait: &dyn Fn(Duration),
) -> Result<Vec<TranscriptSegment>, TranscribeError> {
    let mut attempt = 1;
    loop {
        match transcriber.transcribe(chunk) {
            Err(TranscribeError::Rejected { status: TOO_MANY_REQUESTS, .. }) if attempt < MAX_ATTEMPTS => {
                attempt += 1;
                wait(RATE_LIMIT_WAIT);
            }
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::Mutex;

    use super::*;
    use crate::audio::seconds_to_samples;

    /// Fails with the given status a number of times, then returns one segment per chunk.
    struct FakeTranscriber {
        failures_left: Mutex<usize>,
        status: u16,
        calls: Mutex<usize>,
    }

    impl FakeTranscriber {
        fn failing(times: usize, status: u16) -> Self {
            Self { failures_left: Mutex::new(times), status, calls: Mutex::new(0) }
        }

        fn calls(&self) -> usize {
            *self.calls.lock().unwrap()
        }
    }

    impl Transcriber for FakeTranscriber {
        fn transcribe(&self, chunk: &AudioChunk) -> Result<Vec<TranscriptSegment>, TranscribeError> {
            *self.calls.lock().unwrap() += 1;
            let mut failures_left = self.failures_left.lock().unwrap();
            if *failures_left > 0 {
                *failures_left -= 1;
                return Err(TranscribeError::Rejected { status: self.status, body: String::new() });
            }
            Ok(vec![TranscriptSegment {
                source: chunk.source,
                start_secs: chunk.start_secs,
                end_secs: chunk.start_secs + samples_to_seconds(chunk.samples.len() as u64),
                text: "teks".into(),
                speaker: None,
            }])
        }
    }

    fn speech(seconds: f64) -> Vec<f32> {
        (0..seconds_to_samples(seconds)).map(|index| if index % 2 == 0 { 0.5 } else { -0.5 }).collect()
    }

    fn silence(seconds: f64) -> Vec<f32> {
        vec![0.0; seconds_to_samples(seconds)]
    }

    #[test]
    fn a_noisy_stream_gets_a_higher_speech_threshold_than_a_clean_one() {
        let noise = vec![0.03_f32; seconds_to_samples(2.0)];
        assert!(speech_threshold(&noise) > 0.06);
        assert_eq!(speech_threshold(&silence(2.0)), MIN_SPEECH_RMS);
        assert_eq!(speech_threshold(&vec![0.5; seconds_to_samples(2.0)]), MAX_SPEECH_RMS);
    }

    #[test]
    fn background_noise_is_not_sent_but_louder_speech_is() {
        let transcriber = FakeTranscriber::failing(0, 0);
        let mut stream = vec![0.03_f32; seconds_to_samples(20.0)];
        stream.extend(speech(5.0));
        let segments = refine_stream(&stream, Source::Microphone, &transcriber, &|_| {}).unwrap();
        assert_eq!(transcriber.calls(), 1);
        assert!(segments[0].start_secs >= 19.0);
    }

    fn import(samples: &[f32], transcriber: &FakeTranscriber) -> (Result<(), TranscribeError>, Vec<TranscriptSegment>, usize) {
        let waits = Cell::new(0);
        let mut segments = Vec::new();
        let result = transcribe_recording(samples, transcriber, &|_| waits.set(waits.get() + 1), &mut |segment| {
            segments.push(segment)
        });
        (result, segments, waits.get())
    }

    #[test]
    fn a_long_recording_is_sent_in_pieces_of_at_most_a_minute_on_its_timeline() {
        let transcriber = FakeTranscriber::failing(0, 0);
        let (result, segments, _) = import(&speech(150.0), &transcriber);
        result.unwrap();

        let starts: Vec<f64> = segments.iter().map(|segment| segment.start_secs).collect();
        assert_eq!(starts, [0.0, 60.0, 120.0]);
        assert!(segments.iter().all(|segment| segment.source == Source::System));
    }

    #[test]
    fn a_two_second_pause_does_not_end_a_piece_early() {
        let recording = [speech(10.0), silence(2.5), speech(10.0)].concat();
        let transcriber = FakeTranscriber::failing(0, 0);
        import(&recording, &transcriber).0.unwrap();
        assert_eq!(transcriber.calls(), 1);
    }

    #[test]
    fn a_rate_limited_request_is_repeated_after_waiting() {
        let transcriber = FakeTranscriber::failing(2, 429);
        let (result, segments, waits) = import(&speech(10.0), &transcriber);
        result.unwrap();
        assert_eq!((segments.len(), waits, transcriber.calls()), (1, 2, 3));
    }

    #[test]
    fn rate_limiting_that_does_not_end_fails_the_import() {
        let transcriber = FakeTranscriber::failing(usize::MAX, 429);
        let (result, segments, waits) = import(&speech(10.0), &transcriber);
        assert!(matches!(result, Err(TranscribeError::Rejected { status: 429, .. })));
        assert!(segments.is_empty());
        assert_eq!((waits, transcriber.calls()), (MAX_ATTEMPTS - 1, MAX_ATTEMPTS));
    }

    #[test]
    fn other_failures_are_not_repeated() {
        let transcriber = FakeTranscriber::failing(1, 401);
        let (result, _, waits) = import(&speech(10.0), &transcriber);
        assert!(matches!(result, Err(TranscribeError::Rejected { status: 401, .. })));
        assert_eq!((waits, transcriber.calls()), (0, 1));
    }

    #[test]
    fn a_silent_recording_makes_no_request() {
        let transcriber = FakeTranscriber::failing(0, 0);
        import(&silence(30.0), &transcriber).0.unwrap();
        assert_eq!(transcriber.calls(), 0);
    }
}
