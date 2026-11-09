//! Edits the assistant proposes. This crate never applies them: the app turns each [`Action`]
//! into the same undoable command the UI uses.
//!
//! The model calls tools (JSON schemas in [`tool_definitions`]); [`ActionValidator`] checks
//! every call against the project and turns it into an [`Action`] or a readable reason to drop it.

use aa_core::{CurveTarget, Note, PitchClass, Project, ScaleKind, TrackRole};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MIN_BPM: f32 = 40.0;
pub const MAX_BPM: f32 = 240.0;
/// Longest clip the assistant may create: 16 bars of 16 steps.
pub const MAX_CLIP_STEPS: u16 = 256;
pub const MAX_NOTES: usize = 512;
pub const MAX_CURVE_POINTS: usize = 16;
pub const MIN_VOLUME_DB: f32 = -60.0;
pub const MAX_VOLUME_DB: f32 = 6.0;

/// One note as the assistant writes it. See [`NoteSpec::to_note`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NoteSpec {
    pub step: u16,
    /// MIDI note number, 60 = C4.
    pub pitch: u8,
    /// 0..1.
    pub velocity: f32,
    pub length_steps: f32,
}

impl NoteSpec {
    pub fn to_note(&self) -> Note {
        Note::new(self.step, self.pitch, self.velocity, self.length_steps)
    }
}

/// A proposed edit. Names refer to tracks, clips and sections as shown to the user; the
/// validator has already resolved them to the project's exact spelling (or to an item an
/// earlier action of the same reply creates).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Action {
    SetTempo {
        bpm: f32,
    },
    SetKeyScale {
        key: PitchClass,
        scale: ScaleKind,
    },
    CreateClip {
        track_name: String,
        clip_name: String,
        length_steps: u16,
        notes: Vec<NoteSpec>,
    },
    ReplaceClipNotes {
        track_name: String,
        clip_name: String,
        notes: Vec<NoteSpec>,
    },
    AddTrack {
        name: String,
        role: TrackRole,
    },
    AssignClip {
        section: String,
        track_name: String,
        clip_name: String,
    },
    SetCurve {
        track_name: String,
        param: String,
        target: CurveTarget,
        /// Sorted by input; input 0..1, output in the target's units.
        points: Vec<[f32; 2]>,
    },
    SetInfluence {
        track_name: String,
        value: f32,
    },
    SetMixer {
        track_name: String,
        volume_db: Option<f32>,
        pan: Option<f32>,
        reverb_send: Option<f32>,
    },
    /// Changes nothing: the explanation is in the reply text. The app may link the glossary.
    Explain {
        term: String,
    },
}

impl Action {
    /// A short English label, e.g. for the undo history.
    pub fn describe(&self) -> String {
        match self {
            Action::SetTempo { bpm } => format!("Set tempo to {bpm} BPM"),
            Action::SetKeyScale { key, scale } => {
                format!("Set key to {} {}", key.name(), scale.name())
            }
            Action::CreateClip {
                track_name,
                clip_name,
                length_steps,
                notes,
            } => format!(
                "Create clip \"{clip_name}\" on {track_name} ({length_steps} steps, {} notes)",
                notes.len()
            ),
            Action::ReplaceClipNotes {
                track_name,
                clip_name,
                notes,
            } => {
                format!(
                    "Rewrite \"{clip_name}\" on {track_name} ({} notes)",
                    notes.len()
                )
            }
            Action::AddTrack { name, role } => format!("Add {} track \"{name}\"", role.name()),
            Action::AssignClip {
                section,
                track_name,
                clip_name,
            } => {
                format!("Play \"{clip_name}\" on {track_name} in section \"{section}\"")
            }
            Action::SetCurve {
                track_name,
                param,
                target,
                ..
            } => {
                format!("Map {param} to {} on {track_name}", target.name())
            }
            Action::SetInfluence { track_name, value } => {
                format!("Set influence of {track_name} to {value:.2}")
            }
            Action::SetMixer { track_name, .. } => format!("Adjust mixer of {track_name}"),
            Action::Explain { term } => format!("Explain \"{term}\""),
        }
    }

    /// True for actions that change the project.
    pub fn is_edit(&self) -> bool {
        !matches!(self, Action::Explain { .. })
    }
}

// ---------------------------------------------------------------------------------------------
// Tool schemas

