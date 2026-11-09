//! The real-time audio engine: synth and sampler voices, the section sequencer with
//! crossfading decks, the adaptive layer, the mixer, and the CoreAudio output.

pub mod dsp;
pub mod engine;
pub mod host;
pub mod offline;
pub mod reverb;
pub mod samples;
pub mod telemetry;
pub mod voice;

pub use engine::{Command, Controller, Engine, ExternalNote, ExternalSink, Notice};
pub use host::{AudioHost, HostError};
pub use samples::{LoadedProject, SampleData};
pub use telemetry::{MAX_TRACKS, Telemetry};

#[cfg(test)]
mod tests;
