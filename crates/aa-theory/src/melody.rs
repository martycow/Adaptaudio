//! A melody generator that sounds like someone could sing it.
//!
//! Real tunes mostly move to the next note of the scale (stepwise motion), leap only now and
//! then, follow an overall shape (rising, falling, an arch…) and end each phrase on the key
//! note so they feel finished. The generator follows those same habits.

use aa_core::{Clip, PitchClass, STEPS_PER_BEAT, ScaleKind};
use serde::{Deserialize, Serialize};

use crate::rng::Rng;
use crate::scale::from_scale_index;

/// The overall shape a melody follows within each phrase.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Contour {
    Rising,
    Falling,
    Arch,
    Valley,
    Wave,
    Flat,
}

impl Contour {
    pub const ALL: [Contour; 6] = [
        Contour::Rising,
        Contour::Falling,
        Contour::Arch,
        Contour::Valley,
        Contour::Wave,
        Contour::Flat,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Contour::Rising => "Rising",
            Contour::Falling => "Falling",
            Contour::Arch => "Arch",
            Contour::Valley => "Valley",
            Contour::Wave => "Wave",
            Contour::Flat => "Flat",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Contour::Rising => "climbs up: builds tension, asks a question",
            Contour::Falling => "sinks down: calms, sighs, gives up",
            Contour::Arch => "goes up then comes back: the classic song shape",
            Contour::Valley => "dips down then comes back up: brooding",
            Contour::Wave => "rises and falls twice: restless, curious",
            Contour::Flat => "stays around one note: chant-like, hypnotic",
        }
    }

    /// Where the tune aims at position `t` (0..1) of a phrase, in 7-note-scale steps above home.
    fn target(self, t: f32) -> f32 {
        use std::f32::consts::PI;
        match self {
            Contour::Rising => -1.0 + 6.0 * t,
            Contour::Falling => 5.0 - 6.0 * t,
            Contour::Arch => 1.0 + 4.0 * (PI * t).sin(),
            Contour::Valley => 4.0 - 4.0 * (PI * t).sin(),
            Contour::Wave => 2.0 + 2.5 * (2.0 * PI * t).sin(),
            Contour::Flat => 2.0,
        }
    }
}

const STEPS_PER_BAR: u16 = STEPS_PER_BEAT * 4;

/// How strongly a step of the bar invites a note: downbeats most, sixteenth off-beats least.
fn metric_weight(step_in_bar: u16) -> f32 {
    if step_in_bar.is_multiple_of(8) {
        0.9
    } else if step_in_bar.is_multiple_of(4) {
        0.7
    } else if step_in_bar.is_multiple_of(2) {
        0.35
    } else {
        0.1
    }
}

/// One move of the tune, in scale steps: mostly to the next note toward the target.
fn next_move(diff: i32, rng: &mut Rng) -> i32 {
    let toward = diff.signum();
    let either = if rng.chance(0.5) { 1 } else { -1 };
    let roll = rng.next_f32();
    if diff.abs() >= 3 {
        2 * toward
    } else if roll < 0.55 {
        if toward == 0 { either } else { toward }
    } else if roll < 0.75 {
        0
    } else if roll < 0.93 {
        if toward == 0 { either } else { -toward }
    } else {
        2 * if toward == 0 { either } else { toward }
    }
}

