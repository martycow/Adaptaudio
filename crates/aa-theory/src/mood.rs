//! Feelings as a starting point: pick a mood, get chords, tempo, scale and sounds.

use aa_core::{PitchClass, ScaleKind, TrackRole};
use serde::{Deserialize, Serialize};

use crate::chord::{ChordSpec, diatonic_chord};
use crate::melody::Contour;
use crate::rng::Rng;

/// The feeling a piece of music should give.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mood {
    Dread,
    Curiosity,
    Panic,
    Relief,
    Cozy,
    Mystery,
    Triumph,
}

impl Mood {
    pub const ALL: [Mood; 7] = [
        Mood::Dread,
        Mood::Curiosity,
        Mood::Panic,
        Mood::Relief,
        Mood::Cozy,
        Mood::Mystery,
        Mood::Triumph,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Mood::Dread => "Dread",
            Mood::Curiosity => "Curiosity",
            Mood::Panic => "Panic",
            Mood::Relief => "Relief",
            Mood::Cozy => "Cozy",
            Mood::Mystery => "Mystery",
            Mood::Triumph => "Triumph",
        }
    }

    /// One line about the feeling and where it fits in the game.
    pub fn about(self) -> &'static str {
        match self {
            Mood::Dread => "Something is wrong and getting closer. Stealth, dark corridors.",
            Mood::Curiosity => "What's behind that door? Exploring, poking at things.",
            Mood::Panic => "Run! Chases, alarms, being seen.",
            Mood::Relief => "Safe at last. The danger has passed.",
            Mood::Cozy => "Warm and small: a burrow, a cup of tea, rain outside.",
            Mood::Mystery => "Clues that don't add up yet. Puzzles, strange places.",
            Mood::Triumph => "You did it! A solved puzzle, an escape.",
        }
    }

    /// How likely the suggested chords get a fourth note (sevenths sound softer, jazzier).
    fn seventh_chance(self) -> f32 {
        match self {
            Mood::Dread => 0.2,
            Mood::Curiosity => 0.3,
            Mood::Panic => 0.0,
            Mood::Relief => 0.4,
            Mood::Cozy => 0.7,
            Mood::Mystery => 0.6,
            Mood::Triumph => 0.1,
        }
    }
}

/// A ready starting point for a mood: tempo, scale, which tracks to make, what they sound like.
#[derive(Clone, Debug, PartialEq)]
pub struct MoodPreset {
    pub mood: Mood,
    /// Suggested tempo range in beats per minute, slowest first.
    pub tempo_bpm: [f32; 2],
    pub scale: ScaleKind,
    /// The tracks to start with, most important first.
    pub roles: &'static [TrackRole],
    /// One line for the UI.
    pub description: &'static str,
    /// What the synth sounds should be like, in plain words.
    pub synth_character: &'static str,
    /// How busy a generated melody should be (0 = sparse, 1 = busy).
    pub melody_density: f32,
    /// The shape a generated melody should follow.
    pub contour: Contour,
}

/// The starting point for a mood.
pub fn mood_preset(mood: Mood) -> MoodPreset {
    let preset = |tempo_bpm: [f32; 2],
                  scale: ScaleKind,
                  roles: &'static [TrackRole],
                  description: &'static str,
                  synth_character: &'static str,
                  melody_density: f32,
                  contour: Contour| MoodPreset {
        mood,
        tempo_bpm,
        scale,
        roles,
        description,
        synth_character,
        melody_density,
        contour,
    };
    match mood {
        Mood::Dread => preset(
            [70.0, 90.0],
            ScaleKind::HarmonicMinor,
            &[
                TrackRole::Pad,
                TrackRole::Bass,
                TrackRole::Ambience,
                TrackRole::Perc,
            ],
            "Slow and heavy, with long dark chords and a low pulse.",
            "Detuned saw pad behind a slowly closing low-pass filter, a deep sine sub bass, distant metal hits.",
            0.25,
            Contour::Valley,
        ),
        Mood::Curiosity => preset(
            [90.0, 110.0],
            ScaleKind::Mixolydian,
            &[
                TrackRole::Arp,
                TrackRole::Pad,
                TrackRole::Hats,
                TrackRole::Lead,
            ],
            "Light and questioning, with a bouncy arpeggio.",
            "Plucky square arp with a short decay, a soft sine pad, ticking hats.",
            0.5,
            Contour::Wave,
        ),
        Mood::Panic => preset(
            [150.0, 175.0],
            ScaleKind::Phrygian,
            &[
                TrackRole::Kick,
                TrackRole::Bass,
                TrackRole::Snare,
                TrackRole::Hats,
                TrackRole::Lead,
            ],
            "Fast and relentless: driving bass, pounding drums, a two-chord alarm.",
            "Distorted saw bass with filter snaps, punchy kick, a shrill detuned lead.",
            0.8,
            Contour::Rising,
        ),
        Mood::Relief => preset(
            [80.0, 100.0],
            ScaleKind::Lydian,
            &[
                TrackRole::Pad,
                TrackRole::Lead,
                TrackRole::Bass,
                TrackRole::Ambience,
            ],
            "Open and warm, with chords that land softly on home.",
            "Wide warm pad with slow attack, a gentle triangle lead, lots of reverb.",
            0.35,
            Contour::Falling,
        ),
        Mood::Cozy => preset(
            [75.0, 95.0],
            ScaleKind::MajorPentatonic,
            &[
                TrackRole::Pad,
                TrackRole::Lead,
                TrackRole::Perc,
                TrackRole::Bass,
            ],
            "Small and warm, a folk tune by the fire. Hard to play a wrong note.",
            "Round triangle lead, soft felt-like pad, wooden percussion. Try the tongue drum or ocarina.",
            0.45,
            Contour::Arch,
        ),
        Mood::Mystery => preset(
            [85.0, 100.0],
            ScaleKind::Dorian,
            &[
                TrackRole::Pad,
                TrackRole::Arp,
                TrackRole::Bass,
                TrackRole::Ambience,
            ],
            "Hushed and sneaky, with chords that never quite settle.",
            "Glassy bell arp, a breathy filtered pad, soft round bass.",
            0.4,
            Contour::Wave,
        ),
        Mood::Triumph => preset(
            [120.0, 140.0],
            ScaleKind::Major,
            &[
                TrackRole::Kick,
                TrackRole::Snare,
                TrackRole::Bass,
                TrackRole::Pad,
                TrackRole::Lead,
            ],
            "Big and proud: a rising tune over strong chords and drums.",
            "Bright brassy saw lead, a full pad, solid kick and snare.",
            0.6,
            Contour::Arch,
        ),
    }
}

