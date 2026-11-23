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

/// Reads the PCM of a canonical WAV file by hand, so that a recording that is still being
/// written (its header has no length yet) can be read up to what has reached the disk.
fn read_wav(path: &Path) -> Option<Result<Vec<f32>, String>> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let (mut offset, mut format_ok) = (12, false);
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let declared = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?) as usize;
        let body = offset + 8;
        if id == b"fmt " && body + 16 <= bytes.len() {
            let field = |at: usize| u16::from_le_bytes([bytes[body + at], bytes[body + at + 1]]);
            let rate = u32::from_le_bytes(bytes[body + 4..body + 8].try_into().ok()?);
            format_ok = field(0) == 1 && field(2) == 1 && rate == SAMPLE_RATE && field(14) == 16;
        } else if id == b"data" {
            if !format_ok {
                return Some(Err(NOT_OURS.into()));
            }
            let available = bytes.len() - body;
            // Zero (still recording) or too long a length means "until the end of the file".
            let length = if declared == 0 || declared > available { available } else { declared };
            let samples = bytes[body..body + length]
                .chunks_exact(2)
                .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / 32768.0)
                .collect();
            return Some(Ok(samples));
        }
        offset = body + declared + (declared & 1);
    }
    None
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
    fn a_wav_still_being_written_is_read_up_to_what_is_there() {
        let dir = std::env::temp_dir().join("notetaker-audio-file-growing");
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("growing.wav");
        let mut recorder = crate::audio::WavRecorder::create(&wav).unwrap();
        recorder.write(&vec![0.25; 40_000]).unwrap();
        // Not finished: the header holds no length. Whatever was flushed is readable.
        let samples = read_samples(&wav).unwrap();
        assert!(samples.len() <= 40_000 && samples.iter().all(|s| (s - 0.25).abs() < 0.001));
        recorder.finish().unwrap();
        assert_eq!(read_samples(&wav).unwrap().len(), 40_000);
    }

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
