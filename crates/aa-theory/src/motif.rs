//! Variations of a motif: a short tune, like a character's theme (a leitmotif).
//!
//! Composers keep a theme recognisable while changing it: mirror it, play it backwards,
//! shift it, stretch it. Every helper here works on a [`Clip`] and returns a new one; the
//! ones that change pitches move by scale steps, so the result stays in your key.

use aa_core::{Clip, Note, PitchClass, ScaleKind};

use crate::rng::Rng;
use crate::scale::{from_scale_index, scale_index};

/// A clip from `(step, pitch, length_steps)` triples, the quickest way to write a motif.
pub fn motif_clip(name: &str, length_steps: u16, notes: &[(u16, u8, f32)]) -> Clip {
    let mut clip = Clip::new(name, length_steps);
    for &(step, pitch, len) in notes {
        clip = clip.with_note(step, pitch, 0.8, len);
    }
    clip
}

fn map_pitches(clip: &Clip, f: impl Fn(&Note) -> u8) -> Clip {
    let mut out = clip.clone();
    for (note, original) in out.notes.iter_mut().zip(&clip.notes) {
        note.pitch = f(original);
    }
    out
}

fn sort_by_step(clip: &mut Clip) {
    clip.notes.sort_by_key(|n| n.step);
}

/// Moves every note to the nearest note of the scale.
pub fn snap_to_scale(clip: &Clip, key: PitchClass, scale: ScaleKind) -> Clip {
    map_pitches(clip, |n| scale.snap(key, n.pitch))
}

/// Moves the whole tune up (positive) or down (negative) by `degrees` scale steps.
///
/// Unlike moving by semitones, every note stays in the scale: in C major, C-E-G moved up one
/// step becomes D-F-A.
pub fn transpose_in_scale(clip: &Clip, key: PitchClass, scale: ScaleKind, degrees: i32) -> Clip {
    map_pitches(clip, |n| {
        from_scale_index(key, scale, scale_index(key, scale, n.pitch) + degrees)
    })
}

/// Mirrors the tune upside down around its first note: where it went up, it now goes down.
///
/// The mirror counts scale steps, so the result stays in the scale. Inverting twice gives
/// back the original (for notes already in the scale).
pub fn invert(clip: &Clip, key: PitchClass, scale: ScaleKind) -> Clip {
    let Some(first) = clip.notes.iter().min_by_key(|n| n.step) else {
        return clip.clone();
    };
    let axis = scale_index(key, scale, first.pitch);
    map_pitches(clip, |n| {
        from_scale_index(key, scale, 2 * axis - scale_index(key, scale, n.pitch))
    })
}

/// Plays the tune backwards: the last note comes first. Pitches are untouched.
///
/// Note lengths are kept, so a note that ended at the end of the clip now starts at its
/// beginning. Doing it twice gives back the original.
pub fn retrograde(clip: &Clip) -> Clip {
    let length = i32::from(clip.length_steps);
    let mut out = clip.clone();
    for note in &mut out.notes {
        let duration = (note.length_steps.round() as i32).clamp(1, length.max(1));
        let end = (i32::from(note.step) + duration).min(length);
        note.step = (length - end).max(0) as u16;
    }
    out.notes.reverse();
    sort_by_step(&mut out);
    out
}

/// Stretches the tune to twice as slow: twice the length, every note twice as long.
pub fn augment(clip: &Clip) -> Clip {
    let mut out = clip.clone();
    out.length_steps = clip.length_steps.saturating_mul(2);
    for note in &mut out.notes {
        note.step = note.step.saturating_mul(2);
        note.length_steps *= 2.0;
    }
    out
}

/// Squeezes the tune to twice as fast: half the length, every note half as long.
///
/// Notes that land on the same step with the same pitch are merged into one.
pub fn diminish(clip: &Clip) -> Clip {
    let mut out = clip.clone();
    out.length_steps = (clip.length_steps / 2).max(1);
    out.notes.clear();
    for note in &clip.notes {
        let step = note.step / 2;
        if out
            .notes
            .iter()
            .any(|n| n.step == step && n.pitch == note.pitch)
        {
            continue;
        }
        let mut n = note.clone();
        n.step = step;
        n.length_steps = (note.length_steps / 2.0).max(0.25);
        out.notes.push(n);
    }
    out
}

/// Shifts every note later (positive) or earlier (negative) by `steps`, wrapping around the
/// clip. The same tune now leans on different beats, which changes its feel a lot.
pub fn rhythmic_displace(clip: &Clip, steps: i32) -> Clip {
    let length = i32::from(clip.length_steps.max(1));
    let mut out = clip.clone();
    for note in &mut out.notes {
        note.step = (i32::from(note.step) + steps).rem_euclid(length) as u16;
    }
    sort_by_step(&mut out);
    out
}

