//! Cutting one long capture take into a sample per note.

use crate::analysis::{SILENCE_DB, apply_fades, normalize, peak_db, trim_silence};
use crate::plan::{CapturePlan, ms_to_samples};

/// One captured note: mono samples starting at the attack.
#[derive(Clone, Debug, PartialEq)]
pub struct CapturedNote {
    pub note: u8,
    pub velocity: u8,
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

/// A window whose peak is below this is treated as "the synth made no sound".
pub const SILENT_TAKE_DB: f32 = -70.0;
/// Trimming keeps everything louder than this much below the take's own peak.
pub const TRIM_RANGE_DB: f32 = 50.0;
/// Each window starts this much before the expected key-down, in case the latency estimate is
/// a little high. Never more than the plan's gap, so windows do not overlap the previous take.
const WINDOW_LEAD_MS: u32 = 20;
const FADE_IN_MS: f32 = 0.5;
const FADE_OUT_MS: f32 = 10.0;

/// Cuts a continuous recording of `plan` into one [`CapturedNote`] per take.
///
/// Take `i` is expected at its planned start plus `latency_samples`. Each cut is trimmed of
/// silence (relative to its own peak, so soft notes survive) and gets short fades. Silent takes
/// (the synth did not answer) are left out, so the result can be shorter than the plan.
pub fn slice_recording(
    recording: &[f32],
    sample_rate: u32,
    plan: &CapturePlan,
    latency_samples: usize,
) -> Vec<CapturedNote> {
    let lead = ms_to_samples(u64::from(WINDOW_LEAD_MS.min(plan.gap_ms)), sample_rate);
    let slot = ms_to_samples(plan.slot_ms(), sample_rate);
    let mut captured = Vec::with_capacity(plan.take_count());
    for take in plan.takes() {
        let expected = ms_to_samples(take.start_ms, sample_rate) + latency_samples;
        let start = expected.saturating_sub(lead);
        if start >= recording.len() {
            break;
        }
        let end = (start + slot).min(recording.len());
        let window = &recording[start..end];
        let peak = peak_db(window);
        if peak < SILENT_TAKE_DB {
            continue;
        }
        let keep = trim_silence(window, sample_rate, (peak - TRIM_RANGE_DB).max(SILENCE_DB));
        let mut samples = window[keep].to_vec();
        apply_fades(&mut samples, sample_rate, FADE_IN_MS, FADE_OUT_MS);
        captured.push(CapturedNote {
            note: take.note,
            velocity: take.velocity,
            sample_rate,
            samples,
        });
    }
    captured
}

/// Normalizes all notes by one shared gain so the loudest peak lands at `target_db`.
/// Soft velocities stay softer than hard ones. Returns the gain.
pub fn normalize_together(notes: &mut [CapturedNote], target_db: f32) -> f32 {
    let loudest = notes
        .iter()
        .map(|n| peak_db(&n.samples))
        .fold(SILENCE_DB, f32::max);
    if loudest <= SILENCE_DB {
        return 1.0;
    }
    let gain = crate::analysis::db_to_gain(target_db - loudest);
    for note in notes.iter_mut() {
        for s in &mut note.samples {
            *s *= gain;
        }
    }
    gain
}

/// Normalizes every note to `target_db` on its own. Evens out levels but flattens velocity
/// dynamics; prefer [`normalize_together`] for multi-velocity instruments.
pub fn normalize_each(notes: &mut [CapturedNote], target_db: f32) {
    for note in notes {
        normalize(&mut note.samples, target_db);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::tests::tone;
    use crate::analysis::{db_to_gain, detect_onset};

    /// Renders what a synth would do for `plan`, arriving `latency` samples late, over a
    /// little noise. Velocity sets the amplitude.
    fn fake_take(
        plan: &CapturePlan,
        sample_rate: u32,
        latency: usize,
        silent_take: Option<usize>,
    ) -> Vec<f32> {
        let total = ms_to_samples(plan.total_duration().as_millis() as u64, sample_rate) + latency;
        let mut seed = 1u32;
        let mut recording: Vec<f32> = (0..total)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((seed >> 8) as f32 / (1 << 24) as f32 - 0.5) * 2.0 * db_to_gain(-80.0)
            })
            .collect();
        let note_seconds = (plan.hold_ms + plan.release_ms / 2) as f32 / 1000.0;
        for take in plan.takes() {
            if Some(take.index) == silent_take {
                continue;
            }
            let freq = aa_core::music::midi_to_hz(f32::from(take.note));
            let sound = tone(
                freq,
                note_seconds,
                f32::from(take.velocity) / 127.0 * 0.8,
                sample_rate,
            );
            let at = ms_to_samples(take.start_ms, sample_rate) + latency;
            for (i, s) in sound.iter().enumerate() {
                recording[at + i] += s;
            }
        }
        recording
    }

