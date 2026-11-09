//! Chords: a few notes played together.
//!
//! A chord is named after its lowest note (the *root*) plus its *quality*, the flavour that
//! the gaps between its notes give it: "Dm" is D minor, "Bbmaj7" is B-flat major seventh.

use aa_core::{PitchClass, ScaleKind};
use serde::{Deserialize, Serialize};

/// The flavour of a chord, set by the gaps (in semitones) between its notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChordQuality {
    Major,
    Minor,
    Diminished,
    Augmented,
    Sus2,
    Sus4,
    Major7,
    Minor7,
    Dominant7,
    HalfDiminished7,
    Diminished7,
    MinorMajor7,
    AugmentedMajor7,
    Minor9,
    Add9,
}

impl ChordQuality {
    pub const ALL: [ChordQuality; 15] = [
        ChordQuality::Major,
        ChordQuality::Minor,
        ChordQuality::Diminished,
        ChordQuality::Augmented,
        ChordQuality::Sus2,
        ChordQuality::Sus4,
        ChordQuality::Major7,
        ChordQuality::Minor7,
        ChordQuality::Dominant7,
        ChordQuality::HalfDiminished7,
        ChordQuality::Diminished7,
        ChordQuality::MinorMajor7,
        ChordQuality::AugmentedMajor7,
        ChordQuality::Minor9,
        ChordQuality::Add9,
    ];

    /// Semitones above the root for each note of the chord.
    pub fn intervals(self) -> &'static [u8] {
        match self {
            ChordQuality::Major => &[0, 4, 7],
            ChordQuality::Minor => &[0, 3, 7],
            ChordQuality::Diminished => &[0, 3, 6],
            ChordQuality::Augmented => &[0, 4, 8],
            ChordQuality::Sus2 => &[0, 2, 7],
            ChordQuality::Sus4 => &[0, 5, 7],
            ChordQuality::Major7 => &[0, 4, 7, 11],
            ChordQuality::Minor7 => &[0, 3, 7, 10],
            ChordQuality::Dominant7 => &[0, 4, 7, 10],
            ChordQuality::HalfDiminished7 => &[0, 3, 6, 10],
            ChordQuality::Diminished7 => &[0, 3, 6, 9],
            ChordQuality::MinorMajor7 => &[0, 3, 7, 11],
            ChordQuality::AugmentedMajor7 => &[0, 4, 8, 11],
            ChordQuality::Minor9 => &[0, 3, 7, 10, 14],
            ChordQuality::Add9 => &[0, 4, 7, 14],
        }
    }

    /// What goes after the root in a chord name: "" for major, "m" for minor, "maj7"…
    pub fn suffix(self) -> &'static str {
        match self {
            ChordQuality::Major => "",
            ChordQuality::Minor => "m",
            ChordQuality::Diminished => "dim",
            ChordQuality::Augmented => "aug",
            ChordQuality::Sus2 => "sus2",
            ChordQuality::Sus4 => "sus4",
            ChordQuality::Major7 => "maj7",
            ChordQuality::Minor7 => "m7",
            ChordQuality::Dominant7 => "7",
            ChordQuality::HalfDiminished7 => "m7b5",
            ChordQuality::Diminished7 => "dim7",
            ChordQuality::MinorMajor7 => "m(maj7)",
            ChordQuality::AugmentedMajor7 => "maj7#5",
            ChordQuality::Minor9 => "m9",
            ChordQuality::Add9 => "add9",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ChordQuality::Major => "Major",
            ChordQuality::Minor => "Minor",
            ChordQuality::Diminished => "Diminished",
            ChordQuality::Augmented => "Augmented",
            ChordQuality::Sus2 => "Suspended 2nd",
            ChordQuality::Sus4 => "Suspended 4th",
            ChordQuality::Major7 => "Major 7th",
            ChordQuality::Minor7 => "Minor 7th",
            ChordQuality::Dominant7 => "Dominant 7th",
            ChordQuality::HalfDiminished7 => "Half-diminished 7th",
            ChordQuality::Diminished7 => "Diminished 7th",
            ChordQuality::MinorMajor7 => "Minor-major 7th",
            ChordQuality::AugmentedMajor7 => "Augmented major 7th",
            ChordQuality::Minor9 => "Minor 9th",
            ChordQuality::Add9 => "Add 9",
        }
    }

    /// How the chord tends to feel, in a few words.
    pub fn feeling(self) -> &'static str {
        match self {
            ChordQuality::Major => "bright, settled",
            ChordQuality::Minor => "sad, serious",
            ChordQuality::Diminished => "tense, wants to move on",
            ChordQuality::Augmented => "dreamy, off-balance",
            ChordQuality::Sus2 => "open, floating",
            ChordQuality::Sus4 => "hanging, waiting to land",
            ChordQuality::Major7 => "warm, soft, a bit nostalgic",
            ChordQuality::Minor7 => "mellow, smoky",
            ChordQuality::Dominant7 => "pushy: begs to go home",
            ChordQuality::HalfDiminished7 => "uneasy, searching",
            ChordQuality::Diminished7 => "horror-movie suspense",
            ChordQuality::MinorMajor7 => "noir, spy-film mystery",
            ChordQuality::AugmentedMajor7 => "eerie, glassy",
            ChordQuality::Minor9 => "lush, melancholic",
            ChordQuality::Add9 => "sparkly, hopeful",
        }
    }

    /// True when the chord's third is the small (minor) one, so its numeral is lowercase.
    pub fn is_minor_like(self) -> bool {
        self.intervals().get(1) == Some(&3)
    }

    /// The quality whose notes match these semitone gaps above the root, if there is one.
    pub fn from_intervals(intervals: &[u8]) -> Option<ChordQuality> {
        ChordQuality::ALL
            .into_iter()
            .find(|q| q.intervals() == intervals)
    }
}

