//! Parts that play along with a chord progression: arpeggios, bass lines, block chords.

use aa_core::{Clip, STEPS_PER_BEAT};
use serde::{Deserialize, Serialize};

use crate::chord::ChordSpec;
use crate::rng::Rng;

/// The order an arpeggio plays the notes of a chord in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArpPattern {
    /// Lowest to highest, then again.
    Up,
    /// Highest to lowest, then again.
    Down,
    /// Up then back down, without repeating the top and bottom notes.
    UpDown,
    /// Any note, never the same one twice in a row.
    Random,
}

impl ArpPattern {
    pub const ALL: [ArpPattern; 4] = [
        ArpPattern::Up,
        ArpPattern::Down,
        ArpPattern::UpDown,
        ArpPattern::Random,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ArpPattern::Up => "Up",
            ArpPattern::Down => "Down",
            ArpPattern::UpDown => "Up-down",
            ArpPattern::Random => "Random",
        }
    }
}

/// Plays a chord one note at a time (an arpeggio), one note every `rate_steps` steps.
///
/// `steps` is the clip length; `rate_steps` 1 = sixteenths, 2 = eighths, 4 = quarter notes.
/// `Random` is repeatable: the same chord always gives the same order. Use
/// [`arpeggiate_with`] to choose the randomness yourself.
pub fn arpeggiate(chord_notes: &[u8], pattern: ArpPattern, steps: u16, rate_steps: u16) -> Clip {
    let seed = chord_notes.iter().fold(u64::from(steps), |acc, &n| {
        acc.wrapping_mul(131).wrapping_add(u64::from(n))
    });
    arpeggiate_with(chord_notes, pattern, steps, rate_steps, &mut Rng::new(seed))
}

/// [`arpeggiate`] with your own random number generator for the `Random` pattern.
pub fn arpeggiate_with(
    chord_notes: &[u8],
    pattern: ArpPattern,
    steps: u16,
    rate_steps: u16,
    rng: &mut Rng,
) -> Clip {
    let mut clip = Clip::new(format!("Arp {}", pattern.name().to_lowercase()), steps);
    let mut up: Vec<u8> = chord_notes.to_vec();
    up.sort_unstable();
    up.dedup();
    if up.is_empty() {
        return clip;
    }
    let cycle: Vec<u8> = match pattern {
        ArpPattern::Up | ArpPattern::Random => up.clone(),
        ArpPattern::Down => up.iter().rev().copied().collect(),
        ArpPattern::UpDown => {
            let mut c = up.clone();
            if up.len() > 2 {
                c.extend(up[1..up.len() - 1].iter().rev());
            }
            c
        }
    };
    let rate = rate_steps.max(1);
    let length = (f32::from(rate) * 0.75).max(0.5);
    let mut previous: Option<usize> = None;
    for (i, step) in (0..clip.length_steps)
        .step_by(usize::from(rate))
        .enumerate()
    {
        let index = match pattern {
            ArpPattern::Random => {
                let mut pick = rng.below(cycle.len());
                if cycle.len() > 1 && Some(pick) == previous {
                    pick = (pick + 1 + rng.below(cycle.len() - 1)) % cycle.len();
                }
                pick
            }
            _ => i % cycle.len(),
        };
        previous = Some(index);
        let velocity = if step % STEPS_PER_BEAT == 0 {
            0.85
        } else {
            0.65
        };
        clip = clip.with_note(step, cycle[index], velocity, length);
    }
    clip
}

/// How a bass line moves under the chords.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BassStyle {
    /// One long root note per chord: calm, spacious.
    Roots,
    /// Root and the same note an octave up, bouncing in eighths: disco, energy.
    Octaves,
    /// The root on every eighth note: driving, urgent (chases).
    DrivingEighths,
    /// A note on every beat through the chord's notes, stepping into the next chord: jazzy.
    Walking,
}

impl BassStyle {
    pub const ALL: [BassStyle; 4] = [
        BassStyle::Roots,
        BassStyle::Octaves,
        BassStyle::DrivingEighths,
        BassStyle::Walking,
    ];

    pub fn name(self) -> &'static str {
        match self {
            BassStyle::Roots => "Roots",
            BassStyle::Octaves => "Octaves",
            BassStyle::DrivingEighths => "Driving eighths",
            BassStyle::Walking => "Walking",
        }
    }
}

/// The lowest note of a chord moved into bass range (MIDI 36-47, octave 2).
fn bass_root(chord: &ChordSpec) -> u8 {
    36 + chord.root.0 % 12
}

/// A chord's notes as gaps above its root, placed on `root`.
fn bass_tones(chord: &ChordSpec, root: u8) -> Vec<u8> {
    let lowest = chord.notes.first().copied().unwrap_or(0);
    chord
        .notes
        .iter()
        .map(|&n| root.saturating_add(n.saturating_sub(lowest)))
        .collect()
}

