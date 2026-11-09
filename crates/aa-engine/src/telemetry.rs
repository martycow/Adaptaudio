//! What the audio thread publishes for the UI: lock-free atomics, read every frame.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};

pub const MAX_TRACKS: usize = 32;
pub const MAX_PARAMS: usize = aa_adaptive::MAX_PARAMS;

/// An `f32` in an `AtomicU32`.
#[derive(Debug, Default)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn new(v: f32) -> Self {
        AtomicF32(AtomicU32::new(v.to_bits()))
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }
}

/// An `f64` in an `AtomicU64`.
#[derive(Debug, Default)]
pub struct AtomicF64(AtomicU64);

impl AtomicF64 {
    pub fn get(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::Relaxed))
    }

    pub fn set(&self, v: f64) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }
}

#[derive(Debug, Default)]
pub struct TrackTelemetry {
    /// Peak level 0..1 of the last block (after fader).
    pub level: AtomicF32,
    /// 1 on a note start, decays to 0 (for flashing the track header).
    pub hit: AtomicF32,
    /// Playhead inside the clip the active deck plays (steps), or -1.
    pub clip_step: AtomicF32,
    /// Id of the clip the active deck plays on this track, or 0.
    pub clip: AtomicU32,
    /// The game's current mods on this track.
    pub gain_mod: AtomicF32,
    pub cutoff_mod: AtomicF32,
}

#[derive(Debug, Default)]
pub struct Telemetry {
    pub playing: AtomicBool,
    pub sample_rate: AtomicF32,
    pub bpm: AtomicF32,
    /// Transport position in steps.
    pub position: AtomicF64,
    /// 1 on each beat, decaying (drives the UI pulse).
    pub beat_pulse: AtomicF32,
    /// 1 on each bar.
    pub bar_pulse: AtomicF32,
    pub mode: AtomicI32,
    pub pending_mode: AtomicI32,
    pub pending_at: AtomicF64,
    /// The section each deck plays (0 = none) and its gain.
    pub deck_section: [AtomicU32; 2],
    pub deck_gain: [AtomicF32; 2],
    pub active_deck: AtomicU32,
    pub params: [AtomicF32; MAX_PARAMS],
    pub param_targets: [AtomicF32; MAX_PARAMS],
    /// Bit i = event i has fired.
    pub fired: AtomicU64,
    pub tracks: [TrackTelemetry; MAX_TRACKS],
    pub master_peak: [AtomicF32; 2],
    /// Share of the buffer's time spent rendering (0..1).
    pub load: AtomicF32,
    pub blocks: AtomicU64,
    /// Sidechain gain reduction 0..1.
    pub sidechain: AtomicF32,
}

impl Telemetry {
    pub fn new() -> Self {
        let t = Telemetry::default();
        t.mode.store(-1, Ordering::Relaxed);
        t.pending_mode.store(-1, Ordering::Relaxed);
        t
    }

    pub fn mode(&self) -> Option<usize> {
        usize::try_from(self.mode.load(Ordering::Relaxed)).ok()
    }

    pub fn pending_mode(&self) -> Option<usize> {
        usize::try_from(self.pending_mode.load(Ordering::Relaxed)).ok()
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }
}
