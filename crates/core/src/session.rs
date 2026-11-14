//! One recording session: a capture thread per source feeding a single
//! transcription worker, which reports segments in the order chunks close.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::audio::{samples_to_seconds, seconds_to_samples, Downsampler};
use crate::capture::{self, CaptureMessage};
use crate::chunker::{Chunk, Chunker, ChunkerConfig};
use crate::transcript::{AudioChunk, Source, Transcriber, TranscriptSegment};

const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// Loopback delivers nothing while the computer is silent. Once a stream falls
/// this far behind the clock, the gap is filled with silence so that its
/// timestamps stay aligned with the other stream.
const MAX_LAG_SECS: f64 = 0.5;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "lowercase")]
pub enum SessionEvent {
    Segment(TranscriptSegment),
    Error(String),
}

pub type EventSink = Arc<dyn Fn(SessionEvent) + Send + Sync>;

pub struct Session {
    stop: Arc<AtomicBool>,
    capture_threads: Vec<JoinHandle<()>>,
    worker: JoinHandle<()>,
}

impl Session {
    pub fn start(sources: &[Source], transcriber: Arc<dyn Transcriber>, sink: EventSink) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (chunks, closed_chunks) = mpsc::channel::<AudioChunk>();

        let capture_threads = sources
            .iter()
            .map(|&source| {
                let (stop, chunks, sink) = (stop.clone(), chunks.clone(), sink.clone());
                thread::spawn(move || run_capture(source, &stop, &chunks, &sink))
            })
            .collect();

        let worker = thread::spawn(move || {
            for chunk in closed_chunks {
                match transcriber.transcribe(&chunk) {
                    Ok(segments) => segments.into_iter().for_each(|s| sink(SessionEvent::Segment(s))),
                    Err(error) => sink(SessionEvent::Error(error.to_string())),
                }
            }
        });

        Self { stop, capture_threads, worker }
    }

    /// Stops capture, transcribes what is still queued, then returns.
    pub fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.capture_threads {
            let _ = thread.join();
        }
        // Every chunk sender is gone now, so the worker drains the queue and exits.
        let _ = self.worker.join();
    }
}

fn run_capture(source: Source, stop: &AtomicBool, chunks: &Sender<AudioChunk>, sink: &EventSink) {
    let (messages, incoming) = mpsc::channel();
    let capture = match capture::open(source, messages) {
        Ok(capture) => capture,
        Err(error) => {
            sink(SessionEvent::Error(format!("{source:?}: {error}")));
            return;
        }
    };

    let started = Instant::now();
    let max_lag = seconds_to_samples(MAX_LAG_SECS) as u64;
    let mut downsampler = Downsampler::new(capture.sample_rate);
    let mut chunker = Chunker::new(ChunkerConfig::default());
    let mut fed_samples: u64 = 0;
    let mut resampled = Vec::new();

    while !stop.load(Ordering::Relaxed) {
        resampled.clear();
        match incoming.recv_timeout(POLL_INTERVAL) {
            Ok(CaptureMessage::Samples(samples)) => downsampler.push(&samples, &mut resampled),
            Ok(CaptureMessage::Failed(error)) => sink(SessionEvent::Error(format!("{source:?}: {error}"))),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        let expected = seconds_to_samples(started.elapsed().as_secs_f64()) as u64;
        let lag = expected.saturating_sub(fed_samples + resampled.len() as u64);
        if lag > max_lag {
            let silence = vec![0.0; lag as usize];
            fed_samples += lag;
            forward(source, chunker.push(&silence), chunks);
        }
        fed_samples += resampled.len() as u64;
        forward(source, chunker.push(&resampled), chunks);
    }

    drop(capture);
    forward(source, chunker.flush().into_iter().collect(), chunks);
}

fn forward(source: Source, closed: Vec<Chunk>, chunks: &Sender<AudioChunk>) {
    for chunk in closed {
        // The worker only goes away after every capture thread has finished.
        let _ = chunks.send(AudioChunk {
            source,
            start_secs: samples_to_seconds(chunk.start_sample),
            samples: chunk.samples,
        });
    }
}
