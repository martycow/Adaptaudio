//! A project with its sample data loaded, ready for the audio thread.

use aa_core::{Instrument, Project};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Mono sample frames at their own sample rate.
#[derive(Debug)]
pub struct SampleData {
    pub frames: Vec<f32>,
    pub sample_rate: f32,
}

/// What the audio thread plays: the model plus every WAV its sampler zones use.
/// Built (and dropped) off the audio thread.
#[derive(Debug)]
pub struct LoadedProject {
    pub project: Project,
    pub dir: Option<PathBuf>,
    /// `zone_samples[track][zone]` indexes `samples`; `None` when the file is missing.
    pub zone_samples: Vec<Vec<Option<usize>>>,
    pub samples: Vec<Arc<SampleData>>,
    /// Files that failed to load, for the UI.
    pub missing: Vec<String>,
    /// The zone path each entry of `samples` came from.
    pub(crate) paths: Vec<String>,
}

impl LoadedProject {
    /// A project without sample files (all sampler zones silent).
    pub fn without_samples(project: Project) -> Self {
        let zone_samples = project
            .tracks
            .iter()
            .map(|t| zone_count(&t.instrument))
            .map(|n| vec![None; n])
            .collect();
        LoadedProject {
            project,
            dir: None,
            zone_samples,
            samples: Vec::new(),
            missing: Vec::new(),
            paths: Vec::new(),
        }
    }

    /// Loads every sampler zone's WAV from `dir`, reusing `previous` data for unchanged paths.
    pub fn load(project: Project, dir: Option<&Path>, previous: Option<&LoadedProject>) -> Self {
        let mut samples: Vec<Arc<SampleData>> = Vec::new();
        let mut paths: Vec<String> = Vec::new();
        let mut missing = Vec::new();
        let mut zone_samples = Vec::with_capacity(project.tracks.len());
        for track in &project.tracks {
            let mut zones = Vec::new();
            if let Instrument::Sampler(s) = &track.instrument {
                for zone in &s.zones {
                    let index = if let Some(i) = paths.iter().position(|p| *p == zone.path) {
                        Some(i)
                    } else if let Some(data) = previous.and_then(|p| p.find(&zone.path)) {
                        paths.push(zone.path.clone());
                        samples.push(data);
                        Some(samples.len() - 1)
                    } else {
                        match dir
                            .map(|d| d.join(&zone.path))
                            .ok_or(())
                            .and_then(|p| read_wav(&p).map_err(|_| ()))
                        {
                            Ok(data) => {
                                paths.push(zone.path.clone());
                                samples.push(Arc::new(data));
                                Some(samples.len() - 1)
                            }
                            Err(()) => {
                                missing.push(zone.path.clone());
                                None
                            }
                        }
                    };
                    zones.push(index);
                }
            }
            zone_samples.push(zones);
        }
        LoadedProject {
            project,
            dir: dir.map(Path::to_path_buf),
            zone_samples,
            samples,
            missing,
            paths,
        }
    }

    fn find(&self, path: &str) -> Option<Arc<SampleData>> {
        self.paths
            .iter()
            .position(|p| p == path)
            .and_then(|i| self.samples.get(i).cloned())
    }
}

impl LoadedProject {
    pub(crate) fn clone_shallow(&self) -> LoadedProject {
        LoadedProject {
            project: self.project.clone(),
            dir: self.dir.clone(),
            zone_samples: self.zone_samples.clone(),
            samples: self.samples.clone(),
            missing: self.missing.clone(),
            paths: self.paths.clone(),
        }
    }
}

fn zone_count(instrument: &Instrument) -> usize {
    match instrument {
        Instrument::Sampler(s) => s.zones.len(),
        _ => 0,
    }
}

/// Reads a WAV into mono f32 frames (stereo and wider are averaged).
pub fn read_wav(path: &Path) -> Result<SampleData, hound::Error> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels.max(1));
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
    let frames = interleaved
        .chunks(channels)
        .map(|c| c.iter().sum::<f32>() / channels as f32)
        .collect();
    Ok(SampleData {
        frames,
        sample_rate: spec.sample_rate as f32,
    })
}

/// Writes interleaved stereo f32 as a 32-bit float WAV.
pub fn write_wav_stereo(
    path: &Path,
    interleaved: &[f32],
    sample_rate: u32,
) -> Result<(), hound::Error> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec)?;
    for &s in interleaved {
        w.write_sample(s)?;
    }
    w.finalize()
}
