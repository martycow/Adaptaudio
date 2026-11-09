//! aa-export: turns a project into an FMOD-ready scene folder.
//!
//! In Cedar Station, FMOD plays the music. Adaptaudio exports stems plus a `manifest.json`, and
//! `tools/fmod-import/import_adaptaudio.js` builds the FMOD Studio event from it (see
//! docs/ARCHITECTURE.md, "Связь с игрой").
//!
//! The crate doesn't depend on the audio engine: audio comes from a [`StemRenderer`] the caller
//! passes in. Puzzle test vectors come in as plain JSON for the same reason.
//!
//! - [`convert`]: pure unit conversions (bars → samples, dB, seek speed, curve baking, quantize).
//! - [`manifest`]: the manifest types, [`build_manifest`] and [`validate_manifest`].
//! - [`export_scene`]: renders and writes everything.

pub mod convert;
pub mod export;
pub mod manifest;

pub use export::{
    BitDepth, ExportError, ExportOptions, ExportReport, MANIFEST_FILE, PUZZLE_VECTORS_FILE,
    PUZZLES_FILE, README_FILE, StemRenderer, export_scene, write_wav,
};
pub use manifest::{
    MANIFEST_VERSION, MODE_PARAMETER, Manifest, build_manifest, scene_key, validate_manifest,
};
