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
    /// "anthropic", "openai" or "qwen".
    pub assistant_provider: String,
    /// Overrides the provider's API address (empty = the provider's own).
    pub assistant_base_url: String,
    /// Reduce animation (beat pulses, glows).
    pub calm_ui: bool,
    /// MIDI input (a hardware keyboard) that plays the selected track.
    pub midi_in: Option<String>,
    /// Which input channel Capture records: 0 = all mixed, 1 = input 1, 2 = input 2.
    pub audio_in_channel: u8,
    /// Projects opened lately, newest first.
    pub recent_projects: Vec<PathBuf>,
    /// Ask which project to open when the app starts.
    pub ask_project_on_start: bool,
    /// Start playing when a section is launched from the Session grid.
    pub play_on_launch: bool,
    /// Play a note when it is drawn in the editor.
    pub preview_notes: bool,

    /// Height of the note editor under the Session grid, in points.
    pub editor_height: f32,
    /// GitHub repository ("owner/name") whose releases update the app.
    pub update_repo: String,
    pub check_updates_on_start: bool,
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
            assistant_provider: "anthropic".into(),
            assistant_base_url: String::new(),
            calm_ui: false,
            midi_in: None,
            audio_in_channel: 0,
            recent_projects: Vec::new(),
            ask_project_on_start: true,
            play_on_launch: true,
            preview_notes: true,

            editor_height: 320.0,
            update_repo: String::new(),
            check_updates_on_start: true,
        }
    }
}

pub fn dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Library/Application Support/Adaptaudio"))
}

impl Settings {
    /// Puts `path` at the top of the recent projects.
    pub fn remember_project(&mut self, path: &std::path::Path) {
        self.recent_projects.retain(|p| p != path);
        self.recent_projects.insert(0, path.to_path_buf());
        self.recent_projects.truncate(8);
        self.last_project = Some(path.to_path_buf());
    }

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
