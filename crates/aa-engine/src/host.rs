//! The CoreAudio output (through cpal). The engine moves into the audio callback.

use crate::engine::Engine;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::fmt;

#[derive(Debug)]
pub enum HostError {
    NoDevice,
    Config(String),
    Stream(String),
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostError::NoDevice => write!(f, "no audio output device"),
            HostError::Config(e) => write!(f, "audio device setup failed: {e}"),
            HostError::Stream(e) => write!(f, "audio stream failed: {e}"),
        }
    }
}

impl std::error::Error for HostError {}

/// Names of the output devices, default first.
pub fn output_devices() -> Vec<String> {
    let host = cpal::default_host();
    let default = host.default_output_device().and_then(|d| d.name().ok());
    let mut names: Vec<String> = host
        .output_devices()
        .map(|ds| ds.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    if let Some(d) = default {
        names.retain(|n| *n != d);
        names.insert(0, d);
    }
    names
}

/// A running output stream. Dropping it stops the sound.
pub struct AudioHost {
    _stream: cpal::Stream,
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub buffer_frames: Option<u32>,
}

impl AudioHost {
    /// The sample rate the device would run at, so the engine can be built for it.
    pub fn probe(device: Option<&str>) -> Result<u32, HostError> {
        let (_, config) = pick(device)?;
        Ok(config.sample_rate().0)
    }

    /// Opens `device` (substring match; `None` = system default) and starts `engine` on it.
    /// `buffer_frames` asks for a fixed buffer size (lower = less latency, more CPU).
    pub fn start(
        device: Option<&str>,
        buffer_frames: Option<u32>,
        mut engine: Engine,
    ) -> Result<AudioHost, HostError> {
        let (dev, supported) = pick(device)?;
        let device_name = dev.name().unwrap_or_else(|_| "Unknown device".into());
        let mut config: cpal::StreamConfig = supported.config();
        if let Some(frames) = buffer_frames {
            config.buffer_size = cpal::BufferSize::Fixed(frames);
        }
        let channels = config.channels;
        let sample_rate = config.sample_rate.0;
        if (engine.sample_rate() - sample_rate as f32).abs() > 0.5 {
            return Err(HostError::Config(format!(
                "engine built for {} Hz, device runs at {sample_rate} Hz",
                engine.sample_rate()
            )));
        }
        let ch = usize::from(channels);
        let stream = dev
            .build_output_stream(
                &config,
                move |data: &mut [f32], _| engine.process(data, ch),
                |err| eprintln!("audio stream error: {err}"),
                None,
            )
            .map_err(|e| HostError::Stream(e.to_string()))?;
        stream
            .play()
            .map_err(|e| HostError::Stream(e.to_string()))?;
        Ok(AudioHost {
            _stream: stream,
            device_name,
            sample_rate,
            channels,
            buffer_frames,
        })
    }
}

fn pick(device: Option<&str>) -> Result<(cpal::Device, cpal::SupportedStreamConfig), HostError> {
    let host = cpal::default_host();
    let dev = match device {
        Some(want) => host
            .output_devices()
            .map_err(|e| HostError::Config(e.to_string()))?
            .find(|d| d.name().is_ok_and(|n| n.contains(want)))
            .or_else(|| host.default_output_device()),
        None => host.default_output_device(),
    }
    .ok_or(HostError::NoDevice)?;
    let supported = dev
        .default_output_config()
        .map_err(|e| HostError::Config(e.to_string()))?;
    if supported.sample_format() != cpal::SampleFormat::F32 {
        return Err(HostError::Config(format!(
            "unsupported sample format {:?}",
            supported.sample_format()
        )));
    }
    Ok((dev, supported))
}
