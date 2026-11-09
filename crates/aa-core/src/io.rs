//! Saving and loading projects.
//!
//! A project is a folder, friendly to git:
//!
//! ```text
//! CedarStation.aaproj/
//!   project.json   the whole model, human-readable
//!   samples/       WAV files the sampler zones point at
//!   renders/       exports (not committed)
//! ```

use crate::model::{FORMAT_VERSION, Project};
use std::fmt;
use std::path::{Path, PathBuf};

pub const PROJECT_FILE: &str = "project.json";
pub const SAMPLES_DIR: &str = "samples";
pub const EXTENSION: &str = "aaproj";

#[derive(Debug)]
pub enum IoError {
    Io(std::io::Error),
    Json(serde_json::Error),
    /// The file was written by a newer Adaptaudio.
    TooNew {
        found: u32,
    },
}

impl fmt::Display for IoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IoError::Io(e) => write!(f, "file error: {e}"),
            IoError::Json(e) => write!(f, "project file is damaged: {e}"),
            IoError::TooNew { found } => write!(
                f,
                "project format {found} is newer than this app ({FORMAT_VERSION})"
            ),
        }
    }
}

impl std::error::Error for IoError {}

impl From<std::io::Error> for IoError {
    fn from(e: std::io::Error) -> Self {
        IoError::Io(e)
    }
}

impl From<serde_json::Error> for IoError {
    fn from(e: serde_json::Error) -> Self {
        IoError::Json(e)
    }
}

/// Makes sure a path ends in `.aaproj`.
pub fn project_dir(path: &Path) -> PathBuf {
    if path.extension().is_some_and(|e| e == EXTENSION) {
        path.to_path_buf()
    } else {
        path.with_extension(EXTENSION)
    }
}

pub fn to_json(project: &Project) -> Result<String, IoError> {
    Ok(serde_json::to_string_pretty(project)?)
}

pub fn from_json(src: &str) -> Result<Project, IoError> {
    let raw: serde_json::Value = serde_json::from_str(src)?;
    let found = raw
        .get("format_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;
    if found > FORMAT_VERSION {
        return Err(IoError::TooNew { found });
    }
    let mut project: Project = serde_json::from_value(migrate(raw, found))?;
    project.format_version = FORMAT_VERSION;
    Ok(project)
}

/// Upgrades older files step by step. Version 1 is the first format, so nothing to do yet.
fn migrate(raw: serde_json::Value, _from: u32) -> serde_json::Value {
    raw
}

/// Writes `project.json` atomically (temp file + rename) so a crash never leaves half a file.
pub fn save(project: &Project, dir: &Path) -> Result<PathBuf, IoError> {
    let dir = project_dir(dir);
    std::fs::create_dir_all(dir.join(SAMPLES_DIR))?;
    let target = dir.join(PROJECT_FILE);
    let tmp = dir.join(format!("{PROJECT_FILE}.tmp"));
    std::fs::write(&tmp, to_json(project)?)?;
    std::fs::rename(&tmp, &target)?;
    Ok(dir)
}

pub fn load(dir: &Path) -> Result<Project, IoError> {
    let dir = project_dir(dir);
    let src = std::fs::read_to_string(dir.join(PROJECT_FILE))?;
    from_json(&src)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_survives_a_round_trip() {
        let p = crate::demo::cedar_station();
        let back = from_json(&to_json(&p).unwrap_or_default()).ok();
        assert_eq!(back.as_ref(), Some(&p));
    }

    #[test]
    fn files_from_the_future_are_refused() {
        let p = Project::new("x");
        let json = to_json(&p)
            .unwrap_or_default()
            .replace("\"format_version\": 1", "\"format_version\": 99");
        assert!(matches!(
            from_json(&json),
            Err(IoError::TooNew { found: 99 })
        ));
    }

    #[test]
    fn save_and_load_through_a_folder() {
        let dir = std::env::temp_dir().join(format!("aa-io-test-{}", std::process::id()));
        let p = crate::demo::cedar_station();
        let saved = save(&p, &dir).ok();
        assert!(
            saved
                .as_ref()
                .is_some_and(|d| d.extension().is_some_and(|e| e == EXTENSION))
        );
        let loaded = saved.and_then(|d| load(&d).ok());
        assert_eq!(loaded.as_ref(), Some(&p));
        if let Some(d) = Some(project_dir(&dir)) {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}
