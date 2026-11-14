//! Cuts a continuous 16 kHz stream into utterance-sized chunks at pauses.
//!
//! A chunk starts at the first speech frame and closes when one of these holds:
//! - it is at least `min_chunk_secs` long and a short pause follows,
//! - a long pause follows (so a brief remark is not held back),
//! - it reaches `max_chunk_secs`.
//! Silence between chunks is dropped, so it is never sent to the transcriber.

use crate::audio::seconds_to_samples;

/// 30 ms at 16 kHz.
const FRAME_SAMPLES: usize = 480;

#[derive(Debug, Clone)]
pub struct ChunkerConfig {
    /// Lower bound that keeps the request rate under the provider's limit.
    pub min_chunk_secs: f64,
    pub max_chunk_secs: f64,
    /// Pause that may close a chunk once it is long enough.
    pub pause_secs: f64,
    /// Pause that closes a chunk regardless of its length.
    pub long_pause_secs: f64,
    /// Frames with RMS at or above this count as speech.
    pub speech_rms: f32,
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self {
            min_chunk_secs: 6.0,
            max_chunk_secs: 20.0,
            pause_secs: 0.6,
            long_pause_secs: 2.0,
            speech_rms: 0.01,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// Index of the first sample, counted from the start of the stream.
    pub start_sample: u64,
    pub samples: Vec<f32>,
}

pub struct Chunker {
    min_samples: usize,
    max_samples: usize,
    pause_samples: usize,
    long_pause_samples: usize,
    speech_rms: f32,
    /// Samples received that do not yet fill a frame.
    pending: Vec<f32>,
    /// Stream index of the first sample in `pending`.
    pending_start: u64,
    current: Vec<f32>,
    current_start: u64,
    trailing_silence: usize,
}

impl Chunker {
    pub fn new(config: ChunkerConfig) -> Self {
        Self {
            min_samples: seconds_to_samples(config.min_chunk_secs),
            max_samples: seconds_to_samples(config.max_chunk_secs),
            pause_samples: seconds_to_samples(config.pause_secs),
            long_pause_samples: seconds_to_samples(config.long_pause_secs),
            speech_rms: config.speech_rms,
            pending: Vec::new(),
            pending_start: 0,
            current: Vec::new(),
            current_start: 0,
            trailing_silence: 0,
        }
    }

    pub fn push(&mut self, samples: &[f32]) -> Vec<Chunk> {
        self.pending.extend_from_slice(samples);
        let mut closed = Vec::new();
        let mut consumed = 0;
        while self.pending.len() - consumed >= FRAME_SAMPLES {
            let frame_start = self.pending_start + consumed as u64;
            let frame: Vec<f32> = self.pending[consumed..consumed + FRAME_SAMPLES].to_vec();
            consumed += FRAME_SAMPLES;
            if let Some(chunk) = self.accept_frame(frame_start, &frame) {
                closed.push(chunk);
            }
        }
        self.pending.drain(..consumed);
        self.pending_start += consumed as u64;
        closed
    }

    /// Closes the chunk in progress, if any. Call when the stream ends.
    pub fn flush(&mut self) -> Option<Chunk> {
        if self.current.is_empty() {
            return None;
        }
        Some(self.close_chunk())
    }

    fn accept_frame(&mut self, frame_start: u64, frame: &[f32]) -> Option<Chunk> {
        let is_speech = rms(frame) >= self.speech_rms;
        if self.current.is_empty() {
            if !is_speech {
                return None;
            }
            self.current_start = frame_start;
        }
        self.current.extend_from_slice(frame);
        self.trailing_silence = if is_speech { 0 } else { self.trailing_silence + frame.len() };

        let long_enough = self.current.len() >= self.min_samples;
        let should_close = self.current.len() >= self.max_samples
            || self.trailing_silence >= self.long_pause_samples
            || (long_enough && self.trailing_silence >= self.pause_samples);
        should_close.then(|| self.close_chunk())
    }

    fn close_chunk(&mut self) -> Chunk {
        let spoken = self.current.len() - self.trailing_silence;
        self.current.truncate(spoken);
        self.trailing_silence = 0;
        Chunk {
            start_sample: self.current_start,
            samples: std::mem::take(&mut self.current),
        }
    }
}

fn rms(frame: &[f32]) -> f32 {
    (frame.iter().map(|sample| sample * sample).sum::<f32>() / frame.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speech(seconds: f64) -> Vec<f32> {
        vec![0.5; seconds_to_samples(seconds)]
    }

    fn silence(seconds: f64) -> Vec<f32> {
        vec![0.0; seconds_to_samples(seconds)]
    }

    fn chunker() -> Chunker {
        Chunker::new(ChunkerConfig::default())
    }

    fn seconds(chunk: &Chunk) -> f64 {
        chunk.samples.len() as f64 / 16_000.0
    }

    #[test]
    fn silence_alone_produces_no_chunk() {
        let mut chunker = chunker();
        assert!(chunker.push(&silence(30.0)).is_empty());
        assert_eq!(chunker.flush(), None);
    }

    #[test]
    fn long_utterance_closes_at_short_pause() {
        let mut chunker = chunker();
        let mut closed = chunker.push(&speech(7.0));
        assert!(closed.is_empty());
        // Slightly more than `pause_secs`: the frame that straddles the end of
        // speech still counts as speech.
        closed.extend(chunker.push(&silence(0.7)));
        assert_eq!(closed.len(), 1);
        assert!((seconds(&closed[0]) - 7.0).abs() < 0.05);
    }

    #[test]
    fn short_utterance_waits_through_short_pause() {
        let mut chunker = chunker();
        chunker.push(&speech(2.0));
        assert!(chunker.push(&silence(0.9)).is_empty());
    }

    #[test]
    fn short_utterance_closes_at_long_pause() {
        let mut chunker = chunker();
        chunker.push(&speech(2.0));
        let closed = chunker.push(&silence(2.1));
        assert_eq!(closed.len(), 1);
        assert!((seconds(&closed[0]) - 2.0).abs() < 0.05);
    }

    #[test]
    fn continuous_speech_is_cut_at_max_length() {
        let closed = chunker().push(&speech(45.0));
        assert_eq!(closed.len(), 2);
        assert!((seconds(&closed[0]) - 20.0).abs() < 0.05);
        assert!((closed[1].start_sample as f64 / 16_000.0 - 20.0).abs() < 0.05);
    }

    #[test]
    fn chunk_start_skips_leading_silence() {
        let mut chunker = chunker();
        chunker.push(&silence(3.0));
        chunker.push(&speech(7.0));
        let closed = chunker.push(&silence(1.0));
        assert_eq!(closed[0].start_sample, 48_000);
    }

    #[test]
    fn frames_split_across_pushes_are_reassembled() {
        let mut chunker = chunker();
        let audio = [speech(7.0), silence(1.0)].concat();
        let closed: Vec<Chunk> = audio.chunks(333).flat_map(|part| chunker.push(part)).collect();
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].start_sample, 0);
    }

    #[test]
    fn flush_returns_the_unfinished_chunk() {
        let mut chunker = chunker();
        chunker.push(&speech(1.5));
        let chunk = chunker.flush().expect("speech in progress");
        assert!((seconds(&chunk) - 1.5).abs() < 0.05);
        assert_eq!(chunker.flush(), None);
    }
}
