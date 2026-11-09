//! Reading and writing WAV files.

use crate::analysis::to_mono;
use crate::error::CaptureError;
use std::path::Path;

/// Reads a WAV file as mono samples in -1..1 and its sample rate. Stereo and multichannel
/// files are mixed down. Integer (8 to 32 bit) and 32-bit float files are supported.
pub fn load_wav(path: &Path) -> Result<(Vec<f32>, u32), CaptureError> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<Result<_, _>>()?
        }
    };
    Ok((
        to_mono(&interleaved, usize::from(spec.channels)),
        spec.sample_rate,
    ))
}

/// Writes mono samples as a 32-bit float WAV. Replaces an existing file: callers that must not
/// overwrite (like [`crate::build_instrument`]) check first.
pub fn write_wav(path: &Path, samples: &[f32], sample_rate: u32) -> Result<(), CaptureError> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &s in samples {
        writer.write_sample(s)?;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::temp_dir;

    #[test]
    fn float_wav_round_trips() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("a.wav");
        let samples = vec![0.0, 0.5, -0.25, 1.0, -1.0];
        write_wav(&path, &samples, 44_100).unwrap();
        let (back, rate) = load_wav(&path).unwrap();
        assert_eq!(rate, 44_100);
        assert_eq!(back, samples);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stereo_16_bit_is_mixed_to_mono() {
        let dir = temp_dir("stereo");
        let path = dir.join("s.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for s in [16384i16, 0, -16384, -16384] {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
        let (mono, rate) = load_wav(&path).unwrap();
        assert_eq!(rate, 48_000);
        assert_eq!(mono, vec![0.25, -0.5]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_file_is_an_error() {
        assert!(load_wav(Path::new("/definitely/not/here.wav")).is_err());
    }
}
