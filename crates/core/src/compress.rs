//! Lossless compression of a finished meeting's recordings. They are written
//! as WAV while recording and turned into FLAC afterwards, when nothing needs
//! to read them as WAV any more. FLAC is read by the importer and the diarizer
//! as well, and is about half the size for speech.

use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use flacenc::component::BitRepr;
use flacenc::error::{SourceError, SourceErrorReason, Verify};
use flacenc::source::{Fill, Source};

/// The recordings a meeting folder holds while recording, by name without extension.
const RECORDINGS: [&str; 2] = ["microphone", "system"];

#[derive(Debug, thiserror::Error)]
#[error("kompresi audio gagal ({path}): {reason}")]
pub struct CompressError {
    path: String,
    reason: String,
}

impl CompressError {
    fn new(path: &Path, reason: impl std::fmt::Display) -> Self {
        Self { path: path.display().to_string(), reason: reason.to_string() }
    }
}

/// Turns each `microphone.wav` and `system.wav` in `audio_dir` into a `.flac` and removes the WAV.
/// Returns the bytes saved. A recording that fails stays as WAV and the error is returned.
pub fn compress_recordings(audio_dir: &Path) -> Result<u64, CompressError> {
    let mut saved = 0;
    for name in RECORDINGS {
        let wav = audio_dir.join(format!("{name}.wav"));
        if !wav.exists() {
            continue;
        }
        let flac = wav.with_extension("flac");
        let before = fs::metadata(&wav).map_err(|error| CompressError::new(&wav, error))?.len();
        wav_to_flac(&wav, &flac)?;
        let after = fs::metadata(&flac).map_err(|error| CompressError::new(&flac, error))?.len();
        fs::remove_file(&wav).map_err(|error| CompressError::new(&wav, error))?;
        saved += before.saturating_sub(after);
    }
    Ok(saved)
}

/// Encodes a 16-bit PCM WAV file as FLAC. The WAV is read block by block, so a long
/// meeting is not held in memory twice. The FLAC is written under a temporary name first.
pub fn wav_to_flac(wav: &Path, flac: &Path) -> Result<(), CompressError> {
    let fail = |reason: &dyn std::fmt::Display| CompressError::new(wav, reason);
    let reader = hound::WavReader::open(wav).map_err(|error| fail(&error))?;
    let spec = reader.spec();
    if spec.sample_format != hound::SampleFormat::Int || spec.bits_per_sample != 16 {
        return Err(fail(&"hanya WAV 16-bit yang didukung"));
    }
    let channels = usize::from(spec.channels);
    let frames = reader.len() as usize / channels;
    let source = WavSource { reader, channels, sample_rate: spec.sample_rate as usize, frames, buffer: Vec::new() };

    let config = flacenc::config::Encoder::default().into_verified().map_err(|(_, error)| fail(&format!("{error:?}")))?;
    let mut stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|error| fail(&error))?;
    // The encoder records a shorter last block as the minimum block size, which makes
    // the decoder treat the stream as variable-block and fail with an unexpected end of file.
    stream
        .stream_info_mut()
        .set_block_sizes(config.block_size, config.block_size)
        .map_err(|error| fail(&format!("{error:?}")))?;
    let mut bytes = flacenc::bitsink::ByteSink::new();
    stream.write(&mut bytes).map_err(|error| fail(&error))?;

    let temporary: PathBuf = flac.with_extension("flac.tmp");
    fs::write(&temporary, bytes.as_slice()).map_err(|error| CompressError::new(&temporary, error))?;
    fs::rename(&temporary, flac).map_err(|error| CompressError::new(flac, error))
}

/// Feeds the samples of a WAV file to the encoder.
struct WavSource {
    reader: hound::WavReader<BufReader<File>>,
    channels: usize,
    sample_rate: usize,
    /// Samples per channel in the whole file. `reader.len()` only counts what is left.
    frames: usize,
    buffer: Vec<i32>,
}

impl Source for WavSource {
    fn channels(&self) -> usize {
        self.channels
    }

    fn bits_per_sample(&self) -> usize {
        16
    }

    fn sample_rate(&self) -> usize {
        self.sample_rate
    }

    fn read_samples<F: Fill>(&mut self, block_size: usize, dest: &mut F) -> Result<usize, SourceError> {
        self.buffer.clear();
        for sample in self.reader.samples::<i16>().take(block_size * self.channels) {
            let sample = sample.map_err(|_| SourceError::by_reason(SourceErrorReason::InvalidFormat))?;
            self.buffer.push(i32::from(sample));
        }
        dest.fill_interleaved(&self.buffer)?;
        Ok(self.buffer.len() / self.channels)
    }

    fn len_hint(&self) -> Option<usize> {
        Some(self.frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::WavRecorder;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("compress-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Speech-like: a few tones, slowly changing, with a little noise; 3 seconds at 16 kHz.
    fn write_recording(path: &Path) {
        let mut recorder = WavRecorder::create(path).unwrap();
        let mut noise: u32 = 1;
        let samples: Vec<f32> = (0..48_000)
            .map(|i| {
                noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let t = i as f32 / 16_000.0;
                0.3 * (t * 220.0 * std::f32::consts::TAU).sin()
                    + 0.1 * (t * 1_300.0 * std::f32::consts::TAU).sin()
                    + 0.01 * ((noise >> 16) as f32 / 32_768.0 - 1.0)
            })
            .collect();
        recorder.write(&samples).unwrap();
        recorder.finish().unwrap();
    }

    #[test]
    fn the_flac_decodes_to_exactly_the_same_samples_and_is_smaller() {
        let dir = folder("roundtrip");
        let (wav, flac) = (dir.join("system.wav"), dir.join("system.flac"));
        write_recording(&wav);

        wav_to_flac(&wav, &flac).unwrap();

        let (original, original_rate) = polyvoice::wav::load_audio(&wav).unwrap();
        let (decoded, decoded_rate) = polyvoice::wav::load_audio(&flac).unwrap();
        assert_eq!((original.len(), original_rate), (decoded.len(), decoded_rate));
        assert_eq!(original, decoded, "FLAC is lossless");
        assert!(fs::metadata(&flac).unwrap().len() < fs::metadata(&wav).unwrap().len());
        assert!(!dir.join("system.flac.tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_meeting_folder_ends_up_with_flac_only_and_reports_the_saving() {
        let dir = folder("folder");
        write_recording(&dir.join("system.wav"));
        // The microphone was never recorded; only what exists is compressed.
        let saved = compress_recordings(&dir).unwrap();

        assert!(saved > 0);
        assert!(dir.join("system.flac").exists() && !dir.join("system.wav").exists());
        assert!(!dir.join("microphone.flac").exists());
        assert_eq!(compress_recordings(&dir).unwrap(), 0, "nothing left to compress");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_recording_stays_as_it_was() {
        let dir = folder("broken");
        fs::write(dir.join("system.wav"), b"not a wav file").unwrap();

        let error = compress_recordings(&dir).unwrap_err();

        assert!(error.to_string().contains("system.wav"));
        assert!(dir.join("system.wav").exists() && !dir.join("system.flac").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
