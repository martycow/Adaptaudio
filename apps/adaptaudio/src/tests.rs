//! Headless UI tests: real views, real clicks, the engine running offline.

use crate::settings::Settings;
use crate::state::{App, Screen};
use rune::{Env, Runtime};

fn app() -> Runtime<App> {
    let state = App::new(
        aa_core::demo::cedar_station(),
        None,
        Settings::default(),
        false,
    );
    let mut rt = Runtime::new(
        state,
        crate::view,
        (1480.0, 920.0),
        Env {
            system_dark: true,
            scale: 1.0,
        },
    );
    rt.poll = Some(crate::poll);
    rt.frame(0.0);
    rt
}

fn click(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_down(x, y);
    rt.frame(1.0 / 60.0);
    rt.pointer_up(x, y);
    rt.frame(1.0 / 60.0);
}

fn run(rt: &mut Runtime<App>, seconds: f64) {
    for _ in 0..(seconds * 60.0) as usize {
        rt.frame(1.0 / 60.0);
    }
}

#[test]
fn a_mode_pill_switches_the_music() {
    let mut rt = app();
    let chase = rt.state.project.mode_index("Chase").unwrap();
    click(&mut rt, &format!("mode-{chase}"));
    run(&mut rt, 0.3);
    assert_eq!(rt.state.mode, Some(chase));
}

#[test]
fn play_moves_the_transport() {
    let mut rt = app();
    click(&mut rt, "play");
    run(&mut rt, 1.0);
    assert!(rt.state.playing);
    assert!(
        rt.state.controller.telemetry.position.get() > 4.0,
        "a second at 120 BPM is 8 steps"
    );
    click(&mut rt, "play");
    run(&mut rt, 0.1);
    assert!(!rt.state.playing);
}

#[test]
fn an_empty_cell_makes_a_clip_and_undo_takes_it_back() {
    let mut rt = app();
    let kick = rt
        .state
        .project
        .tracks
        .iter()
        .find(|t| t.name == "Kick")
        .unwrap()
        .id;
    let explore = rt
        .state
        .project
        .sections
        .iter()
        .find(|s| s.name == "Explore")
        .unwrap()
        .id;
    let before = rt.state.project.track(kick).unwrap().clips.len();
    click(&mut rt, &format!("cell-{kick}-{explore}"));
    assert_eq!(
        rt.state.project.track(kick).unwrap().clips.len(),
        before + 1
    );
    assert!(
        rt.state
            .project
            .section(explore)
            .unwrap()
            .clip_for(kick)
            .is_some()
    );
    rt.state.undo();
    assert_eq!(rt.state.project.track(kick).unwrap().clips.len(), before);
}

#[test]
fn clicking_the_piano_roll_adds_a_note_in_the_scale() {
    let mut rt = app();
    let lead = rt
        .state
        .project
        .tracks
        .iter()
        .find(|t| t.name == "Lead")
        .unwrap();
    let (tid, cid) = (lead.id, lead.clips[0].id);
    rt.update(|s, _| {
        s.select_clip(tid, Some(cid));
        s.screen = Screen::Pattern;
    });
    run(&mut rt, 1.0);
    let before = rt.state.project.clip(tid, cid).unwrap().notes.len();
    // Step 2 is empty in the demo's lead line; D5 (74) is in D minor.
    click(&mut rt, "c-74-1");
    let clip = rt.state.project.clip(tid, cid).unwrap();
    assert_eq!(clip.notes.len(), before + 1);
    let p = &rt.state.project;
    let added = clip
        .notes
        .iter()
        .find(|n| n.step == 1)
        .expect("note at step 2");
    assert!(p.scale.contains(p.key, added.pitch));
}

#[test]
fn every_screen_and_panel_renders() {
    let mut rt = app();
    rt.update(|s, _| s.assistant_open = true);
    for screen in Screen::ALL {
        rt.update(|s, _| s.screen = screen);
        rt.frame(1.0 / 60.0);
        assert!(rt.find("content").is_some(), "{screen:?}");
    }
}

#[test]
fn an_assistant_proposal_applies_as_one_undo_step() {
    let mut rt = app();
    let action = aa_assistant::Action::SetTempo { bpm: 132.0 };
    crate::ext::apply_action(&mut rt.state, &action).unwrap();
    assert_eq!(rt.state.project.tempo_bpm, 132.0);
    rt.state.undo();
    assert_eq!(rt.state.project.tempo_bpm, 120.0);
}

#[test]
fn the_simulator_pad_sets_intensity_and_tension() {
    let mut rt = app();
    rt.update(|s, _| s.screen = Screen::Simulator);
    run(&mut rt, 1.0);
    let (x, y) = rt.find("pad-big").unwrap();
    // Centre of the pad is (0.5, 0.5); press right of and above it.
    rt.pointer_down(x + 100.0, y - 100.0);
    rt.frame(1.0 / 60.0);
    rt.pointer_move(x + 110.0, y - 110.0);
    rt.frame(1.0 / 60.0);
    rt.pointer_up(x + 110.0, y - 110.0);
    run(&mut rt, 0.1);
    let p = &rt.state.project;
    let t = &rt.state.controller.telemetry;
    let (xi, yi) = (
        p.param_index("Intensity").unwrap(),
        p.param_index("Tension").unwrap(),
    );
    assert!(
        t.param_targets[xi].get() > 0.6,
        "{}",
        t.param_targets[xi].get()
    );
    assert!(
        t.param_targets[yi].get() > 0.6,
        "{}",
        t.param_targets[yi].get()
    );
}

#[test]
fn export_writes_stems_and_a_valid_manifest() {
    let mut project = aa_core::demo::cedar_station();
    // Keep it quick: two sections, one bar each.
    let keep: Vec<u32> = project
        .sections
        .iter()
        .filter(|s| s.name == "Explore" || s.name == "Chase")
        .map(|s| s.id)
        .collect();
    project.sections.retain(|s| keep.contains(&s.id));
    for s in &mut project.sections {
        s.length_bars = 1;
    }
    for m in &mut project.game_modes {
        if m.section.is_some_and(|id| !keep.contains(&id)) {
            m.section = None;
        }
    }
    let base = aa_engine::LoadedProject::without_samples(project.clone());
    let dir = std::env::temp_dir().join(format!("aa-app-export-{}", std::process::id()));
    let mut renderer = crate::ext::EngineStems { base };
    let vectors = crate::ext::puzzle_vectors(&project);
    let report = aa_export::export_scene(
        &project,
        &dir,
        &mut renderer,
        Some(vectors),
        &aa_export::ExportOptions::default(),
    )
    .unwrap();
    assert!(report.stems_written > 5);
    assert!(aa_export::validate_manifest(&report.manifest).is_empty());
    let vectors = std::fs::read_to_string(report.scene_dir.join("puzzle_vectors.json")).unwrap();
    assert!(vectors.contains("Cedar door knocks"));
    let _ = std::fs::remove_dir_all(&dir);
}