pub const TOOL_SET_TEMPO: &str = "set_tempo";
pub const TOOL_SET_KEY_SCALE: &str = "set_key_scale";
pub const TOOL_CREATE_CLIP: &str = "create_clip";
pub const TOOL_REPLACE_CLIP_NOTES: &str = "replace_clip_notes";
pub const TOOL_ADD_TRACK: &str = "add_track";
pub const TOOL_ASSIGN_CLIP: &str = "assign_clip";
pub const TOOL_SET_CURVE: &str = "set_curve";
pub const TOOL_SET_INFLUENCE: &str = "set_influence";
pub const TOOL_SET_MIXER: &str = "set_mixer";
pub const TOOL_EXPLAIN_TERM: &str = "explain_term";

fn key_names() -> Vec<&'static str> {
    PitchClass::NAMES.to_vec()
}

fn scale_names() -> Vec<&'static str> {
    ScaleKind::ALL.iter().map(|s| s.name()).collect()
}

fn role_names() -> Vec<&'static str> {
    TrackRole::ALL.iter().map(|r| r.name()).collect()
}

fn target_names() -> Vec<&'static str> {
    CurveTarget::ALL.iter().map(|t| t.name()).collect()
}

fn notes_schema() -> Value {
    json!({
        "type": "array",
        "description": "The notes. An empty array makes a silent clip.",
        "items": {
            "type": "object",
            "properties": {
                "step": {
                    "type": "integer", "minimum": 0,
                    "description": "Start step, 0-based; 16 steps per 4/4 bar, 4 per beat. Must be less than the clip length."
                },
                "pitch": {
                    "type": "integer", "minimum": 0, "maximum": 127,
                    "description": "MIDI note number: 60 = C4 (middle C), 36 = C2, 48 = C3. Drum tracks: kick 36, snare 38, closed hat 42, open hat 46."
                },
                "velocity": {
                    "type": "number", "minimum": 0, "maximum": 1,
                    "description": "How hard the note is hit, 0..1 (0.8 is a normal hit)."
                },
                "length_steps": {
                    "type": "number", "exclusiveMinimum": 0,
                    "description": "Duration in steps (1 = a sixteenth note, 4 = a beat)."
                }
            },
            "required": ["step", "pitch", "velocity", "length_steps"],
            "additionalProperties": false
        }
    })
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "input_schema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        }
    })
}

