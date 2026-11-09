//! Long-running jobs off the UI thread (assistant, export, capture) and their results.

use crate::state::App;
use aa_assistant::{Action, Assistant, CurlTransport, Reply};
use aa_core::{Clip, Project, ResponseCurve};
use aa_engine::{Engine, LoadedProject};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub from_user: bool,
    pub text: String,
    /// Proposed edits, with whether each was applied.
    pub actions: Vec<(Action, bool)>,
    pub warnings: Vec<String>,
    pub error: bool,
}

pub struct Extras {
    pub assistant: Option<Assistant>,
    pub chat: Vec<ChatMessage>,
    pub chat_input: rune::TextEdit,
    pub chat_rx: Option<Receiver<(Assistant, Result<Reply, String>)>>,
    pub api_key_input: rune::TextEdit,
    pub has_key: Option<bool>,
    pub export_busy: bool,
    pub export_rx: Option<Receiver<Result<String, String>>>,
    pub export_last: Option<String>,
    pub puzzle: crate::screens::puzzle::PuzzlePlay,
    pub seed: u64,
    pub capture: crate::screens::capture::CaptureUi,
}

impl Default for Extras {
    fn default() -> Self {
        Extras {
            assistant: Some(Assistant::new()),
            chat: Vec::new(),
            chat_input: rune::TextEdit::new(""),
            chat_rx: None,
            api_key_input: rune::TextEdit::new(""),
            has_key: None,
            export_busy: false,
            export_rx: None,
            export_last: None,
            puzzle: Default::default(),
            seed: 0xCEDA_5747,
            capture: Default::default(),
        }
    }
}

/// Sends External MIDI notes from the engine to the hardware.
pub fn wire_external_midi(engine: &mut Engine, app: &mut App) {
    crate::screens::capture::wire_external(engine, app);
}

/// Collects finished jobs. True when the view should rebuild.
pub fn poll(s: &mut App, now: f64, tx: &mut rune::Tx) -> bool {
    let mut changed = false;
    if let Some(rx) = &s.ext.chat_rx {
        match rx.try_recv() {
            Ok((assistant, result)) => {
                s.ext.assistant = Some(assistant);
                s.ext.chat_rx = None;
                match result {
                    Ok(reply) => s.ext.chat.push(ChatMessage {
                        from_user: false,
                        text: reply.text,
                        actions: reply.actions.into_iter().map(|a| (a, false)).collect(),
                        warnings: reply.warnings,
                        error: false,
                    }),
                    Err(e) => s.ext.chat.push(ChatMessage {
                        from_user: false,
                        text: e,
                        actions: Vec::new(),
                        warnings: Vec::new(),
                        error: true,
                    }),
                }
                changed = true;
            }
            Err(TryRecvError::Empty) => tx.wake_after(Duration::from_millis(200)),
            Err(TryRecvError::Disconnected) => {
                s.ext.chat_rx = None;
                s.ext.assistant = Some(Assistant::new());
                changed = true;
            }
        }
    }
    if let Some(rx) = &s.ext.export_rx {
        match rx.try_recv() {
            Ok(result) => {
                s.ext.export_rx = None;
                s.ext.export_busy = false;
                match result {
                    Ok(msg) => {
                        s.toast(&msg);
                        s.ext.export_last = Some(msg);
                    }
                    Err(e) => s.toast_error(&format!("Export failed: {e}")),
                }
                changed = true;
            }
            Err(TryRecvError::Empty) => tx.wake_after(Duration::from_millis(250)),
            Err(TryRecvError::Disconnected) => {
                s.ext.export_rx = None;
                s.ext.export_busy = false;
                changed = true;
            }
        }
    }
    changed |= crate::screens::puzzle::poll(s, now, tx);
    changed |= crate::screens::capture::poll(s, now, tx);
    changed
}

// ---------------------------------------------------------------------------------- assistant

pub fn ask(s: &mut App) {
    let question = s.ext.chat_input.text.trim().to_string();
    if question.is_empty() || s.ext.chat_rx.is_some() {
        return;
    }
    let Some(mut assistant) = s.ext.assistant.take() else {
        return;
    };
    assistant.model = s.settings.assistant_model.clone();
    s.ext.chat_input = rune::TextEdit::new("");
    s.ext.chat.push(ChatMessage {
        from_user: true,
        text: question.clone(),
        actions: Vec::new(),
        warnings: Vec::new(),
        error: false,
    });
    let project = s.project.clone();
    let selection = selection_context(s);
    let (tx, rx) = channel();
    s.ext.chat_rx = Some(rx);
    std::thread::spawn(move || {
        let transport = CurlTransport::default();
        let result = assistant
            .ask(&transport, &project, &selection, &question)
            .map_err(|e| e.to_string());
        let _ = tx.send((assistant, result));
    });
}

fn selection_context(s: &App) -> String {
    let mut out = format!("Screen: {}.", s.screen.name());
    if let Some(m) = s.mode.and_then(|m| s.project.game_modes.get(m)) {
        out.push_str(&format!(" Playing game mode: {}.", m.name));
    }
    if let (Some(t), Some(c)) = (
        s.selected_track.and_then(|t| s.project.track(t)),
        s.selected_clip,
    ) {
        out.push_str(&format!(" Selected track: {} ({}).", t.name, t.role.name()));
        if let Some(clip) = t.clip(c) {
            out.push_str(&format!(
                " Selected clip “{}”: {}",
                clip.name,
                aa_assistant::clip_notes_text(clip)
            ));
        }
    }
    out
}