const FLAT_NAMES: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];

/// A note name without octave, spelled with flats ("Bb") or sharps ("A#").
pub fn pitch_name(pitch: PitchClass, flats: bool) -> &'static str {
    let i = usize::from(pitch.0 % 12);
    if flats {
        FLAT_NAMES[i]
    } else {
        PitchClass::NAMES[i]
    }
}

/// True when this key is normally written with flats (D minor has B♭, so "Bb", not "A#").
pub fn key_uses_flats(key: PitchClass, scale: ScaleKind) -> bool {
    // Every 7-note scale here is a rotation of a major scale; that major key decides.
    let offset_from_parent_major = match scale {
        ScaleKind::Major | ScaleKind::MajorPentatonic | ScaleKind::Chromatic => 0,
        ScaleKind::NaturalMinor
        | ScaleKind::HarmonicMinor
        | ScaleKind::MinorPentatonic
        | ScaleKind::Blues => 9,
        ScaleKind::Dorian => 2,
        ScaleKind::Phrygian => 4,
        ScaleKind::Lydian => 5,
        ScaleKind::Mixolydian => 7,
        ScaleKind::Locrian => 11,
    };
    let parent = PitchClass::new(i32::from(key.0) - offset_from_parent_major);
    matches!(parent.0, 1 | 3 | 5 | 8 | 10)
}

/// A chord name like "Dm", "F#dim" or "Bbmaj7".
pub fn chord_name(root: PitchClass, quality: ChordQuality, flats: bool) -> String {
    format!("{}{}", pitch_name(root, flats), quality.suffix())
}

/// The MIDI notes of a chord built up from `root_midi` (notes above 127 are dropped).
pub fn chord_notes(root_midi: u8, quality: ChordQuality) -> Vec<u8> {
    quality
        .intervals()
        .iter()
        .map(|&i| u16::from(root_midi) + u16::from(i))
        .filter(|&n| n <= 127)
        .map(|n| n as u8)
        .collect()
}