/// The tool list sent with every request. Built only from constants, so it is byte-identical
/// across requests (a changing tool list would invalidate the prompt cache and earlier thinking).
pub fn tool_definitions() -> Value {
    let track_name =
        json!({"type": "string", "description": "Exact track name from the project state."});
    let clip_name = json!({"type": "string", "description": "Clip name on that track."});
    Value::Array(vec![
        tool(
            TOOL_SET_TEMPO,
            "Propose a new project tempo. Call when the user asks for a faster/slower feel or a specific BPM.",
            json!({"bpm": {"type": "number", "minimum": MIN_BPM, "maximum": MAX_BPM, "description": "Beats per minute."}}),
            &["bpm"],
        ),
        tool(
            TOOL_SET_KEY_SCALE,
            "Propose a new key (tonic) and scale for the whole project. Call when the user wants a different mood or names a key, e.g. \"D minor\".",
            json!({
                "key": {"type": "string", "enum": key_names(), "description": "Tonic note."},
                "scale": {"type": "string", "enum": scale_names()}
            }),
            &["key", "scale"],
        ),
        tool(
            TOOL_CREATE_CLIP,
            "Propose a new clip (a looping pattern) on an existing track, or on a track an earlier add_track call in this reply creates. Call whenever the user asks you to write a beat, bassline, melody, chords or arpeggio. Use a new clip name; to change an existing clip use replace_clip_notes.",
            json!({
                "track_name": track_name,
                "clip_name": {"type": "string", "description": "Short new clip name, e.g. \"Stealth bass\"."},
                "length_steps": {"type": "integer", "minimum": 1, "maximum": MAX_CLIP_STEPS, "description": "Loop length in steps: 16 = 1 bar, 32 = 2 bars, 64 = 4 bars."},
                "notes": notes_schema()
            }),
            &["track_name", "clip_name", "length_steps", "notes"],
        ),
        tool(
            TOOL_REPLACE_CLIP_NOTES,
            "Propose new notes for an existing clip, keeping its name and length. Call when the user asks to change, simplify, darken or vary a pattern that already exists.",
            json!({"track_name": track_name, "clip_name": clip_name, "notes": notes_schema()}),
            &["track_name", "clip_name", "notes"],
        ),
        tool(
            TOOL_ADD_TRACK,
            "Propose a new track (one instrument layer). Call when the user's request needs a layer the project does not have yet.",
            json!({
                "name": {"type": "string", "description": "New, unique track name."},
                "role": {"type": "string", "enum": role_names(), "description": "What the track does; picks its starting sound."}
            }),
            &["name", "role"],
        ),
        tool(
            TOOL_ASSIGN_CLIP,
            "Propose which clip a track plays in a section (sections are what game modes play). Call after creating a clip the user wants to hear in a section.",
            json!({
                "section": {"type": "string", "description": "Exact section name from the project state."},
                "track_name": track_name,
                "clip_name": clip_name
            }),
            &["section", "track_name", "clip_name"],
        ),
        tool(
            TOOL_SET_CURVE,
            "Propose a response curve: how one game parameter (0..1) drives one sound property of a track. Replaces an existing curve for the same parameter and target. Call when the user wants a layer to react to the game (e.g. \"bass gets louder in a chase\").",
            json!({
                "track_name": track_name,
                "param": {"type": "string", "description": "Game parameter name from the project state, e.g. Intensity."},
                "target": {"type": "string", "enum": target_names(), "description": "Volume, Cutoff, Density, Drive, Reverb, Wobble: output 0..1. Detune: output in cents 0..100."},
                "points": {
                    "type": "array", "minItems": 1, "maxItems": MAX_CURVE_POINTS,
                    "description": "[input, output] pairs, input 0..1 ascending; linear between points.",
                    "items": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2}
                }
            }),
            &["track_name", "param", "target", "points"],
        ),
        tool(
            TOOL_SET_INFLUENCE,
            "Propose how strongly a track follows the game: 0 ignores its response curves, 1 follows them fully.",
            json!({"track_name": track_name, "value": {"type": "number", "minimum": 0, "maximum": 1}}),
            &["track_name", "value"],
        ),
        tool(
            TOOL_SET_MIXER,
            "Propose mixer changes for a track. Give only the fields to change.",
            json!({
                "track_name": track_name,
                "volume_db": {"type": "number", "minimum": MIN_VOLUME_DB, "maximum": MAX_VOLUME_DB, "description": "Track volume in dB; 0 is unchanged level, -6 is about half as loud."},
                "pan": {"type": "number", "minimum": -1, "maximum": 1, "description": "-1 left, 0 centre, 1 right."},
                "reverb_send": {"type": "number", "minimum": 0, "maximum": 1, "description": "How much of the track goes to the reverb."}
            }),
            &["track_name"],
        ),
        tool(
            TOOL_EXPLAIN_TERM,
            "Mark that your reply explains a music or audio term (e.g. \"sidechain\", \"Dorian\"), so the app can link its glossary. Changes nothing; write the explanation itself in your reply.",
            json!({"term": {"type": "string", "description": "The term, in English."}}),
            &["term"],
        ),
    ])
}

// ---------------------------------------------------------------------------------------------
// Validation

/// Checks tool calls against the project plus the edits already proposed in the same reply,
/// so "add a track, then a clip on it" works within one answer.
#[derive(Debug)]
pub struct ActionValidator<'a> {
    project: &'a Project,
    new_tracks: Vec<String>,
    /// (track, clip, length_steps) of clips proposed in this reply.
    new_clips: Vec<(String, String, u16)>,
}

impl<'a> ActionValidator<'a> {
    pub fn new(project: &'a Project) -> Self {
        ActionValidator {
            project,
            new_tracks: Vec::new(),
            new_clips: Vec::new(),
        }
    }

