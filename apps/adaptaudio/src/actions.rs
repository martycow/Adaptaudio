//! What buttons and keys do: transport, files, export, scenarios, timers.

use crate::state::{App, ScenarioRun, Screen};
use aa_core::{PitchClass, ScaleKind, ScenarioAction};
use aa_engine::Command;
use rune::file_dialog::FileDialogOptions;
use rune::{Key, KeyEvent, Tx};
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Keeps live values and engine-side helpers in step with the project.
pub fn after_project_change(s: &mut App) {
    let p = &s.project;
    if let Ok(mut lens) = s.live.clip_len.lock() {
        for (i, t) in p.tracks.iter().enumerate().take(aa_engine::MAX_TRACKS) {
            let playing = s.clip_playing.get(i).copied().unwrap_or(0);
            let len = t
                .clip(playing)
                .or_else(|| t.clips.first())
                .map_or(16, |c| c.length_steps);
            lens[i] = f32::from(len);
        }
    }
    s.live
        .x_param
        .store(p.param_index("Intensity").unwrap_or(0), Ordering::Relaxed);
    s.live
        .y_param
        .store(p.param_index("Tension").unwrap_or(1), Ordering::Relaxed);
}

/// Runs every frame. True when the view should rebuild.
pub fn tick(s: &mut App, now: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    after_project_change(s);
    changed |= run_scenario(s, now, tx);
    changed |= crate::ext::poll(s, now, tx);
    let before = s.toasts.len();
    s.toasts.retain(|t| now - t.born < 5.0);
    if s.toasts.len() != before {
        changed = true;
    }
    if !s.toasts.is_empty() {
        tx.wake_after(Duration::from_millis(500));
    }
    if s.dirty
        && s.path.is_some()
        && s.settings.autosave_seconds > 0
        && now - s.last_autosave > f64::from(s.settings.autosave_seconds)
    {
        s.last_autosave = now;
        if let Some(path) = s.path.clone() {
            match aa_core::io::save(&s.project, &path) {
                Ok(_) => {
                    s.dirty = false;
                    changed = true;
                }
                Err(e) => s.toast_error(&format!("Autosave failed: {e}")),
            }
        }
    }
    changed
}

pub fn start_scenario(s: &mut App, index: usize) {
    if index >= s.project.scenarios.len() {
        return;
    }
    s.send(Command::ResetAdaptive);
    if !s.playing {
        s.set_playing(true);
    }
    s.scenario = Some(ScenarioRun {
        index,
        started: s.now,
        next_step: 0,
    });
    let name = s.project.scenarios[index].name.clone();
    s.log_line(format!("▶ scenario “{name}”"), None);
}

pub fn stop_scenario(s: &mut App) {
    s.scenario = None;
    s.live.scenario.set([0.0, 0.0]);
}

fn run_scenario(s: &mut App, now: f64, tx: &mut Tx) -> bool {
    let Some(run) = s.scenario.clone() else {
        return false;
    };
    let Some(scenario) = s.project.scenarios.get(run.index).cloned() else {
        s.scenario = None;
        return true;
    };
    let elapsed = (now - run.started) as f32;
    let mut steps = scenario.steps.clone();
    steps.sort_by(|a, b| a.at_seconds.total_cmp(&b.at_seconds));
    let mut next = run.next_step;
    let mut changed = false;
    while let Some(step) = steps.get(next).filter(|st| st.at_seconds <= elapsed) {
        match &step.action {
            ScenarioAction::SetMode(m) => {
                if let Some(i) = s.project.mode_index(m) {
                    s.send(Command::RequestMode(i));
                    s.pending_mode = Some(i);
                    s.log_line(
                        format!("→ {m}"),
                        s.project.game_modes.get(i).map(|g| g.color.clone()),
                    );
                }
            }
            ScenarioAction::SetParam {
                name,
                value,
                over_seconds,
            } => {
                if let Some(i) = s.project.param_index(name) {
                    s.send(Command::RampParam {
                        index: i,
                        value: *value,
                        seconds: *over_seconds,
                    });
                    s.live.set_param_target(i, *value);
                }
            }
            ScenarioAction::Trigger(e) => {
                if let Some(i) = s.project.event_index(e) {
                    s.trigger_event(i);
                }
            }
        }
        next += 1;
        changed = true;
    }
    let duration = scenario.duration().max(0.1);
    s.live
        .scenario
        .set([0.0, (elapsed / duration).clamp(0.0, 1.0)]);
    if let Some(r) = s.scenario.as_mut() {
        r.next_step = next;
    }
    if next >= steps.len() && elapsed > duration + 2.0 {
        s.scenario = None;
        s.log_line("■ scenario done".into(), None);
        changed = true;
    } else {
        tx.wake_after(Duration::from_millis(40));
    }
    changed
}

pub fn toggle_metronome(s: &mut App) {
    s.metronome = !s.metronome;
    let on = s.metronome;
    s.send(Command::SetMetronome(on));
    s.settings.metronome = on;
    let _ = s.settings.save();
}

pub fn set_scale(s: &mut App, index: usize) {
    let Some(next) = ScaleKind::ALL.get(index).copied() else {
        return;
    };
    s.edit(|p| p.scale = next);
    s.toast(&format!("Scale: {} — {}", next.name(), next.mood()));
}

pub fn set_key(s: &mut App, index: usize) {
    let next = PitchClass::new(index as i32);
    s.edit(|p| p.key = next);
}

pub fn set_tempo(s: &mut App, bpm: f32, starting: bool) {
    let bpm = bpm.clamp(40.0, 240.0).round();
    if starting {
        s.begin_gesture();
    }
    s.edit_quiet(|p| p.tempo_bpm = bpm);
}

// ---------------------------------------------------------------------------------- files