/// Generates a melody of `bars` bars (4/4, 16 steps each) in the key and scale.
///
/// - `density` 0..1: 0 is a few long notes, 1 is busy sixteenths.
/// - `contour`: the shape each phrase follows.
///
/// Phrases are two bars long. Each starts on its first beat, moves mostly by single scale
/// steps, and lands on the key note on beat 3 of its last bar, approached by a step. Every
/// note is in the scale. The tune sits around octave 4-5 (lead range). Same seed, same tune.
pub fn generate_melody(
    key: PitchClass,
    scale: ScaleKind,
    bars: u16,
    density: f32,
    contour: Contour,
    rng: &mut Rng,
) -> Clip {
    let bars = bars.clamp(1, 512);
    let density = density.clamp(0.0, 1.0);
    let len = scale.intervals().len() as i32;
    let home = 5 * len; // the key note in octave 4
    let low = -3;
    let high = len + 4;
    let scale_steps = len as f32 / 7.0;
    let phrase_bars: u16 = if bars >= 2 { 2 } else { 1 };
    let mut clip = Clip::new(format!("{} melody", contour.name()), bars * STEPS_PER_BAR);

    let mut bar = 0;
    while bar < bars {
        let start = bar * STEPS_PER_BAR;
        let end = (bar + phrase_bars.min(bars - bar)) * STEPS_PER_BAR;
        let landing = end - STEPS_PER_BAR / 2;

        let mut onsets = vec![start];
        for step in start + 1..landing {
            let chance = (metric_weight(step % STEPS_PER_BAR) * (0.15 + 1.1 * density)).min(0.95);
            if rng.chance(chance) {
                onsets.push(step);
            }
        }

        let span = f32::from(landing - start);
        let mut current = (contour.target(0.0) * scale_steps).round() as i32;
        let mut degrees = Vec::with_capacity(onsets.len());
        for (i, &step) in onsets.iter().enumerate() {
            if i > 0 {
                let t = f32::from(step - start) / span;
                let target = (contour.target(t) * scale_steps).round() as i32;
                current = (current + next_move(target - current, rng)).clamp(low, high);
            }
            degrees.push(current);
        }
        let final_degree = (current as f32 / len as f32).round() as i32 * len;
        if degrees.len() > 1 {
            // Approach home from the note just above or below it.
            let side = if current < final_degree { -1 } else { 1 };
            if let Some(last) = degrees.last_mut() {
                *last = final_degree + side;
            }
        }

        for (i, (&step, &degree)) in onsets.iter().zip(&degrees).enumerate() {
            let next = onsets.get(i + 1).copied().unwrap_or(landing);
            let length = f32::from((next - step).clamp(1, 6));
            let accent = if step % STEPS_PER_BAR == 0 {
                0.9
            } else if step % STEPS_PER_BEAT == 0 {
                0.8
            } else {
                0.68
            };
            let velocity = (accent + rng.next_f32() * 0.08).min(1.0);
            clip = clip.with_note(
                step,
                from_scale_index(key, scale, home + degree),
                velocity,
                length,
            );
        }
        clip = clip.with_note(
            landing,
            from_scale_index(key, scale, home + final_degree),
            0.85,
            f32::from(end - landing),
        );

        bar += phrase_bars;
    }
    clip
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn melodies_are_in_scale_and_end_on_the_key_note() {
        for scale in ScaleKind::ALL {
            for contour in Contour::ALL {
                for seed in 0..10 {
                    let key = PitchClass((seed % 12) as u8);
                    let density = seed as f32 / 9.0;
                    let m = generate_melody(key, scale, 4, density, contour, &mut Rng::new(seed));
                    assert_eq!(m.length_steps, 64);
                    assert!(m.notes.iter().all(|n| scale.contains(key, n.pitch)));
                    let last = m.notes.last().map(|n| n.pitch % 12);
                    assert_eq!(last, Some(key.0));
                    // Phrase 1 lands on beat 3 of bar 2 as well.
                    assert!(
                        m.notes
                            .iter()
                            .any(|n| n.step == 24 && n.pitch % 12 == key.0)
                    );
                    assert!(m.notes.iter().all(|n| n.step < 64));
                }
            }
        }
    }

    #[test]
    fn melodies_move_mostly_by_step() {
        let key = PitchClass(2);
        let scale = ScaleKind::NaturalMinor;
        let m = generate_melody(key, scale, 8, 0.6, Contour::Arch, &mut Rng::new(11));
        let idx: Vec<i32> = m
            .notes
            .iter()
            .map(|n| crate::scale::scale_index(key, scale, n.pitch))
            .collect();
        let small = idx.windows(2).filter(|w| (w[1] - w[0]).abs() <= 2).count();
        assert!(
            small * 10 >= (idx.len() - 1) * 8,
            "{small} of {}",
            idx.len() - 1
        );
    }

    #[test]
    fn density_changes_how_busy_the_tune_is() {
        let key = PitchClass(0);
        let sparse = generate_melody(
            key,
            ScaleKind::Major,
            8,
            0.0,
            Contour::Wave,
            &mut Rng::new(5),
        );
        let busy = generate_melody(
            key,
            ScaleKind::Major,
            8,
            1.0,
            Contour::Wave,
            &mut Rng::new(5),
        );
        assert!(busy.notes.len() > sparse.notes.len() * 2);
    }

    #[test]
    fn same_seed_same_melody() {
        let a = generate_melody(
            PitchClass(9),
            ScaleKind::Dorian,
            4,
            0.5,
            Contour::Valley,
            &mut Rng::new(99),
        );
        let b = generate_melody(
            PitchClass(9),
            ScaleKind::Dorian,
            4,
            0.5,
            Contour::Valley,
            &mut Rng::new(99),
        );
        let c = generate_melody(
            PitchClass(9),
            ScaleKind::Dorian,
            4,
            0.5,
            Contour::Valley,
            &mut Rng::new(100),
        );
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