    /// Turns one tool call into an action, or explains why it is invalid.
    pub fn check(&mut self, tool_name: &str, input: &Value) -> Result<Action, String> {
        if !input.is_object() {
            return Err("tool input must be a JSON object".to_string());
        }
        let action = match tool_name {
            TOOL_SET_TEMPO => {
                let bpm = number(input, "bpm")?;
                in_range("bpm", bpm, MIN_BPM, MAX_BPM)?;
                Action::SetTempo { bpm }
            }
            TOOL_SET_KEY_SCALE => {
                let key = parse_key(&text(input, "key")?)?;
                let scale = parse_scale(&text(input, "scale")?)?;
                Action::SetKeyScale { key, scale }
            }
            TOOL_CREATE_CLIP => self.check_create_clip(input)?,
            TOOL_REPLACE_CLIP_NOTES => {
                let track_name = self.resolve_track(&text(input, "track_name")?)?;
                let (clip_name, length) =
                    self.resolve_clip(&track_name, &text(input, "clip_name")?)?;
                let notes = parse_notes(input, length)?;
                Action::ReplaceClipNotes {
                    track_name,
                    clip_name,
                    notes,
                }
            }
            TOOL_ADD_TRACK => {
                let name = text(input, "name")?;
                if self.resolve_track(&name).is_ok() {
                    return Err(format!("a track named \"{name}\" already exists"));
                }
                let role = parse_role(&text(input, "role")?)?;
                self.new_tracks.push(name.clone());
                Action::AddTrack { name, role }
            }
            TOOL_ASSIGN_CLIP => {
                let section = self.resolve_section(&text(input, "section")?)?;
                let track_name = self.resolve_track(&text(input, "track_name")?)?;
                let (clip_name, _) = self.resolve_clip(&track_name, &text(input, "clip_name")?)?;
                Action::AssignClip {
                    section,
                    track_name,
                    clip_name,
                }
            }
            TOOL_SET_CURVE => {
                let track_name = self.resolve_track(&text(input, "track_name")?)?;
                let param = self.resolve_param(&text(input, "param")?)?;
                let target = parse_target(&text(input, "target")?)?;
                let points = parse_points(input, target)?;
                Action::SetCurve {
                    track_name,
                    param,
                    target,
                    points,
                }
            }
            TOOL_SET_INFLUENCE => {
                let track_name = self.resolve_track(&text(input, "track_name")?)?;
                let value = number(input, "value")?;
                in_range("value", value, 0.0, 1.0)?;
                Action::SetInfluence { track_name, value }
            }
            TOOL_SET_MIXER => {
                let track_name = self.resolve_track(&text(input, "track_name")?)?;
                let volume_db = optional_number(input, "volume_db")?;
                let pan = optional_number(input, "pan")?;
                let reverb_send = optional_number(input, "reverb_send")?;
                if let Some(v) = volume_db {
                    in_range("volume_db", v, MIN_VOLUME_DB, MAX_VOLUME_DB)?;
                }
                if let Some(v) = pan {
                    in_range("pan", v, -1.0, 1.0)?;
                }
                if let Some(v) = reverb_send {
                    in_range("reverb_send", v, 0.0, 1.0)?;
                }
                if volume_db.is_none() && pan.is_none() && reverb_send.is_none() {
                    return Err("give at least one of volume_db, pan, reverb_send".to_string());
                }
                Action::SetMixer {
                    track_name,
                    volume_db,
                    pan,
                    reverb_send,
                }
            }
            TOOL_EXPLAIN_TERM => Action::Explain {
                term: text(input, "term")?,
            },
            other => return Err(format!("unknown tool \"{other}\"")),
        };
        Ok(action)
    }

    fn check_create_clip(&mut self, input: &Value) -> Result<Action, String> {
        let track_name = self.resolve_track(&text(input, "track_name")?)?;
        let clip_name = text(input, "clip_name")?;
        if self.resolve_clip(&track_name, &clip_name).is_ok() {
            return Err(format!(
                "track \"{track_name}\" already has a clip \"{clip_name}\"; use replace_clip_notes or a new name"
            ));
        }
        let length = integer(input, "length_steps")?;
        if !(1..=u64::from(MAX_CLIP_STEPS)).contains(&length) {
            return Err(format!(
                "length_steps must be 1..={MAX_CLIP_STEPS}, got {length}"
            ));
        }
        let length_steps = length as u16;
        let notes = parse_notes(input, length_steps)?;
        self.new_clips
            .push((track_name.clone(), clip_name.clone(), length_steps));
        Ok(Action::CreateClip {
            track_name,
            clip_name,
            length_steps,
            notes,
        })
    }

    fn resolve_track(&self, name: &str) -> Result<String, String> {
        if let Some(track) = self
            .project
            .tracks
            .iter()
            .find(|t| same_name(&t.name, name))
        {
            return Ok(track.name.clone());
        }
        if let Some(track) = self.new_tracks.iter().find(|t| same_name(t, name)) {
            return Ok(track.clone());
        }
        Err(format!("no track named \"{name}\""))
    }

