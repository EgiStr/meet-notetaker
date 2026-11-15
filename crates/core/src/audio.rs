//! Sample-level helpers. Everything downstream of capture works on 16 kHz mono f32.

use std::fs::File;
use std::io::{BufWriter, Cursor, Seek, Write};
use std::path::Path;

pub const SAMPLE_RATE: u32 = 16_000;

pub fn seconds_to_samples(seconds: f64) -> usize {
    (seconds * f64::from(SAMPLE_RATE)).round() as usize
}

pub fn samples_to_seconds(samples: u64) -> f64 {
    samples as f64 / f64::from(SAMPLE_RATE)
}

/// Averages interleaved channels into one.
pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Streaming downsampler to 16 kHz. Each output sample is the mean of the
/// input samples it covers, which is exact for 48 kHz and keeps the average
/// rate correct for non-integer ratios such as 44.1 kHz.
pub struct Downsampler {
    inputs_per_output: f64,
    position: f64,
    sum: f32,
    count: u32,
}

impl Downsampler {
    pub fn new(input_rate: u32) -> Self {
        Self {
            inputs_per_output: f64::from(input_rate) / f64::from(SAMPLE_RATE),
            position: 0.0,
            sum: 0.0,
            count: 0,
        }
    }

    pub fn push(&mut self, input: &[f32], output: &mut Vec<f32>) {
        for &sample in input {
            self.sum += sample;
            self.count += 1;
            self.position += 1.0;
            if self.position >= self.inputs_per_output {
                output.push(self.sum / self.count as f32);
                self.position -= self.inputs_per_output;
                self.sum = 0.0;
                self.count = 0;
            }
        }
    }
}

/// Encodes 16 kHz mono samples as a 16-bit PCM WAV file in memory.
pub fn encode_wav(samples: &[f32]) -> Result<Vec<u8>, hound::Error> {
    let mut bytes = Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(&mut bytes, WAV_SPEC)?;
    write_samples(&mut writer, samples)?;
    writer.finalize()?;
    Ok(bytes.into_inner())
}

const WAV_SPEC: hound::WavSpec = hound::WavSpec {
    channels: 1,
    sample_rate: SAMPLE_RATE,
    bits_per_sample: 16,
    sample_format: hound::SampleFormat::Int,
};

fn write_samples<W: Write + Seek>(writer: &mut hound::WavWriter<W>, samples: &[f32]) -> Result<(), hound::Error> {
    samples
        .iter()
        .try_for_each(|sample| writer.write_sample((sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16))
}

/// Writes one stream of a meeting to a 16 kHz mono WAV file as it is captured.
pub struct WavRecorder(hound::WavWriter<BufWriter<File>>);

impl WavRecorder {
    pub fn create(path: &Path) -> Result<Self, hound::Error> {
        hound::WavWriter::create(path, WAV_SPEC).map(Self)
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<(), hound::Error> {
        write_samples(&mut self.0, samples)
    }

    /// Completes the file header; without this the file is unreadable.
    pub fn finish(self) -> Result<(), hound::Error> {
        self.0.finalize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_each_stereo_frame() {
        assert_eq!(downmix(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
    }

    #[test]
    fn downmix_keeps_mono_unchanged() {
        assert_eq!(downmix(&[0.1, 0.2], 1), vec![0.1, 0.2]);
    }

    #[test]
    fn downsampler_averages_three_samples_from_48k() {
        let mut output = Vec::new();
        Downsampler::new(48_000).push(&[0.0, 0.3, 0.6, 1.0, 1.0, 1.0], &mut output);
        assert_eq!(output.len(), 2);
        assert!((output[0] - 0.3).abs() < 1e-6);
        assert!((output[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn downsampler_carries_state_across_pushes() {
        let mut downsampler = Downsampler::new(48_000);
        let mut output = Vec::new();
        downsampler.push(&[1.0, 1.0], &mut output);
        assert!(output.is_empty());
        downsampler.push(&[1.0], &mut output);
        assert_eq!(output, vec![1.0]);
    }

    #[test]
    fn downsampler_keeps_average_rate_for_44100() {
        let mut output = Vec::new();
        Downsampler::new(44_100).push(&vec![0.0; 44_100], &mut output);
        assert!(output.len().abs_diff(16_000) <= 1, "got {}", output.len());
    }

    #[test]
    fn downsampler_passes_16k_through() {
        let mut output = Vec::new();
        Downsampler::new(16_000).push(&[0.1, 0.2, 0.3], &mut output);
        assert_eq!(output, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn encode_wav_writes_header_and_two_bytes_per_sample() {
        let bytes = encode_wav(&[0.0; 100]).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(bytes.len(), 44 + 200);
    }
}
