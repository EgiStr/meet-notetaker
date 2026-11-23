//! Speaker labels while the meeting is still going. The system audio recorded so far is
//! diarized again every little while and the labels are applied to the transcript, so
//! "Pembicara 1, 2, 3" appear during the meeting instead of only after it. Numbers can shift as
//! more of the meeting is heard; the final pass after the recording stops settles them.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::notes::MeetingNotes;
use crate::speakers::Diarizer;

/// Too little speech to tell voices apart.
const MIN_AUDIO_BYTES: u64 = 20 * 16_000 * 2;

pub struct LiveSpeakers {
    stopped: Arc<AtomicBool>,
    _stop: Sender<()>,
}

impl LiveSpeakers {
    /// `recording` is the system-audio WAV being written. A run takes longer as the meeting grows,
    /// so the wait before the next one is at least twice the last run. `changed` is called after
    /// labels were applied.
    pub fn start(
        notes: Arc<MeetingNotes>,
        recording: PathBuf,
        diarizer: Arc<dyn Diarizer>,
        interval: Duration,
        changed: impl Fn() + Send + 'static,
    ) -> Self {
        let (stop, stopped_signal) = mpsc::channel::<()>();
        let stopped = Arc::new(AtomicBool::new(false));
        let flag = stopped.clone();
        thread::spawn(move || {
            let mut wait = interval;
            let mut last_size = 0;
            while let Err(RecvTimeoutError::Timeout) = stopped_signal.recv_timeout(wait) {
                wait = interval;
                let size = std::fs::metadata(&recording).map_or(0, |meta| meta.len());
                if size < MIN_AUDIO_BYTES || size < last_size + MIN_AUDIO_BYTES / 2 {
                    continue;
                }
                last_size = size;
                let started = Instant::now();
                // A failure is not reported: the final pass will try again and report.
                let Ok(turns) = diarizer.diarize(&recording) else { continue };
                // The recording may have stopped while this ran; the final labels win then.
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                notes.apply_speaker_turns(&turns);
                changed();
                wait = interval.max(started.elapsed() * 2);
            }
        });
        Self { stopped, _stop: stop }
    }

    /// Does not wait for a run in flight; its result is discarded.
    pub fn stop(self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}
