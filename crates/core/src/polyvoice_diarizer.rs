//! Local diarization with the polyvoice crate. Its models are downloaded to
//! the user's cache on first use.

use std::path::Path;

use polyvoice::models::ModelRegistry;
use polyvoice::types::{Profile, SampleRate};
use polyvoice::{Pipeline, PipelineConfig};

use crate::speakers::{DiarizeError, Diarizer, SpeakerTurn};

/// Long enough for any meeting; memory (4 bytes a sample) is the real limit.
const MAX_MEETING_SECS: usize = 6 * 3600;

pub struct PolyvoiceDiarizer;

impl Diarizer for PolyvoiceDiarizer {
    fn diarize(&self, recording: &Path) -> Result<Vec<SpeakerTurn>, DiarizeError> {
        let samples = crate::audio_file::read_samples(recording).map_err(DiarizeError)?;
        let rate = SampleRate::new(crate::audio::SAMPLE_RATE).ok_or_else(|| DiarizeError("unsupported sample rate".into()))?;

        let mut config = PipelineConfig::default();
        config.profile = Profile::Balanced;
        // The default refuses more than an hour, and the audio here is our own recording.
        config.max_audio_samples = MAX_MEETING_SECS * crate::audio::SAMPLE_RATE as usize;
        let pipeline = Pipeline::builder()
            .config(config)
            .with_models_from(ModelRegistry::default().map_err(failed)?)
            .build()
            .map_err(failed)?;

        let result = pipeline.run(&samples, rate).map_err(failed)?;
        Ok(result
            .turns
            .iter()
            .map(|turn| SpeakerTurn { start_secs: turn.time.start, end_secs: turn.time.end, speaker: turn.speaker.0 })
            .collect())
    }
}

fn failed(error: impl std::fmt::Display) -> DiarizeError {
    DiarizeError(error.to_string())
}
