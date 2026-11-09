//! Counting notes in scale steps instead of semitones.
//!
//! Moving "two steps up the scale" always lands on a note of the scale, which is how every
//! helper here keeps tunes in key.

use aa_core::{PitchClass, ScaleKind};

/// The position of a note counted in scale steps, where 0 is the key note in MIDI octave -1.
///
/// A note outside the scale is first moved to the nearest scale note ([`ScaleKind::snap`]).
/// Example: in C major, C4 (60) is step 35 and D4 (62) is step 36.
pub fn scale_index(key: PitchClass, scale: ScaleKind, midi: u8) -> i32 {
    let intervals = scale.intervals();
    let len = intervals.len() as i32;
    let snapped = scale.snap(key, midi);
    let relative = i32::from(snapped) - i32::from(key.0 % 12);
    let octave = relative.div_euclid(12);
    let within = relative.rem_euclid(12) as u8;
    let position = intervals.iter().position(|&s| s == within).unwrap_or(0) as i32;
    octave * len + position
}

/// The MIDI note at a scale-step position (the reverse of [`scale_index`]).
///
/// Positions beyond the MIDI range are folded back by whole octaves, so the result is always
/// a valid note of the scale.
pub fn from_scale_index(key: PitchClass, scale: ScaleKind, index: i32) -> u8 {
    let intervals = scale.intervals();
    let len = intervals.len() as i32;
    let octave = index.div_euclid(len);
    let position = index.rem_euclid(len) as usize;
    let interval = intervals.get(position).copied().unwrap_or(0);
    let mut midi = i32::from(key.0 % 12) + octave * 12 + i32::from(interval);
    while midi > 127 {
        midi -= 12;
    }
    while midi < 0 {
        midi += 12;
    }
    midi as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_and_note_round_trip_for_every_scale_note() {
        for scale in ScaleKind::ALL {
            for key in 0..12 {
                let key = PitchClass(key);
                for midi in 0..=127u8 {
                    if scale.contains(key, midi) {
                        assert_eq!(
                            from_scale_index(key, scale, scale_index(key, scale, midi)),
                            midi
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn c_major_positions() {
        let c = PitchClass(0);
        assert_eq!(scale_index(c, ScaleKind::Major, 60), 35);
        assert_eq!(scale_index(c, ScaleKind::Major, 62), 36);
        assert_eq!(from_scale_index(c, ScaleKind::Major, 37), 64);
    }
}
