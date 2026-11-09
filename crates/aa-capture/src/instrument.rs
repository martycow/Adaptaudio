//! Turning captured notes into a multisample [`SamplerInstrument`].

use crate::analysis::find_loop_points;
use crate::error::CaptureError;
use crate::slice::CapturedNote;
use crate::wav::write_wav;
use aa_core::io::SAMPLES_DIR;
use aa_core::music::note_name;
use aa_core::{Adsr, SampleZone, SamplerInstrument};
use std::path::{Path, PathBuf};

/// Key range `(low, high)` (both included) for each root note, split halfway between
/// neighbouring roots. The lowest root reaches down to note 0 and the highest up to 127.
/// The result lines up with `roots`, which can be in any order.
///
/// Roots 36 and 39 split as 0..=37 and 38..=127. When two roots are an even distance apart,
/// the note in the middle goes to the lower root.
pub fn zone_ranges(roots: &[u8]) -> Vec<(u8, u8)> {
    roots
        .iter()
        .map(|&root| {
            let below = roots.iter().copied().filter(|&r| r < root).max();
            let above = roots.iter().copied().filter(|&r| r > root).min();
            let low = below.map_or(0, |b| midpoint(b, root) + 1);
            let high = above.map_or(127, |a| midpoint(root, a));
            (low, high)
        })
        .collect()
}

fn midpoint(a: u8, b: u8) -> u8 {
    ((u16::from(a) + u16::from(b)) / 2) as u8
}

/// Velocity range `(low, high)` in 0..1 for each captured MIDI velocity, split halfway between
/// neighbours. The softest layer starts at 0 and the loudest ends at 1; neighbours share their
/// boundary. Lines up with `velocities`, which can be in any order.
pub fn velocity_ranges(velocities: &[u8]) -> Vec<(f32, f32)> {
    let unit = |v: f32| (v / 127.0).clamp(0.0, 1.0);
    velocities
        .iter()
        .map(|&v| {
            let below = velocities.iter().copied().filter(|&o| o < v).max();
            let above = velocities.iter().copied().filter(|&o| o > v).min();
            let low = below.map_or(0.0, |b| unit((f32::from(b) + f32::from(v)) / 2.0));
            let high = above.map_or(1.0, |a| unit((f32::from(v) + f32::from(a)) / 2.0));
            (low, high)
        })
        .collect()
}

/// Turns any text into something safe in a file name: letters, digits, `-` and `_`.
fn file_safe(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "capture".to_string()
    } else {
        cleaned
    }
}

/// File name of one sample, e.g. `hydra_pad_C#3_v90` becomes `hydra_pad_Cs3_v90.wav`
/// (`#` is spelled `s` to keep names safe everywhere).
pub fn sample_file_name(prefix: &str, note: u8, velocity: u8) -> String {
    let note = note_name(note).replace('#', "s");
    format!("{}_{note}_v{velocity}.wav", file_safe(prefix))
}