    fn small_plan() -> CapturePlan {
        let mut plan = CapturePlan::every_nth_semitone(48, 54, 3, &[40, 120]);
        plan.hold_ms = 300;
        plan.release_ms = 200;
        plan.gap_ms = 100;
        plan.pre_roll_ms = 200;
        plan
    }

    #[test]
    fn every_take_is_cut_at_its_attack() {
        let sr = 48_000;
        let plan = small_plan();
        let latency = 600; // 12.5 ms
        let recording = fake_take(&plan, sr, latency, None);
        let notes = slice_recording(&recording, sr, &plan, latency);
        assert_eq!(notes.len(), plan.take_count());
        for (note, take) in notes.iter().zip(plan.takes()) {
            assert_eq!((note.note, note.velocity), (take.note, take.velocity));
            // The 20 ms of silence before the attack was trimmed away.
            let onset = detect_onset(&note.samples, peak_db(&note.samples) - 40.0).unwrap();
            assert!(onset < 240, "onset at {onset}");
            // Long enough to hold the sound, short enough not to reach the next take.
            let len_ms = note.samples.len() as u32 * 1000 / sr;
            assert!((400..=600).contains(&len_ms), "{len_ms} ms");
            assert_eq!(note.samples[0], 0.0, "faded in");
        }
        // Velocity dynamics survive slicing.
        assert!(peak_db(&notes[1].samples) > peak_db(&notes[0].samples) + 6.0);
    }

    #[test]
    fn silent_takes_are_skipped_and_a_wrong_latency_is_tolerated() {
        let sr = 48_000;
        let plan = small_plan();
        let recording = fake_take(&plan, sr, 600, Some(2));
        // Latency guessed 10 ms too high: the window lead still catches the attack.
        let notes = slice_recording(&recording, sr, &plan, 600 + 480);
        assert_eq!(notes.len(), plan.take_count() - 1);
        assert!(!notes.iter().any(|n| n.note == 51 && n.velocity == 40));
    }

    #[test]
    fn short_recording_gives_what_it_has() {
        let sr = 48_000;
        let plan = small_plan();
        let recording = fake_take(&plan, sr, 0, None);
        let half = &recording[..recording.len() / 2];
        let notes = slice_recording(half, sr, &plan, 0);
        assert!(!notes.is_empty() && notes.len() < plan.take_count());
        assert!(slice_recording(&[], sr, &plan, 0).is_empty());
    }

    #[test]
    fn group_normalization_keeps_the_balance() {
        let mut notes = vec![
            CapturedNote {
                note: 60,
                velocity: 40,
                sample_rate: 48_000,
                samples: vec![0.1, -0.1],
            },
            CapturedNote {
                note: 60,
                velocity: 120,
                sample_rate: 48_000,
                samples: vec![0.4, -0.2],
            },
        ];
        let gain = normalize_together(&mut notes, -6.0206);
        assert!((gain - 1.25).abs() < 1e-3);
        assert!((notes[1].samples[0] - 0.5).abs() < 1e-4);
        assert!((notes[0].samples[0] - 0.125).abs() < 1e-4);
        normalize_each(&mut notes, 0.0);
        assert!((notes[0].samples[0] - 1.0).abs() < 1e-4);
    }
}