    /// The clip's exact name and length, looked up on the (already resolved) track.
    fn resolve_clip(&self, track_name: &str, clip_name: &str) -> Result<(String, u16), String> {
        let existing = self
            .project
            .tracks
            .iter()
            .filter(|t| t.name == track_name)
            .flat_map(|t| t.clips.iter())
            .find(|c| same_name(&c.name, clip_name))
            .map(|c| (c.name.clone(), c.length_steps));
        let proposed = || {
            self.new_clips
                .iter()
                .find(|(t, c, _)| t == track_name && same_name(c, clip_name))
                .map(|(_, c, len)| (c.clone(), *len))
        };
        existing
            .or_else(proposed)
            .ok_or_else(|| format!("track \"{track_name}\" has no clip \"{clip_name}\""))
    }

    fn resolve_section(&self, name: &str) -> Result<String, String> {
        self.project
            .sections
            .iter()
            .find(|s| same_name(&s.name, name))
            .map(|s| s.name.clone())
            .ok_or_else(|| format!("no section named \"{name}\""))
    }

    fn resolve_param(&self, name: &str) -> Result<String, String> {
        self.project
            .game_params
            .iter()
            .find(|p| same_name(&p.name, name))
            .map(|p| p.name.clone())
            .ok_or_else(|| {
                let known: Vec<&str> = self
                    .project
                    .game_params
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect();
                format!(
                    "unknown game parameter \"{name}\"; known: {}",
                    known.join(", ")
                )
            })
    }
}

/// Names match ignoring case and surrounding spaces (Cyrillic included).
fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

fn text(input: &Value, field: &str) -> Result<String, String> {
    let value = input
        .get(field)
        .ok_or_else(|| format!("missing \"{field}\""))?;
    let s = value
        .as_str()
        .ok_or_else(|| format!("\"{field}\" must be a string"))?
        .trim();
    if s.is_empty() {
        return Err(format!("\"{field}\" is empty"));
    }
    if s.chars().count() > 80 {
        return Err(format!("\"{field}\" is longer than 80 characters"));
    }
    Ok(s.to_string())
}

fn number(input: &Value, field: &str) -> Result<f32, String> {
    optional_number(input, field)?.ok_or_else(|| format!("missing \"{field}\""))
}

fn optional_number(input: &Value, field: &str) -> Result<Option<f32>, String> {
    match input.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let n = value
                .as_f64()
                .ok_or_else(|| format!("\"{field}\" must be a number"))?;
            if !n.is_finite() {
                return Err(format!("\"{field}\" must be finite"));
            }
            Ok(Some(n as f32))
        }
    }
}

fn integer(input: &Value, field: &str) -> Result<u64, String> {
    let value = input
        .get(field)
        .ok_or_else(|| format!("missing \"{field}\""))?;
    value
        .as_u64()
        .ok_or_else(|| format!("\"{field}\" must be a non-negative integer"))
}

fn in_range(field: &str, value: f32, min: f32, max: f32) -> Result<(), String> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(format!("\"{field}\" must be {min}..={max}, got {value}"))
    }
}

fn parse_notes(input: &Value, clip_length: u16) -> Result<Vec<NoteSpec>, String> {
    let items = input
        .get("notes")
        .ok_or("missing \"notes\"")?
        .as_array()
        .ok_or("\"notes\" must be an array")?;
    if items.len() > MAX_NOTES {
        return Err(format!("too many notes ({}, max {MAX_NOTES})", items.len()));
    }
    let mut notes = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let note = parse_note(item, clip_length).map_err(|e| format!("note {i}: {e}"))?;
        notes.push(note);
    }
    notes.sort_by_key(|n| (n.step, n.pitch));
    notes.dedup_by(|a, b| a.step == b.step && a.pitch == b.pitch);
    Ok(notes)
}