/// `base`, or `base_2`, `base_3`... whichever no sample file in `<project_dir>/samples/` uses
/// yet. Use it to capture the same synth again without touching earlier recordings.
pub fn unused_prefix(project_dir: &Path, base: &str) -> String {
    let base = file_safe(base);
    let existing: Vec<String> = std::fs::read_dir(project_dir.join(SAMPLES_DIR))
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let taken = |prefix: &str| {
        existing
            .iter()
            .any(|name| name.starts_with(&format!("{prefix}_")))
    };
    if !taken(&base) {
        return base;
    }
    (2u32..)
        .map(|n| format!("{base}_{n}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or(base)
}

/// The envelope a fresh captured instrument starts with: the recording already holds the
/// synth's own attack and decay, so only a short release is added.
pub const CAPTURED_AMP_ENV: Adsr = Adsr::new(0.002, 0.0, 1.0, 0.3);

/// Writes each captured note as a mono 32-bit float WAV into `<project_dir>/samples/` and
/// returns an instrument whose zones point at them.
///
/// Key ranges are split between neighbouring roots ([`zone_ranges`]) and, per root, velocity
/// ranges between the captured velocities ([`velocity_ranges`]). Loop points are searched in
/// each sample. Nothing is written if any target file already exists.
pub fn build_instrument(
    name: &str,
    captured: &[CapturedNote],
    project_dir: &Path,
    file_prefix: &str,
) -> Result<SamplerInstrument, CaptureError> {
    if captured.is_empty() {
        return Err(CaptureError::NothingCaptured);
    }
    let samples_dir = project_dir.join(SAMPLES_DIR);

    let mut files: Vec<(String, PathBuf)> = Vec::with_capacity(captured.len());
    for (i, note) in captured.iter().enumerate() {
        if captured[..i]
            .iter()
            .any(|n| n.note == note.note && n.velocity == note.velocity)
        {
            return Err(CaptureError::DuplicateTake {
                note: note.note,
                velocity: note.velocity,
            });
        }
        let file_name = sample_file_name(file_prefix, note.note, note.velocity);
        let path = samples_dir.join(&file_name);
        if path.exists() {
            return Err(CaptureError::FileExists(path));
        }
        files.push((format!("{SAMPLES_DIR}/{file_name}"), path));
    }

    let mut roots: Vec<u8> = captured.iter().map(|n| n.note).collect();
    roots.sort_unstable();
    roots.dedup();
    let key_ranges = zone_ranges(&roots);

    std::fs::create_dir_all(&samples_dir)?;
    let mut zones = Vec::with_capacity(captured.len());
    for (note, (relative_path, path)) in captured.iter().zip(&files) {
        write_wav(path, &note.samples, note.sample_rate)?;

        let root_index = roots.iter().position(|&r| r == note.note).unwrap_or(0);
        let (low_note, high_note) = key_ranges[root_index];
        let layer: Vec<u8> = captured
            .iter()
            .filter(|n| n.note == note.note)
            .map(|n| n.velocity)
            .collect();
        let layer_index = layer.iter().position(|&v| v == note.velocity).unwrap_or(0);
        let (low_velocity, high_velocity) = velocity_ranges(&layer)[layer_index];

        zones.push(SampleZone {
            path: relative_path.clone(),
            root_note: note.note,
            low_note,
            high_note,
            low_velocity,
            high_velocity,
            loop_points: find_loop_points(&note.samples, note.sample_rate),
        });
    }
    zones.sort_by(|a, b| {
        (a.root_note, a.low_velocity.to_bits()).cmp(&(b.root_note, b.low_velocity.to_bits()))
    });

    Ok(SamplerInstrument {
        name: name.to_string(),
        zones,
        amp_env: CAPTURED_AMP_ENV,
        gain_db: 0.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::tests::tone;
    use crate::test_util::temp_dir;
    use crate::wav::load_wav;

    #[test]
    fn key_ranges_split_halfway_between_roots() {
        assert_eq!(zone_ranges(&[60]), vec![(0, 127)]);
        assert_eq!(
            zone_ranges(&[36, 39, 42]),
            vec![(0, 37), (38, 40), (41, 127)]
        );
        // Even gaps: the middle note goes down.
        assert_eq!(zone_ranges(&[36, 40]), vec![(0, 38), (39, 127)]);
        // Neighbouring semitones.
        assert_eq!(zone_ranges(&[60, 61]), vec![(0, 60), (61, 127)]);
        // Order does not matter.
        assert_eq!(
            zone_ranges(&[42, 36, 39]),
            vec![(41, 127), (0, 37), (38, 40)]
        );
        assert_eq!(zone_ranges(&[0, 127]), vec![(0, 63), (64, 127)]);
        assert!(zone_ranges(&[]).is_empty());
    }

    #[test]
    fn key_ranges_cover_every_note_once() {
        let roots: Vec<u8> = (36..=84).step_by(3).collect();
        let ranges = zone_ranges(&roots);
        for note in 0..=127u8 {
            let owners = ranges
                .iter()
                .filter(|(lo, hi)| (*lo..=*hi).contains(&note))
                .count();
            assert_eq!(owners, 1, "note {note}");
        }
        for (root, (lo, hi)) in roots.iter().zip(&ranges) {
            assert!(lo <= root && root <= hi);
        }
    }

    #[test]
    fn velocity_ranges_split_between_layers() {
        let ranges = velocity_ranges(&[45, 90, 127]);
        assert_eq!(ranges[0].0, 0.0);
        assert!((ranges[0].1 - 67.5 / 127.0).abs() < 1e-6);
        assert_eq!(ranges[0].1, ranges[1].0);
        assert!((ranges[1].1 - 108.5 / 127.0).abs() < 1e-6);
        assert_eq!(ranges[2].1, 1.0);
        assert_eq!(velocity_ranges(&[100]), vec![(0.0, 1.0)]);
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(
            sample_file_name("hydra pad", 61, 90),
            "hydra_pad_Cs4_v90.wav"
        );
        assert_eq!(sample_file_name("", 60, 1), "capture_C4_v1.wav");
        assert_eq!(sample_file_name("../evil", 60, 1), "___evil_C4_v1.wav");
    }

    fn note(note: u8, velocity: u8) -> CapturedNote {
        CapturedNote {
            note,
            velocity,
            sample_rate: 48_000,
            samples: tone(220.0, 0.6, f32::from(velocity) / 127.0, 48_000),
        }
    }

    #[test]
    fn builds_zones_and_writes_wavs() {
        let project = temp_dir("build");
        let captured = vec![note(48, 50), note(48, 110), note(51, 50), note(51, 110)];
        let inst = build_instrument("Hydra Pad", &captured, &project, "hydra").unwrap();
        assert_eq!(inst.name, "Hydra Pad");
        assert_eq!(inst.zones.len(), 4);
        let first = &inst.zones[0];
        assert_eq!(first.path, "samples/hydra_C3_v50.wav");
        assert_eq!(
            (first.root_note, first.low_note, first.high_note),
            (48, 0, 49)
        );
        assert_eq!(first.low_velocity, 0.0);
        assert!((first.high_velocity - 80.0 / 127.0).abs() < 1e-6);
        let last = &inst.zones[3];
        assert_eq!(
            (last.root_note, last.low_note, last.high_note),
            (51, 50, 127)
        );
        assert_eq!(last.high_velocity, 1.0);
        assert!(first.loop_points.is_some(), "a held tone gets a loop");

        let (samples, rate) = load_wav(&project.join(&first.path)).unwrap();
        assert_eq!(rate, 48_000);
        assert_eq!(samples, captured[0].samples);

        // A second capture with the same prefix must not overwrite the first.
        let again = build_instrument("Hydra Pad", &captured, &project, "hydra");
        assert!(matches!(again, Err(CaptureError::FileExists(_))));
        let prefix = unused_prefix(&project, "hydra");
        assert_eq!(prefix, "hydra_2");
        assert!(build_instrument("Hydra Pad 2", &captured, &project, &prefix).is_ok());
        assert_eq!(unused_prefix(&project, "hydra"), "hydra_3");
        assert_eq!(unused_prefix(&project, "freak"), "freak");
        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn empty_and_duplicate_captures_are_refused() {
        let project = temp_dir("refuse");
        assert!(matches!(
            build_instrument("x", &[], &project, "x"),
            Err(CaptureError::NothingCaptured)
        ));
        let dup = vec![note(60, 100), note(60, 100)];
        assert!(matches!(
            build_instrument("x", &dup, &project, "x"),
            Err(CaptureError::DuplicateTake {
                note: 60,
                velocity: 100
            })
        ));
        assert!(!project.join(SAMPLES_DIR).exists(), "nothing written");
        std::fs::remove_dir_all(project).unwrap();
    }
}
