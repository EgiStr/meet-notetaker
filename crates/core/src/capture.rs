//! Opens the default microphone or the default output device (loopback) and
//! delivers mono f32 samples at the device's native rate.

use std::sync::mpsc::Sender;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream, StreamConfig};

use crate::audio::downmix;
use crate::transcript::Source;

pub enum CaptureMessage {
    Samples(Vec<f32>),
    Failed(String),
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("no default {0} device")]
    NoDevice(&'static str),
    #[error("could not read the device format: {0}")]
    Config(#[from] cpal::DefaultStreamConfigError),
    #[error("could not open the stream: {0}")]
    Build(#[from] cpal::BuildStreamError),
    #[error("could not start the stream: {0}")]
    Play(#[from] cpal::PlayStreamError),
    #[error("unsupported sample format {0:?}")]
    UnsupportedFormat(SampleFormat),
}

pub struct OpenCapture {
    /// Capture stops when this is dropped.
    pub stream: Stream,
    pub sample_rate: u32,
}

pub fn open(source: Source, messages: Sender<CaptureMessage>) -> Result<OpenCapture, CaptureError> {
    let host = cpal::default_host();
    // On WASAPI, an input stream opened on an output device records what it plays.
    let (device, supported) = match source {
        Source::Microphone => {
            let device = host.default_input_device().ok_or(CaptureError::NoDevice("input"))?;
            let supported = device.default_input_config()?;
            (device, supported)
        }
        Source::System => {
            let device = host.default_output_device().ok_or(CaptureError::NoDevice("output"))?;
            let supported = device.default_output_config()?;
            (device, supported)
        }
    };

    let config: StreamConfig = supported.config();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &config, messages)?,
        SampleFormat::I16 => build::<i16>(&device, &config, messages)?,
        SampleFormat::U16 => build::<u16>(&device, &config, messages)?,
        SampleFormat::I32 => build::<i32>(&device, &config, messages)?,
        other => return Err(CaptureError::UnsupportedFormat(other)),
    };
    stream.play()?;
    Ok(OpenCapture { stream, sample_rate: config.sample_rate.0 })
}

fn build<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    messages: Sender<CaptureMessage>,
) -> Result<Stream, cpal::BuildStreamError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels);
    let failures = messages.clone();
    device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let interleaved: Vec<f32> = data.iter().map(|sample| sample.to_sample::<f32>()).collect();
            // The receiver is gone only while the session is shutting down.
            let _ = messages.send(CaptureMessage::Samples(downmix(&interleaved, channels)));
        },
        move |error| {
            let _ = failures.send(CaptureMessage::Failed(error.to_string()));
        },
        None,
    )
}
