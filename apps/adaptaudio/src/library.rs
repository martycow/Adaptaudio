//! The sound library: synth sounds saved as files in
//! `~/Library/Application Support/Adaptaudio/sounds`, so any project and any track can use them.

use aa_core::SynthPatch;
use std::path::{Path, PathBuf};

const EXTENSION: &str = "aasound";

/// A sound in the library.
#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    pub name: String,
    pub path: PathBuf,
}

pub fn dir() -> Option<PathBuf> {
    crate::settings::dir().map(|d| d.join("sounds"))
}

/// Every saved sound in `dir`, by name.
pub fn list(dir: &Path) -> Vec<Saved> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Saved> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == EXTENSION))
        .filter_map(|path| {
            let name = load(&path).ok()?.name;
            Some(Saved { name, path })
        })
        .collect();
    out.sort_by_key(|s| s.name.to_lowercase());
    out
}

fn file_stem(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let clean = clean.trim();
    if clean.is_empty() {
        "Sound".into()
    } else {
        clean.to_string()
    }
}

/// Saves `patch` as a new file in `dir`. An existing sound is never overwritten: a second
/// "Bass" becomes "Bass 2".
pub fn save(dir: &Path, patch: &SynthPatch) -> Result<Saved, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let stem = file_stem(&patch.name);
    let (mut name, mut path) = (patch.name.clone(), dir.join(format!("{stem}.{EXTENSION}")));
    let mut n = 2;
    while path.exists() {
        name = format!("{} {n}", patch.name);
        path = dir.join(format!("{stem} {n}.{EXTENSION}"));
        n += 1;
    }
    let mut patch = patch.clone();
    patch.name = name.clone();
    let json = serde_json::to_string_pretty(&patch).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(Saved { name, path })
}

pub fn load(path: &Path) -> Result<SynthPatch, String> {
    let json = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&json).map_err(|e| e.to_string())
}

pub fn delete(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aa_core::TrackRole;

    #[test]
    fn a_saved_sound_comes_back_and_never_overwrites_another() {
        let dir = std::env::temp_dir().join(format!("aa-sounds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut patch = SynthPatch::preset(TrackRole::Bass);
        patch.name = "Mossy / bass".into();
        patch.filter.cutoff_hz = 321.0;
        let first = save(&dir, &patch).unwrap();
        let second = save(&dir, &patch).unwrap();
        assert_ne!(first.path, second.path);
        assert_eq!(second.name, "Mossy / bass 2");
        let sounds = list(&dir);
        assert_eq!(sounds.len(), 2);
        assert_eq!(sounds[0].name, "Mossy / bass");
        assert_eq!(load(&first.path).unwrap().filter.cutoff_hz, 321.0);
        delete(&first.path).unwrap();
        assert_eq!(list(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
