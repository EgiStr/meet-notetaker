//! Local diarization with the polyvoice crate. Its models are downloaded to
//! the user's cache on first use.

use std::path::Path;

use polyvoice::models::ModelRegistry;
use polyvoice::types::{Profile, SampleRate};
use polyvoice::{Pipeline, PipelineConfig};

use crate::speakers::{DiarizeError, Diarizer, SpeakerTurn};

pub struct PolyvoiceDiarizer;

impl Diarizer for PolyvoiceDiarizer {
    fn diarize(&self, recording: &Path) -> Result<Vec<SpeakerTurn>, DiarizeError> {
        let (samples, rate) = polyvoice::wav::load_audio(recording).map_err(failed)?;
        let rate = SampleRate::new(rate).ok_or_else(|| DiarizeError(format!("unsupported sample rate {rate}")))?;

        let mut config = PipelineConfig::default();
        config.profile = Profile::Balanced;
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
