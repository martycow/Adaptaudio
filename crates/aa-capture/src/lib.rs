//! aa-capture: auto-sampling hardware synths into multisample instruments, and measuring
//! round-trip latency.
//!
//! The workflow (see docs/VISION.md, "Capture"):
//! 1. Make a [`CapturePlan`]: which notes and velocities to play.
//! 2. [`CaptureSession::run`] plays it over MIDI and records one continuous take.
//! 3. [`slice_recording`] cuts the take into one trimmed sample per note.
//! 4. [`normalize_together`] evens out the level without flattening velocity.
//! 5. [`build_instrument`] writes the WAVs into the project and returns a `SamplerInstrument`.
//!
//! Everything except [`CaptureSession`] and [`list_input_devices`] is pure and works without
//! hardware.

pub mod analysis;
pub mod error;
pub mod instrument;
pub mod latency;
pub mod plan;
pub mod session;
pub mod slice;
pub mod wav;

pub use analysis::{
    SILENCE_DB, apply_fades, db_to_gain, detect_onset, find_loop_points, gain_to_db, normalize,
    peak_db, rms_db, to_mono, trim_silence,
};
pub use error::CaptureError;
pub use instrument::{
    CAPTURED_AMP_ENV, build_instrument, sample_file_name, unused_prefix, velocity_ranges,
    zone_ranges,
};
pub use latency::{click_signal, latency_to_samples, measure_latency};
pub use plan::{CapturePlan, PlannedTake, TimedMidi, clock_events, ms_to_samples};
pub use session::{
    CaptureProgress, CaptureSession, InputChannel, NoteSender, Recording, list_input_devices,
};
pub use slice::{
    CapturedNote, SILENT_TAKE_DB, TRIM_RANGE_DB, normalize_each, normalize_together, one_shot,
    slice_recording,
};
pub use wav::{load_wav, write_wav};

#[cfg(test)]
pub(crate) mod test_util {
    use std::path::PathBuf;

    /// A fresh, empty folder for one test, under the system temp folder.
    pub(crate) fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("aa-capture-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
