//! Offline transcription with whisper.cpp. Nothing leaves the machine.
//! Compiled only with the `local-whisper` feature, which needs cmake and
//! libclang (LLVM) at build time.

use std::path::Path;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::transcript::{AudioChunk, TranscribeError, Transcriber, TranscriptSegment};

pub struct WhisperTranscriber {
    context: WhisperContext,
    language: String,
    threads: i32,
}

impl WhisperTranscriber {
    /// `model` is a ggml model file; `language` an ISO 639-1 code such as "id".
    pub fn new(model: &Path, language: String) -> Result<Self, TranscribeError> {
        let model = model.to_str().ok_or_else(|| TranscribeError::Local("model path is not valid UTF-8".into()))?;
        let context = WhisperContext::new_with_params(model, WhisperContextParameters::default()).map_err(local)?;
        let threads = std::thread::available_parallelism().map_or(4, |count| count.get() as i32);
        Ok(Self { context, language, threads })
    }
}

impl Transcriber for WhisperTranscriber {
    fn transcribe(&self, chunk: &AudioChunk) -> Result<Vec<TranscriptSegment>, TranscribeError> {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(&self.language));
        params.set_n_threads(self.threads);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        let mut state = self.context.create_state().map_err(local)?;
        state.full(params, &chunk.samples).map_err(local)?;

        Ok(state
            .as_iter()
            .map(|segment| TranscriptSegment {
                source: chunk.source,
                start_secs: chunk.start_secs + centiseconds_to_seconds(segment.start_timestamp()),
                end_secs: chunk.start_secs + centiseconds_to_seconds(segment.end_timestamp()),
                text: segment.to_string().trim().to_owned(),
                speaker: None,
            })
            .filter(|segment| !segment.text.is_empty())
            .collect())
    }
}

fn centiseconds_to_seconds(centiseconds: i64) -> f64 {
    centiseconds as f64 / 100.0
}

fn local(error: impl std::fmt::Display) -> TranscribeError {
    TranscribeError::Local(error.to_string())
}
