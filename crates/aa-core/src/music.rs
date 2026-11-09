//! The small amount of music theory the data model itself needs: pitch classes and scales.

use serde::{Deserialize, Serialize};

/// A note name without octave: 0 = C, 1 = C♯ … 11 = B.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PitchClass(pub u8);

impl PitchClass {
    pub const NAMES: [&'static str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];

    pub fn new(semitone: i32) -> Self {
        PitchClass(semitone.rem_euclid(12) as u8)
    }

    pub fn name(self) -> &'static str {
        Self::NAMES[(self.0 % 12) as usize]
    }
}

/// Name of a MIDI note, e.g. 60 → "C4".
pub fn note_name(midi: u8) -> String {
    let octave = i32::from(midi) / 12 - 1;
    format!("{}{}", PitchClass::new(i32::from(midi)).name(), octave)
}

/// Frequency in Hz of a (possibly fractional) MIDI note, A4 = 440 Hz.
pub fn midi_to_hz(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

/// A musical scale ("лад"). Never called "mode" in code: `GameMode` means the game's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScaleKind {
    Major,
    NaturalMinor,
    HarmonicMinor,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Locrian,
    MajorPentatonic,
    MinorPentatonic,
    Blues,
    Chromatic,
}

impl ScaleKind {
    pub const ALL: [ScaleKind; 12] = [
        ScaleKind::Major,
        ScaleKind::NaturalMinor,
        ScaleKind::HarmonicMinor,
        ScaleKind::Dorian,
        ScaleKind::Phrygian,
        ScaleKind::Lydian,
        ScaleKind::Mixolydian,
        ScaleKind::Locrian,
        ScaleKind::MajorPentatonic,
        ScaleKind::MinorPentatonic,
        ScaleKind::Blues,
        ScaleKind::Chromatic,
    ];

    /// Semitone offsets from the root.
    pub fn intervals(self) -> &'static [u8] {
        match self {
            ScaleKind::Major => &[0, 2, 4, 5, 7, 9, 11],
            ScaleKind::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
            ScaleKind::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            ScaleKind::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            ScaleKind::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            ScaleKind::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            ScaleKind::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            ScaleKind::Locrian => &[0, 1, 3, 5, 6, 8, 10],
            ScaleKind::MajorPentatonic => &[0, 2, 4, 7, 9],
            ScaleKind::MinorPentatonic => &[0, 3, 5, 7, 10],
            ScaleKind::Blues => &[0, 3, 5, 6, 7, 10],
            ScaleKind::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ScaleKind::Major => "Major",
            ScaleKind::NaturalMinor => "Minor",
            ScaleKind::HarmonicMinor => "Harmonic minor",
            ScaleKind::Dorian => "Dorian",
            ScaleKind::Phrygian => "Phrygian",
            ScaleKind::Lydian => "Lydian",
            ScaleKind::Mixolydian => "Mixolydian",
            ScaleKind::Locrian => "Locrian",
            ScaleKind::MajorPentatonic => "Major pentatonic",
            ScaleKind::MinorPentatonic => "Minor pentatonic",
            ScaleKind::Blues => "Blues",
            ScaleKind::Chromatic => "Chromatic",
        }
    }

    /// One-line feeling, shown next to the scale picker.
    pub fn mood(self) -> &'static str {
        match self {
            ScaleKind::Major => "bright, confident",
            ScaleKind::NaturalMinor => "sad, serious",
            ScaleKind::HarmonicMinor => "dramatic, exotic",
            ScaleKind::Dorian => "minor with hope: mystery, sneaking",
            ScaleKind::Phrygian => "minor with menace: danger, chase",
            ScaleKind::Lydian => "major with magic: wonder, relief",
            ScaleKind::Mixolydian => "major with grit: adventure",
            ScaleKind::Locrian => "unstable, unsettling",
            ScaleKind::MajorPentatonic => "folk, cozy, hard to play wrong",
            ScaleKind::MinorPentatonic => "earthy, calm minor",
            ScaleKind::Blues => "gritty, bent",
            ScaleKind::Chromatic => "every note: no scale lock",
        }
    }

    /// True when the pitch belongs to this scale rooted at `key`.
    pub fn contains(self, key: PitchClass, midi: u8) -> bool {
        let degree = (i32::from(midi) - i32::from(key.0)).rem_euclid(12) as u8;
        self.intervals().contains(&degree)
    }

    /// The nearest pitch in the scale (ties go down), so scale lock never moves a note far.
    pub fn snap(self, key: PitchClass, midi: u8) -> u8 {
        if self.contains(key, midi) {
            return midi;
        }
        for distance in 1..=6i32 {
            for candidate in [i32::from(midi) - distance, i32::from(midi) + distance] {
                if (0..=127).contains(&candidate) && self.contains(key, candidate as u8) {
                    return candidate as u8;
                }
            }
        }
        midi
    }

    /// The `degree`-th note of the scale (0 = root) from `root_midi`, wrapping into octaves.
    pub fn degree(self, root_midi: u8, degree: i32) -> u8 {
        let iv = self.intervals();
        let len = iv.len() as i32;
        let octave = degree.div_euclid(len);
        let step = degree.rem_euclid(len) as usize;
        (i32::from(root_midi) + octave * 12 + i32::from(iv[step])).clamp(0, 127) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_names_follow_the_midi_convention() {
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(69), "A4");
        assert_eq!(note_name(38), "D2");
    }

    #[test]
    fn a4_is_440_hz() {
        assert!((midi_to_hz(69.0) - 440.0).abs() < 1e-3);
        assert!((midi_to_hz(81.0) - 880.0).abs() < 1e-2);
    }

    #[test]
    fn snapping_keeps_notes_in_d_phrygian() {
        let d = PitchClass(2);
        // D phrygian: D Eb F G A Bb C
        assert!(ScaleKind::Phrygian.contains(d, 63)); // Eb4
        assert!(!ScaleKind::Phrygian.contains(d, 64)); // E4
        assert_eq!(ScaleKind::Phrygian.snap(d, 64), 63);
        for n in 0..=127u8 {
            assert!(ScaleKind::Phrygian.contains(d, ScaleKind::Phrygian.snap(d, n)));
        }
    }

    #[test]
    fn degrees_wrap_into_octaves() {
        assert_eq!(ScaleKind::Major.degree(60, 0), 60);
        assert_eq!(ScaleKind::Major.degree(60, 7), 72);
        assert_eq!(ScaleKind::Major.degree(60, -1), 59);
        assert_eq!(ScaleKind::MinorPentatonic.degree(57, 5), 69);
    }
}
