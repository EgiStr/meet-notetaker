//! Reads the recordings this app wrote, of any length. The general decoder of the diarization
//! crate refuses files over an hour, which a long meeting easily is.

use std::path::Path;

use crate::audio::SAMPLE_RATE;

/// 16 kHz mono samples of a recording: our own WAV and FLAC files are read directly, other
/// formats (an imported mp3, m4a, ...) go through the general decoder.
pub fn read_samples(path: &Path) -> Result<Vec<f32>, String> {
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let own = match extension.as_str() {
        "wav" => read_wav(path),
        "flac" => read_flac(path),
        _ => None,
    };
    if let Some(result) = own {
        match result {
            Ok(samples) => return Ok(samples),
            // Not 16 kHz mono: let the general decoder resample it.
            Err(reason) if reason != NOT_OURS => return Err(reason),
            Err(_) => {}
        }
    }
    let (samples, rate) = polyvoice::wav::load_audio(path).map_err(|error| error.to_string())?;
    if rate != SAMPLE_RATE {
        return Err(format!("decoded at {rate} Hz instead of {SAMPLE_RATE} Hz"));
    }
    Ok(samples)
}

const NOT_OURS: &str = "not 16 kHz mono 16-bit";

fn read_wav(path: &Path) -> Option<Result<Vec<f32>, String>> {
    let mut reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.sample_rate != SAMPLE_RATE || spec.sample_format != hound::SampleFormat::Int || spec.bits_per_sample != 16 {
        return Some(Err(NOT_OURS.into()));
    }
    Some(reader.samples::<i16>().map(|s| s.map(|v| f32::from(v) / 32768.0).map_err(|e| e.to_string())).collect())
}

fn read_flac(path: &Path) -> Option<Result<Vec<f32>, String>> {
    let mut reader = claxon::FlacReader::open(path).ok()?;
    let info = reader.streaminfo();
    if info.channels != 1 || info.sample_rate != SAMPLE_RATE || info.bits_per_sample != 16 {
        return Some(Err(NOT_OURS.into()));
    }
    Some(reader.samples().map(|s| s.map(|v| v as f32 / 32768.0).map_err(|e| e.to_string())).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_and_its_flac_read_back_the_same_samples() {
        let dir = std::env::temp_dir().join("notetaker-audio-file-test");
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("a.wav");
        let spec = hound::WavSpec { channels: 1, sample_rate: SAMPLE_RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut writer = hound::WavWriter::create(&wav, spec).unwrap();
        for i in 0..20_000 {
            writer.write_sample(((i % 200) as i16 - 100) * 100).unwrap();
        }
        writer.finalize().unwrap();
        let flac = dir.join("a.flac");
        crate::compress::wav_to_flac(&wav, &flac).unwrap();
        let from_wav = read_samples(&wav).unwrap();
        assert_eq!(from_wav.len(), 20_000);
        assert_eq!(read_samples(&flac).unwrap(), from_wav);
    }
}