/// Hand-picked chord progressions per mood, as scale degrees (0 = home chord, 4 = V…).
///
/// They work in any scale; they sound most like the mood in the scale of [`mood_preset`].
/// Every Dread progression starts on the home chord, so the darkness feels anchored.
pub fn progressions(mood: Mood) -> &'static [&'static [usize]] {
    match mood {
        Mood::Dread => &[&[0, 5, 3, 4], &[0, 1, 0, 5], &[0, 3, 0, 5], &[0, 0, 5, 4]],
        Mood::Curiosity => &[&[0, 4, 5, 3], &[0, 2, 3, 0], &[0, 6, 3, 0], &[0, 3, 0, 4]],
        Mood::Panic => &[&[0, 1, 0, 1], &[0, 6, 5, 6], &[0, 5, 6, 0], &[0, 1, 6, 1]],
        Mood::Relief => &[&[3, 4, 0, 0], &[5, 6, 0, 0], &[3, 0, 4, 0], &[1, 4, 0, 0]],
        Mood::Cozy => &[&[0, 3, 0, 4], &[0, 5, 3, 4], &[0, 2, 3, 0], &[0, 3, 5, 4]],
        Mood::Mystery => &[&[0, 6, 0, 5], &[0, 2, 1, 0], &[0, 5, 1, 4], &[0, 3, 6, 2]],
        Mood::Triumph => &[&[5, 6, 0, 0], &[0, 4, 5, 3], &[0, 5, 6, 0], &[0, 3, 4, 0]],
    }
}

/// Suggests a chord progression for a mood in your key and scale.
///
/// Picks one of the hand-picked [`progressions`] and varies it a little: sometimes one middle
/// chord is swapped for a close relative (a chord sharing two notes with it), and sometimes
/// every chord gets a seventh. The first and last chords are never swapped, so the
/// progression keeps its start and its landing. Chords sit around octave 3 (pad range).
pub fn suggest_progression(
    mood: Mood,
    key: PitchClass,
    scale: ScaleKind,
    rng: &mut Rng,
) -> Vec<ChordSpec> {
    let options = progressions(mood);
    let mut degrees: Vec<usize> = rng.pick(options).map_or_else(|| vec![0], |p| p.to_vec());
    if degrees.len() > 2 && rng.chance(0.3) {
        let i = 1 + rng.below(degrees.len() - 2);
        let relative = if rng.chance(0.5) { 2 } else { 5 };
        degrees[i] = (degrees[i] + relative) % 7;
    }
    let sevenths = rng.chance(mood.seventh_chance());
    let tonic_midi = 48 + key.0 % 12;
    degrees
        .into_iter()
        .map(|d| diatonic_chord(key, scale, d, tonic_midi, sevenths))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dread_always_starts_on_the_home_chord() {
        let d = PitchClass(2);
        for seed in 0..50 {
            let p =
                suggest_progression(Mood::Dread, d, ScaleKind::NaturalMinor, &mut Rng::new(seed));
            assert_eq!(p[0].degree, 0);
            assert!(p[0].name.starts_with("Dm"), "{}", p[0].name);
        }
    }

    #[test]
    fn relief_always_lands_on_home() {
        for seed in 0..50 {
            let p = suggest_progression(
                Mood::Relief,
                PitchClass(0),
                ScaleKind::Lydian,
                &mut Rng::new(seed),
            );
            assert_eq!(p.last().map(|c| c.degree), Some(0));
        }
    }

    #[test]
    fn suggestions_are_repeatable_and_vary_between_seeds() {
        let a = suggest_progression(
            Mood::Mystery,
            PitchClass(9),
            ScaleKind::Dorian,
            &mut Rng::new(3),
        );
        let b = suggest_progression(
            Mood::Mystery,
            PitchClass(9),
            ScaleKind::Dorian,
            &mut Rng::new(3),
        );
        assert_eq!(a, b);
        let all: Vec<Vec<ChordSpec>> = (0..20)
            .map(|s| {
                suggest_progression(
                    Mood::Mystery,
                    PitchClass(9),
                    ScaleKind::Dorian,
                    &mut Rng::new(s),
                )
            })
            .collect();
        assert!(all.iter().any(|p| p != &a));
    }

    #[test]
    fn every_mood_has_a_preset_with_a_sane_tempo() {
        for mood in Mood::ALL {
            let p = mood_preset(mood);
            assert_eq!(p.mood, mood);
            assert!(p.tempo_bpm[0] < p.tempo_bpm[1]);
            assert!(!p.roles.is_empty());
            assert!(!progressions(mood).is_empty());
        }
    }
}