pub fn save(s: &mut App, tx: &mut Tx) {
    match s.path.clone() {
        Some(path) => write(s, &path),
        None => save_as(s, tx),
    }
}

/// A file name made from the project's name.
fn file_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let stem = if clean.is_empty() { "Project" } else { &clean };
    format!("{stem}.{}", aa_core::io::EXTENSION)
}

pub fn save_as(s: &mut App, tx: &mut Tx) {
    let options = FileDialogOptions {
        title: "Save project".into(),
        file_name: Some(file_name(&s.project.name)),
        ..Default::default()
    };
    tx.save_file(options, |s: &mut App, result, _| match result {
        Ok(Some(path)) => {
            let path = aa_core::io::project_dir(&path);
            write(s, &path);
        }
        Ok(None) => {}
        Err(e) => s.toast_error(&e.0),
    });
}

fn write(s: &mut App, path: &std::path::Path) {
    match aa_core::io::save(&s.project, path) {
        Ok(dir) => {
            s.path = Some(dir.clone());
            s.dirty = false;
            s.settings.remember_project(&dir);
            let _ = s.settings.save();
            s.toast(&format!("Saved {}", dir.display()));
        }
        Err(e) => s.toast_error(&format!("Save failed: {e}")),
    }
}

pub fn open_dialog(_: &mut App, tx: &mut Tx) {
    let options = FileDialogOptions {
        title: "Open an .aaproj folder".into(),
        ..Default::default()
    };
    tx.open_folder(options, |s: &mut App, result, _| match result {
        Ok(Some(path)) => open(s, &path),
        Ok(None) => {}
        Err(e) => s.toast_error(&e.0),
    });
}

pub fn open(s: &mut App, path: &std::path::Path) {
    match aa_core::io::load(path) {
        Ok(project) => {
            let dir = aa_core::io::project_dir(path);
            s.replace_project(project, Some(dir.clone()));
            s.settings.remember_project(&dir);
            let _ = s.settings.save();
            s.wizard = None;
            s.toast("Project opened");
        }
        Err(e) => s.toast_error(&format!("Can't open: {e}")),
    }
}

pub fn new_demo(s: &mut App) {
    s.replace_project(aa_core::demo::cedar_station(), None);
    s.dirty = true;
    s.toast("Started from the Cedar Station demo");
}

/// File → New: the project chooser on its "new project" page.
pub fn new_project(s: &mut App, _: &mut Tx) {
    s.open_menu = None;
    s.wizard = Some(crate::screens::wizard::Wizard::new_project());
}

/// Makes the project the wizard describes, then asks where to keep it.
pub fn create_from_wizard(s: &mut App, tx: &mut Tx) {
    let Some(w) = s.wizard.take() else {
        return;
    };
    s.replace_project(w.build(), None);
    s.dirty = true;
    s.screen = Screen::Session;
    save_as(s, tx);
}

pub fn export_dialog(s: &mut App, tx: &mut Tx) {
    if s.ext.export_busy {
        return;
    }
    let options = FileDialogOptions {
        title: "Export for FMOD: choose a folder".into(),
        directory: s.settings.export_dir.clone(),
        ..Default::default()
    };
    tx.open_folder(options, |s: &mut App, result, _| match result {
        Ok(Some(dir)) => {
            s.settings.export_dir = Some(dir.clone());
            let _ = s.settings.save();
            crate::ext::start_export(s, dir);
        }
        Ok(None) => {}
        Err(e) => s.toast_error(&e.0),
    });
}

// ---------------------------------------------------------------------------------- keys

pub fn on_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if tx
        .focused()
        .is_some_and(|f| f.contains("field") || f.contains("input"))
    {
        return false;
    }
    let m = ev.mods;
    // While the project chooser is up, only file shortcuts work.
    if s.wizard.is_some() && !matches!(&ev.key, Key::Command('o') | Key::Command('n')) {
        return false;
    }
    match &ev.key {
        Key::Text(t) if t == " " && !m.cmd => {
            s.toggle_play();
            true
        }
        Key::Text(t) if !m.cmd && t.len() == 1 && t.chars().all(|c| c.is_ascii_digit()) => {
            let n = t.parse::<usize>().unwrap_or(0);
            if (1..=s.project.game_modes.len()).contains(&n) {
                s.request_mode(n - 1);
                true
            } else {
                false
            }
        }
        Key::Command('z') if m.shift => {
            s.redo();
            true
        }
        Key::Command('z') | Key::Command('Z') if m.shift => {
            s.redo();
            true
        }
        Key::Command('z') => {
            s.undo();
            true
        }
        Key::Command('s') => {
            if m.shift {
                save_as(s, tx)
            } else {
                save(s, tx)
            }
            true
        }
        Key::Command('n') => {
            new_project(s, tx);
            true
        }
        Key::Command('o') => {
            open_dialog(s, tx);
            true
        }
        Key::Command('e') => {
            export_dialog(s, tx);
            true
        }
        Key::Command('/') => {
            s.assistant_open = !s.assistant_open;
            true
        }
        Key::Command(c) if c.is_ascii_digit() => {
            let n = c.to_digit(10).unwrap_or(0) as usize;
            if let Some(screen) = n.checked_sub(1).and_then(|i| Screen::ALL.get(i)) {
                s.screen = *screen;
            }
            true
        }
        Key::Escape => {
            if s.open_menu.take().is_some() {
                true
            } else if s.selected_clip.is_some() && s.screen == Screen::Session {
                s.selected_clip = None;
                s.selected_note = None;
                true
            } else if s.assistant_open {
                s.assistant_open = false;
                true
            } else if s.scenario.is_some() {
                stop_scenario(s);
                true
            } else {
                false
            }
        }
        _ => false,
    }
}