/// Applies one proposed edit as an undo step. Returns an error message if it no longer fits.
pub fn apply_action(s: &mut App, action: &Action) -> Result<(), String> {
    let mut next = s.project.clone();
    apply_to(&mut next, action)?;
    s.edit(|p| *p = next);
    s.fix_selection();
    Ok(())
}

fn apply_to(p: &mut Project, action: &Action) -> Result<(), String> {
    let track_id = |p: &Project, name: &str| {
        p.tracks
            .iter()
            .find(|t| t.name == *name)
            .map(|t| t.id)
            .ok_or_else(|| format!("no track “{name}”"))
    };
    match action {
        Action::SetTempo { bpm } => p.tempo_bpm = bpm.clamp(40.0, 240.0),
        Action::SetKeyScale { key, scale } => {
            p.key = *key;
            p.scale = *scale;
        }
        Action::CreateClip {
            track_name,
            clip_name,
            length_steps,
            notes,
        } => {
            let t = track_id(p, track_name)?;
            let mut clip = Clip::new(clip_name.clone(), *length_steps);
            clip.notes = notes.iter().map(|n| n.to_note()).collect();
            clip.notes.sort_by_key(|n| n.step);
            p.add_clip(t, clip);
        }
        Action::ReplaceClipNotes {
            track_name,
            clip_name,
            notes,
        } => {
            let t = track_id(p, track_name)?;
            let track = p.track_mut(t).ok_or("track vanished")?;
            let clip = track
                .clips
                .iter_mut()
                .find(|c| c.name == *clip_name)
                .ok_or_else(|| format!("no clip “{clip_name}”"))?;
            clip.notes = notes.iter().map(|n| n.to_note()).collect();
            clip.notes.sort_by_key(|n| n.step);
        }
        Action::AddTrack { name, role } => {
            p.add_track(name.clone(), *role);
        }
        Action::AssignClip {
            section,
            track_name,
            clip_name,
        } => {
            let t = track_id(p, track_name)?;
            let clip = p
                .track(t)
                .and_then(|tr| tr.clips.iter().find(|c| c.name == *clip_name))
                .map(|c| c.id)
                .ok_or_else(|| format!("no clip “{clip_name}”"))?;
            let sec = p
                .sections
                .iter()
                .find(|s| s.name == *section)
                .map(|s| s.id)
                .ok_or_else(|| format!("no section “{section}”"))?;
            p.set_slot(sec, t, Some(clip));
        }
        Action::SetCurve {
            track_name,
            param,
            target,
            points,
        } => {
            let t = track_id(p, track_name)?;
            let track = p.track_mut(t).ok_or("track vanished")?;
            track
                .adaptive
                .curves
                .retain(|c| !(c.param == *param && c.target == *target));
            track
                .adaptive
                .curves
                .push(ResponseCurve::new(param.clone(), *target, points));
        }
        Action::SetInfluence { track_name, value } => {
            let t = track_id(p, track_name)?;
            if let Some(track) = p.track_mut(t) {
                track.adaptive.influence = value.clamp(0.0, 1.0);
            }
        }
        Action::SetMixer {
            track_name,
            volume_db,
            pan,
            reverb_send,
        } => {
            let t = track_id(p, track_name)?;
            if let Some(track) = p.track_mut(t) {
                if let Some(v) = volume_db {
                    track.mixer.volume_db = v.clamp(-60.0, 6.0);
                }
                if let Some(v) = pan {
                    track.mixer.pan = v.clamp(-1.0, 1.0);
                }
                if let Some(v) = reverb_send {
                    track.mixer.reverb_send = v.clamp(0.0, 1.0);
                }
            }
        }
        Action::Explain { .. } => {}
    }
    Ok(())
}

// ---------------------------------------------------------------------------------- export

pub struct EngineStems {
    pub base: LoadedProject,
}

impl aa_export::StemRenderer for EngineStems {
    fn render(
        &mut self,
        project: &Project,
        section: aa_core::SectionId,
        track: aa_core::TrackId,
        bars: u32,
        sample_rate: u32,
    ) -> Vec<f32> {
        let base = aa_engine::offline::with_project(&self.base, project.clone());
        aa_engine::offline::render_bars(&base, section, track, bars, sample_rate as f32, true)
    }
}

pub fn puzzle_vectors(project: &Project) -> serde_json::Value {
    let puzzles: Vec<serde_json::Value> = project
        .puzzles
        .iter()
        .map(|z| serde_json::json!({ "puzzle_id": z.id, "name": z.name, "kind": z.kind.name(), "vectors": aa_puzzle::test_vectors(z, project.tempo_bpm) }))
        .collect();
    serde_json::json!({ "vectors_version": 1, "tempo_bpm": project.tempo_bpm, "puzzles": puzzles })
}

pub fn start_export(s: &mut App, dir: PathBuf) {
    s.ext.export_busy = true;
    let project = s.project.clone();
    let base = s.loaded.share();
    let (tx, rx) = channel();
    s.ext.export_rx = Some(rx);
    s.toast("Exporting stems for FMOD…");
    std::thread::spawn(move || {
        let mut renderer = EngineStems { base };
        let vectors = puzzle_vectors(&project);
        let options = aa_export::ExportOptions::default();
        let result =
            aa_export::export_scene(&project, &dir, &mut renderer, Some(vectors), &options)
                .map(|r| {
                    format!(
                        "Exported {} stems to {} ({} notes in README)",
                        r.stems_written,
                        r.scene_dir.display(),
                        r.warnings.len()
                    )
                })
                .map_err(|e| e.to_string());
        let _ = tx.send(result);
    });
}
