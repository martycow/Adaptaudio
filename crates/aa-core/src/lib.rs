//! Adaptaudio data model: projects, tracks, clips, instruments, adaptive rules and puzzles.
//!
//! Pure data and serialization. No audio, no UI: everything here is testable on its own.

pub mod demo;
pub mod io;
pub mod model;
pub mod music;

pub use model::*;
pub use music::{PitchClass, ScaleKind};

/// Steps per beat used everywhere in the sequencer (sixteenth notes in 4/4).
pub const STEPS_PER_BEAT: u16 = 4;
