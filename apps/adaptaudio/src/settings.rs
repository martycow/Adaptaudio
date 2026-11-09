//! Application settings (not part of any project), stored in
//! `~/Library/Application Support/Adaptaudio/settings.json`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Output device name (substring match); `None` = system default.
    pub audio_device: Option<String>,
    /// Fixed buffer size in frames; `None` = device default.
    pub buffer_frames: Option<u32>,
    /// Seconds between autosaves of a project that has a folder (0 = off).
    pub autosave_seconds: u32,
    pub metronome: bool,
    /// MIDI output used by External tracks and Capture.
    pub midi_out: Option<String>,
    /// Audio input used by Capture (substring); `None` = default.
    pub audio_in: Option<String>,
    pub last_project: Option<PathBuf>,
    pub export_dir: Option<PathBuf>,
    pub assistant_model: String,
    /// Reduce animation (beat pulses, glows).
    pub calm_ui: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            audio_device: None,
            buffer_frames: None,
            autosave_seconds: 60,
            metronome: false,
            midi_out: None,
            audio_in: None,
            last_project: None,
            export_dir: None,
            assistant_model: "claude-opus-5-5".into(),
            calm_ui: false,
        }
    }
}

pub fn dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Library/Application Support/Adaptaudio"))
}

impl Settings {
    pub fn load() -> Settings {
        dir()
            .map(|d| d.join("settings.json"))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let d = dir().ok_or("no home folder")?;
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(d.join("settings.json"), json).map_err(|e| e.to_string())
    }
}