/// A random variation: one or two of transpose, invert, retrograde and displace.
///
/// The result is always in the scale and keeps the clip length. Its name says what was done,
/// e.g. "Theme (mirrored, 2 steps up)". The same seed gives the same variation.
pub fn vary(clip: &Clip, key: PitchClass, scale: ScaleKind, rng: &mut Rng) -> Clip {
    let mut out = snap_to_scale(clip, key, scale);
    let mut ops = [0usize, 1, 2, 3];
    rng.shuffle(&mut ops);
    let count = 1 + rng.below(2);
    let mut labels = Vec::new();
    for &op in &ops[..count] {
        match op {
            0 => {
                let amount = rng.range_i32(1, 3) * if rng.chance(0.5) { 1 } else { -1 };
                out = transpose_in_scale(&out, key, scale, amount);
                let dir = if amount > 0 { "up" } else { "down" };
                labels.push(format!(
                    "{} step{} {dir}",
                    amount.abs(),
                    if amount.abs() == 1 { "" } else { "s" }
                ));
            }
            1 => {
                out = invert(&out, key, scale);
                labels.push("mirrored".to_string());
            }
            2 => {
                out = retrograde(&out);
                labels.push("backwards".to_string());
            }
            _ => {
                let amount = [1, 2, 4][rng.below(3)] * if rng.chance(0.5) { 1 } else { -1 };
                out = rhythmic_displace(&out, amount);
                let dir = if amount > 0 { "later" } else { "earlier" };
                labels.push(format!(
                    "shifted {} step{} {dir}",
                    amount.abs(),
                    if amount.abs() == 1 { "" } else { "s" }
                ));
            }
        }
    }
    out.name = format!("{} ({})", clip.name, labels.join(", "));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: PitchClass = PitchClass(2);
    const MINOR: ScaleKind = ScaleKind::NaturalMinor;

    /// D F A G E D: in D minor.
    fn theme() -> Clip {
        motif_clip(
            "Theme",
            16,
            &[
                (0, 62, 2.0),
                (2, 65, 2.0),
                (4, 69, 1.0),
                (6, 67, 2.0),
                (8, 64, 2.0),
                (12, 62, 4.0),
            ],
        )
    }

    fn pitches(c: &Clip) -> Vec<u8> {
        c.notes.iter().map(|n| n.pitch).collect()
    }

    #[test]
    fn transposing_stays_in_the_scale() {
        let up = transpose_in_scale(&theme(), D, MINOR, 1);
        assert_eq!(pitches(&up), [64, 67, 70, 69, 65, 64]); // E G Bb A F E
        let back = transpose_in_scale(&up, D, MINOR, -1);
        assert_eq!(back, theme());
    }

    #[test]
    fn inversion_mirrors_and_round_trips() {
        let inv = invert(&theme(), D, MINOR);
        // D F A G E D, steps 0 +2 +4 +3 +1 0 → 0 -2 -4 -3 -1 0: D Bb G A C D
        assert_eq!(pitches(&inv), [62, 58, 55, 57, 60, 62]);
        assert_eq!(invert(&inv, D, MINOR), theme());
    }

    #[test]
    fn retrograde_reverses_and_round_trips() {
        let r = retrograde(&theme());
        assert_eq!(pitches(&r), [62, 64, 67, 69, 65, 62]);
        assert_eq!(r.notes[0].step, 0);
        assert_eq!(retrograde(&r), theme());
    }

    #[test]
    fn stretching_and_shifting() {
        let a = augment(&theme());
        assert_eq!(a.length_steps, 32);
        assert_eq!(a.notes[1].step, 4);
        assert_eq!(diminish(&a), theme());
        let shifted = rhythmic_displace(&theme(), 2);
        assert_eq!(shifted.notes[0].step, 2);
        assert_eq!(shifted.notes.last().map(|n| n.step), Some(14));
        assert_eq!(
            rhythmic_displace(&theme(), -2)
                .notes
                .last()
                .map(|n| (n.step, n.pitch)),
            Some((14, 62))
        );
    }

    #[test]
    fn variations_stay_in_scale_and_repeat_with_the_same_seed() {
        let mut out_of_key = theme();
        out_of_key.notes[2].pitch = 68; // G#: not in D minor
        for seed in 0..100 {
            let v = vary(&out_of_key, D, MINOR, &mut Rng::new(seed));
            assert_eq!(v.length_steps, 16);
            assert_eq!(v.notes.len(), 6);
            assert!(
                v.notes.iter().all(|n| MINOR.contains(D, n.pitch)),
                "{}",
                v.name
            );
            assert!(v.name.starts_with("Theme ("));
            assert_eq!(v, vary(&out_of_key, D, MINOR, &mut Rng::new(seed)));
        }
    }
}