/// A bass line for a progression, `steps_per_chord` steps per chord (16 = one bar).
pub fn bass_line(progression: &[ChordSpec], style: BassStyle, steps_per_chord: u16) -> Clip {
    let spc = steps_per_chord.max(1);
    let total = (progression.len() as u16).saturating_mul(spc);
    let mut clip = Clip::new(format!("Bass {}", style.name().to_lowercase()), total);
    for (i, chord) in progression.iter().enumerate() {
        let start = i as u16 * spc;
        let root = bass_root(chord);
        match style {
            BassStyle::Roots => {
                clip = clip.with_note(start, root, 0.9, f32::from(spc) * 0.95);
            }
            BassStyle::Octaves | BassStyle::DrivingEighths => {
                for (k, offset) in (0..spc).step_by(2).enumerate() {
                    let up = style == BassStyle::Octaves && k % 2 == 1;
                    let pitch = if up { root + 12 } else { root };
                    let velocity = if offset % STEPS_PER_BEAT == 0 {
                        0.9
                    } else {
                        0.68
                    };
                    clip = clip.with_note(
                        start + offset,
                        pitch,
                        velocity,
                        1.6_f32.min(f32::from(spc - offset)),
                    );
                }
            }
            BassStyle::Walking => {
                let tones = bass_tones(chord, root);
                let third = tones.get(1).copied().unwrap_or(root);
                let fifth = tones.get(2).copied().unwrap_or(root);
                let next = &progression[(i + 1) % progression.len()];
                // Step into the next chord from its fifth, just below its root (a V-I pull).
                let next_root = bass_root(next);
                let next_fifth = bass_tones(next, next_root)
                    .get(2)
                    .copied()
                    .unwrap_or(next_root + 7);
                let approach = next_fifth.saturating_sub(12).max(28);
                let beats = (spc / STEPS_PER_BEAT).max(1);
                let body = [root, third, fifth, third];
                for b in 0..beats {
                    let pitch = if b + 1 == beats && beats > 1 {
                        approach
                    } else {
                        body[usize::from(b) % body.len()]
                    };
                    let velocity = if b == 0 { 0.9 } else { 0.75 };
                    clip = clip.with_note(
                        start + b * STEPS_PER_BEAT,
                        pitch,
                        velocity,
                        f32::from(STEPS_PER_BEAT) * 0.9,
                    );
                }
            }
        }
    }
    clip
}

/// The progression as block chords (all notes together), `steps_per_chord` steps each.
pub fn chord_clip(progression: &[ChordSpec], steps_per_chord: u16) -> Clip {
    let spc = steps_per_chord.max(1);
    let total = (progression.len() as u16).saturating_mul(spc);
    let mut clip = Clip::new("Chords", total);
    for (i, chord) in progression.iter().enumerate() {
        for &note in &chord.notes {
            clip = clip.with_note(i as u16 * spc, note, 0.7, f32::from(spc) * 0.95);
        }
    }
    clip
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chord::diatonic_chord;
    use aa_core::{PitchClass, ScaleKind};

    fn pitches(c: &Clip) -> Vec<u8> {
        c.notes.iter().map(|n| n.pitch).collect()
    }

    #[test]
    fn arpeggio_orders() {
        let chord = [67, 60, 64];
        assert_eq!(
            pitches(&arpeggiate(&chord, ArpPattern::Up, 8, 2)),
            [60, 64, 67, 60]
        );
        assert_eq!(
            pitches(&arpeggiate(&chord, ArpPattern::Down, 8, 2)),
            [67, 64, 60, 67]
        );
        assert_eq!(
            pitches(&arpeggiate(&chord, ArpPattern::UpDown, 16, 2)),
            [60, 64, 67, 64, 60, 64, 67, 64]
        );
        let random = arpeggiate(&chord, ArpPattern::Random, 64, 1);
        assert_eq!(random.notes.len(), 64);
        assert!(random.notes.windows(2).all(|w| w[0].pitch != w[1].pitch));
        assert_eq!(random, arpeggiate(&chord, ArpPattern::Random, 64, 1));
        assert!(arpeggiate(&[], ArpPattern::Up, 16, 1).notes.is_empty());
    }

    fn d_minor(degrees: &[usize]) -> Vec<ChordSpec> {
        degrees
            .iter()
            .map(|&d| diatonic_chord(PitchClass(2), ScaleKind::NaturalMinor, d, 50, false))
            .collect()
    }

    #[test]
    fn bass_styles() {
        let prog = d_minor(&[0, 5, 6, 0]); // Dm Bb C Dm
        let roots = bass_line(&prog, BassStyle::Roots, 16);
        assert_eq!(roots.length_steps, 64);
        assert_eq!(pitches(&roots), [38, 46, 36, 38]);
        let octaves = bass_line(&prog, BassStyle::Octaves, 16);
        assert_eq!(&pitches(&octaves)[..4], [38, 50, 38, 50]);
        let driving = bass_line(&prog, BassStyle::DrivingEighths, 16);
        assert_eq!(driving.notes.len(), 32);
        let walking = bass_line(&prog, BassStyle::Walking, 16);
        // Dm: D F A, then F (fifth of Bb, below its root) leads into Bb.
        assert_eq!(&pitches(&walking)[..5], [38, 41, 45, 41, 46]);
        assert!(
            walking
                .notes
                .iter()
                .all(|n| ScaleKind::NaturalMinor.contains(PitchClass(2), n.pitch))
        );
    }

    #[test]
    fn chord_clip_plays_every_note() {
        let prog = d_minor(&[0, 3]);
        let clip = chord_clip(&prog, 8);
        assert_eq!(clip.length_steps, 16);
        assert_eq!(clip.notes.len(), 6);
        assert_eq!(clip.notes_at(8).count(), 3);
    }
}