fn parse_note(item: &Value, clip_length: u16) -> Result<NoteSpec, String> {
    if !item.is_object() {
        return Err("must be an object".to_string());
    }
    let step = integer(item, "step")?;
    if step >= u64::from(clip_length) {
        return Err(format!(
            "step {step} is outside the clip (length {clip_length} steps)"
        ));
    }
    let pitch = integer(item, "pitch")?;
    if pitch > 127 {
        return Err(format!("pitch {pitch} is outside MIDI range 0..=127"));
    }
    let velocity = optional_number(item, "velocity")?.unwrap_or(0.8);
    in_range("velocity", velocity, 0.0, 1.0)?;
    let length_steps = optional_number(item, "length_steps")?.unwrap_or(1.0);
    if length_steps <= 0.0 || length_steps > f32::from(clip_length) {
        return Err(format!(
            "length_steps must be > 0 and at most {clip_length}, got {length_steps}"
        ));
    }
    Ok(NoteSpec {
        step: step as u16,
        pitch: pitch as u8,
        velocity,
        length_steps,
    })
}

fn parse_points(input: &Value, target: CurveTarget) -> Result<Vec<[f32; 2]>, String> {
    let items = input
        .get("points")
        .ok_or("missing \"points\"")?
        .as_array()
        .ok_or("\"points\" must be an array")?;
    if items.is_empty() || items.len() > MAX_CURVE_POINTS {
        return Err(format!("give 1..={MAX_CURVE_POINTS} points"));
    }
    let max_out = if target == CurveTarget::Detune {
        100.0
    } else {
        1.0
    };
    let mut points = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let pair = item
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or_else(|| format!("point {i} must be [input, output]"))?;
        let x = pair[0]
            .as_f64()
            .ok_or_else(|| format!("point {i}: input must be a number"))? as f32;
        let y = pair[1]
            .as_f64()
            .ok_or_else(|| format!("point {i}: output must be a number"))? as f32;
        in_range("point input", x, 0.0, 1.0).map_err(|e| format!("point {i}: {e}"))?;
        in_range("point output", y, 0.0, max_out).map_err(|e| format!("point {i}: {e}"))?;
        points.push([x, y]);
    }
    points.sort_by(|a, b| a[0].total_cmp(&b[0]));
    Ok(points)
}

/// Accepts the names in [`PitchClass::NAMES`] plus flats and Unicode accidentals.
pub fn parse_key(name: &str) -> Result<PitchClass, String> {
    let normalized = name.trim().replace('♯', "#").replace('♭', "b");
    let mut chars = normalized.chars();
    let letter = chars.next().map(|c| c.to_ascii_uppercase());
    let natural = match letter {
        Some('C') => 0,
        Some('D') => 2,
        Some('E') => 4,
        Some('F') => 5,
        Some('G') => 7,
        Some('A') => 9,
        Some('B') => 11,
        _ => {
            return Err(format!(
                "unknown key \"{name}\"; use one of {}",
                key_names().join(", ")
            ));
        }
    };
    let offset = match chars.as_str() {
        "" => 0,
        "#" => 1,
        "b" => -1,
        _ => {
            return Err(format!(
                "unknown key \"{name}\"; use one of {}",
                key_names().join(", ")
            ));
        }
    };
    Ok(PitchClass::new(natural + offset))
}

pub fn parse_scale(name: &str) -> Result<ScaleKind, String> {
    let wanted = name.trim();
    ScaleKind::ALL
        .into_iter()
        .find(|s| {
            s.name().eq_ignore_ascii_case(wanted) || format!("{s:?}").eq_ignore_ascii_case(wanted)
        })
        .or_else(|| match wanted.to_ascii_lowercase().as_str() {
            "natural minor" | "aeolian" => Some(ScaleKind::NaturalMinor),
            "ionian" => Some(ScaleKind::Major),
            _ => None,
        })
        .ok_or_else(|| {
            format!(
                "unknown scale \"{name}\"; use one of {}",
                scale_names().join(", ")
            )
        })
}

pub fn parse_role(name: &str) -> Result<TrackRole, String> {
    let wanted = name.trim();
    TrackRole::ALL
        .into_iter()
        .find(|r| r.name().eq_ignore_ascii_case(wanted))
        .ok_or_else(|| {
            format!(
                "unknown role \"{name}\"; use one of {}",
                role_names().join(", ")
            )
        })
}

pub fn parse_target(name: &str) -> Result<CurveTarget, String> {
    let wanted = name.trim();
    CurveTarget::ALL
        .into_iter()
        .find(|t| {
            t.name().eq_ignore_ascii_case(wanted) || format!("{t:?}").eq_ignore_ascii_case(wanted)
        })
        .ok_or_else(|| {
            format!(
                "unknown target \"{name}\"; use one of {}",
                target_names().join(", ")
            )
        })
}
