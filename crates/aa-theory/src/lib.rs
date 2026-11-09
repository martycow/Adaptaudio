//! aa-theory: music helpers for people who know no music theory.
//!
//! Everything here is a pure function: the same inputs (and the same random seed) always give
//! the same notes. Nothing plays sound; the results are plain [`aa_core::Clip`]s and chord lists
//! the editor can show, audition and drop onto a track.
//!
//! - [`chord`]: chords, their names ("Dm", "Bbmaj7") and roman numerals ("i", "VI").
//! - [`mood`]: chord progressions and starting points per feeling (Dread, Panic, Cozy…).
//! - [`rhythm`]: Euclidean rhythms ("5 hits spread over 16 steps").
//! - [`motif`]: variations of a short tune: mirrored, backwards, shifted, stretched.
//! - [`melody`]: a melody generator that moves by small steps and ends phrases on home.
//! - [`accompaniment`]: arpeggios, bass lines and chord clips from a progression.
//! - [`rng`]: the tiny repeatable random number generator they all share.

pub mod accompaniment;
pub mod chord;
pub mod melody;
pub mod mood;
pub mod motif;
pub mod rhythm;
pub mod rng;
pub mod scale;

pub use accompaniment::{
    ArpPattern, BassStyle, arpeggiate, arpeggiate_with, bass_line, chord_clip,
};
pub use chord::{
    ChordQuality, ChordSpec, chord_name, chord_notes, diatonic_chord, diatonic_chords,
    harmony_scale, key_uses_flats, pitch_name, roman_numeral,
};
pub use melody::{Contour, generate_melody};
pub use mood::{Mood, MoodPreset, mood_preset, progressions, suggest_progression};
pub use motif::{
    augment, diminish, invert, motif_clip, retrograde, rhythmic_displace, snap_to_scale,
    transpose_in_scale, vary,
};
pub use rhythm::{euclid, euclid_clip, pattern_string};
pub use rng::Rng;
pub use scale::{from_scale_index, scale_index};