/// The roman numeral for a chord on a scale degree: "i", "VI", "vii°", "V7".
///
/// The number says which note of the scale the chord stands on (I = the first). Uppercase
/// means a major-sounding chord, lowercase a minor-sounding one.
pub fn roman_numeral(degree: usize, quality: ChordQuality) -> String {
    const NUMERALS: [&str; 7] = ["I", "II", "III", "IV", "V", "VI", "VII"];
    let base = NUMERALS[degree % 7];
    let base = if quality.is_minor_like() {
        base.to_lowercase()
    } else {
        base.to_string()
    };
    let mark = match quality {
        ChordQuality::Major | ChordQuality::Minor => "",
        ChordQuality::Diminished => "°",
        ChordQuality::Augmented => "+",
        ChordQuality::Sus2 => "sus2",
        ChordQuality::Sus4 => "sus4",
        ChordQuality::Major7 => "maj7",
        ChordQuality::Minor7 | ChordQuality::Dominant7 => "7",
        ChordQuality::HalfDiminished7 => "ø7",
        ChordQuality::Diminished7 => "°7",
        ChordQuality::MinorMajor7 => "(maj7)",
        ChordQuality::AugmentedMajor7 => "+maj7",
        ChordQuality::Minor9 => "9",
        ChordQuality::Add9 => "add9",
    };
    format!("{base}{mark}")
}

/// The 7-note scale chords are built from.
///
/// Chords are stacks of every other scale note, which needs a 7-note scale. Pentatonic and
/// blues scales borrow the chords of their parent scale; chromatic (no scale) uses natural
/// minor, the home of most of Cedar Station's music.
pub fn harmony_scale(scale: ScaleKind) -> ScaleKind {
    match scale {
        ScaleKind::MajorPentatonic => ScaleKind::Major,
        ScaleKind::MinorPentatonic | ScaleKind::Blues | ScaleKind::Chromatic => {
            ScaleKind::NaturalMinor
        }
        other => other,
    }
}

/// A chord picked for a progression, ready to show and to play.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChordSpec {
    /// Which note of the scale the chord stands on: 0 = the key note (I), 4 = the fifth (V).
    pub degree: usize,
    pub root: PitchClass,
    pub quality: ChordQuality,
    /// MIDI notes, lowest (the root) first.
    pub notes: Vec<u8>,
    /// "Dm", "Bbmaj7".
    pub name: String,
    /// "i", "VI", "VII7".
    pub numeral: String,
}

/// The key note at or above `midi` (folded down an octave if that runs past 127).
fn tonic_at_or_above(key: PitchClass, midi: u8) -> u8 {
    let up = (i32::from(key.0 % 12) - i32::from(midi)).rem_euclid(12);
    let mut tonic = i32::from(midi) + up;
    while tonic > 127 {
        tonic -= 12;
    }
    tonic as u8
}

/// The chord that "belongs" to a key on one scale degree, built only from scale notes.
///
/// `degree` 0 is the home chord (I or i), 3 is IV/iv, 4 is V/v and so on. `tonic_midi` picks
/// the octave: the chord is built from the key note at or above it (50 in D = D3). With
/// `sevenths` the chord gets a fourth note, which sounds richer and jazzier.
pub fn diatonic_chord(
    key: PitchClass,
    scale: ScaleKind,
    degree: usize,
    tonic_midi: u8,
    sevenths: bool,
) -> ChordSpec {
    let harmony = harmony_scale(scale);
    let tonic = tonic_at_or_above(key, tonic_midi);
    let degree = degree % 7;
    let count = if sevenths { 4 } else { 3 };
    let notes: Vec<u8> = (0..count)
        .map(|i| harmony.degree(tonic, degree as i32 + 2 * i))
        .collect();
    let root_midi = notes.first().copied().unwrap_or(tonic);
    let gaps: Vec<u8> = notes.iter().map(|&n| n.saturating_sub(root_midi)).collect();
    let quality = ChordQuality::from_intervals(&gaps).unwrap_or(if gaps.get(1) == Some(&3) {
        ChordQuality::Minor
    } else {
        ChordQuality::Major
    });
    let root = PitchClass::new(i32::from(root_midi));
    ChordSpec {
        degree,
        root,
        quality,
        notes,
        name: chord_name(root, quality, key_uses_flats(key, scale)),
        numeral: roman_numeral(degree, quality),
    }
}

/// All seven chords of a key, I to VII: the palette a chord picker can offer.
pub fn diatonic_chords(
    key: PitchClass,
    scale: ScaleKind,
    tonic_midi: u8,
    sevenths: bool,
) -> Vec<ChordSpec> {
    (0..7)
        .map(|d| diatonic_chord(key, scale, d, tonic_midi, sevenths))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: PitchClass = PitchClass(2);

    #[test]
    fn chord_notes_stack_the_right_gaps() {
        assert_eq!(chord_notes(60, ChordQuality::Major), vec![60, 64, 67]);
        assert_eq!(chord_notes(62, ChordQuality::Minor7), vec![62, 65, 69, 72]);
        assert_eq!(chord_notes(126, ChordQuality::Major), vec![126]);
    }

    #[test]
    fn d_minor_chords_have_the_expected_names_and_numerals() {
        let chords = diatonic_chords(D, ScaleKind::NaturalMinor, 50, false);
        let names: Vec<&str> = chords.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Dm", "Edim", "F", "Gm", "Am", "Bb", "C"]);
        let numerals: Vec<&str> = chords.iter().map(|c| c.numeral.as_str()).collect();
        assert_eq!(numerals, ["i", "ii°", "III", "iv", "v", "VI", "VII"]);
        assert_eq!(chords[0].notes, vec![50, 53, 57]);
    }

    #[test]
    fn sevenths_in_d_minor() {
        let vi = diatonic_chord(D, ScaleKind::NaturalMinor, 5, 50, true);
        assert_eq!(vi.name, "Bbmaj7");
        assert_eq!(vi.numeral, "VImaj7");
        let ii = diatonic_chord(D, ScaleKind::NaturalMinor, 1, 50, true);
        assert_eq!(ii.quality, ChordQuality::HalfDiminished7);
    }

    #[test]
    fn harmonic_minor_has_a_major_five_and_every_chord_is_named() {
        let a = PitchClass(9);
        let v = diatonic_chord(a, ScaleKind::HarmonicMinor, 4, 45, false);
        assert_eq!(v.name, "E");
        for scale in ScaleKind::ALL {
            for sevenths in [false, true] {
                for c in diatonic_chords(a, scale, 45, sevenths) {
                    let gaps: Vec<u8> = c.notes.iter().map(|n| n - c.notes[0]).collect();
                    assert_eq!(
                        ChordQuality::from_intervals(&gaps),
                        Some(c.quality),
                        "{scale:?} {}",
                        c.name
                    );
                }
            }
        }
    }

    #[test]
    fn flats_follow_the_key_signature() {
        assert!(key_uses_flats(D, ScaleKind::NaturalMinor));
        assert!(!key_uses_flats(PitchClass(4), ScaleKind::NaturalMinor)); // E minor: F#
        assert!(key_uses_flats(PitchClass(5), ScaleKind::Major)); // F major: Bb
        assert!(!key_uses_flats(D, ScaleKind::Dorian)); // no accidentals
    }

    #[test]
    fn tonic_octave_is_found_upwards() {
        assert_eq!(tonic_at_or_above(D, 48), 50);
        assert_eq!(tonic_at_or_above(D, 50), 50);
        assert_eq!(tonic_at_or_above(PitchClass(11), 127), 119);
    }
}
