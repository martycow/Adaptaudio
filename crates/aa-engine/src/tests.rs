use crate::engine::{Command, Engine, Notice};
use crate::offline::{render_oneshot, render_scenario, render_stem};
use crate::samples::LoadedProject;
use aa_core::demo::cedar_station;
use std::sync::Arc;

const SR: f32 = 48_000.0;

fn demo() -> LoadedProject {
    LoadedProject::without_samples(cedar_station())
}

fn peak(buf: &[f32]) -> f32 {
    buf.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn rms(buf: &[f32]) -> f32 {
    (buf.iter().map(|s| s * s).sum::<f32>() / buf.len().max(1) as f32).sqrt()
}

#[test]
fn every_mode_sounds_clean() {
    let base = demo();
    for (m, mode) in base.project.game_modes.iter().enumerate() {
        let (mut engine, _c) = Engine::new(Arc::new(base.share()), SR);
        engine.apply(Command::RequestMode(m));
        for (name, value) in &mode.param_presets {
            if let Some(i) = base.project.param_index(name) {
                engine.apply(Command::JumpParam {
                    index: i,
                    value: *value,
                });
            }
        }
        engine.apply(Command::Play);
        let out = engine.render_offline(4.0);
        assert!(out.iter().all(|s| s.is_finite()), "{}: NaN", mode.name);
        assert!(peak(&out) <= 1.0, "{}: clips", mode.name);
        assert!(rms(&out) > 1e-4, "{} is silent", mode.name);
    }
}

#[test]
fn chase_is_louder_than_stealth() {
    let base = demo();
    let level = |mode: &str, intensity: f32| {
        let (mut engine, _c) = Engine::new(Arc::new(base.share()), SR);
        engine.apply(Command::RequestMode(base.project.mode_index(mode).unwrap()));
        engine.apply(Command::JumpParam {
            index: base.project.param_index("Intensity").unwrap(),
            value: intensity,
        });
        engine.apply(Command::Play);
        rms(&engine.render_offline(4.0))
    };
    assert!(level("Chase", 1.0) > 2.0 * level("Stealth", 0.2));
}

#[test]
fn a_mode_change_lands_on_its_grid() {
    let base = demo();
    let p = &base.project;
    let (mut engine, mut controller) = Engine::new(Arc::new(base.share()), SR);
    engine.apply(Command::RequestMode(p.mode_index("Stealth").unwrap()));
    engine.apply(Command::Play);
    engine.render_offline(2.1); // past the dwell, mid-beat
    let alert = p.mode_index("Alert").unwrap();
    engine.apply(Command::RequestMode(alert));
    let asked_at = engine.position();
    assert_ne!(engine.adaptive().mode(), Some(alert));
    // Stealth → Alert is beat-quantized: within one beat (4 steps) it must have switched.
    let sps = f64::from(SR) * p.seconds_per_step();
    let mut switched_at = None;
    for _ in 0..2_000 {
        let mut buf = vec![0.0; 128];
        engine.process(&mut buf, 2);
        if engine.adaptive().mode() == Some(alert) {
            switched_at = Some(engine.position());
            break;
        }
    }
    let at = switched_at.expect("switched");
    assert!(at - asked_at <= 4.0 + 64.0 / sps, "{asked_at} → {at}");
    let notices = controller.drain_notices();
    assert!(notices.contains(&Notice::ModeSwitched { mode: alert }));
    let detected = p.event_index("Detected").unwrap();
    assert!(notices.contains(&Notice::EventFired { event: detected }));
}

#[test]
fn the_spotted_scenario_ends_in_chase() {
    let base = demo();
    let scenario = base
        .project
        .scenarios
        .iter()
        .find(|s| s.name == "Spotted!")
        .unwrap()
        .clone();
    let (out, engine) = render_scenario(&base, &scenario, 16.0, SR);
    assert!(out.iter().all(|s| s.is_finite()));
    assert_eq!(engine.adaptive().mode(), base.project.mode_index("Chase"));
}

#[test]
fn stems_are_exactly_their_length_and_not_silent() {
    let base = demo();
    let p = &base.project;
    let chase = p.sections.iter().find(|s| s.name == "Chase").unwrap();
    let bass = p.tracks.iter().find(|t| t.name == "Bass").unwrap();
    let stem = render_stem(&base, chase.id, bass.id, 4, SR);
    let expected = (4.0 * f64::from(p.steps_per_bar()) * p.seconds_per_step() * f64::from(SR))
        .round() as usize
        * 2;
    assert_eq!(stem.len(), expected);
    assert!(rms(&stem) > 1e-3);
    // The kick is muted in a bass stem.
    let kick = p.tracks.iter().find(|t| t.name == "Kick").unwrap();
    let explore = p.sections.iter().find(|s| s.name == "Explore").unwrap();
    let empty = render_stem(&base, explore.id, kick.id, 1, SR);
    assert!(peak(&empty) < 1e-3, "Explore has no kick clip");
}

#[test]
fn stingers_render_with_their_tail() {
    let base = demo();
    let p = &base.project;
    let st = p
        .events
        .iter()
        .find(|e| e.name == "Detected")
        .and_then(|e| e.stinger)
        .unwrap();
    let out = render_oneshot(&base, st.track, st.clip, 2.0, SR);
    let len_steps = p.clip(st.track, st.clip).unwrap().length_steps;
    let expected =
        ((f64::from(len_steps) * p.seconds_per_step() + 2.0) * f64::from(SR)).round() as usize * 2;
    assert_eq!(out.len(), expected);
    assert!(rms(&out) > 1e-3);
}

#[test]
fn the_kick_ducks_the_pad() {
    let mut project = cedar_station();
    for t in &mut project.tracks {
        t.mixer.mute = !(t.name == "Pad" || t.name == "Kick");
        t.adaptive.influence = 0.0;
    }
    let chase = project.mode_index("Chase").unwrap();
    let base = LoadedProject::without_samples(project);
    let (mut engine, controller) = Engine::new(Arc::new(base.share()), SR);
    engine.apply(Command::RequestMode(chase));
    engine.apply(Command::Play);
    engine.render_offline(1.0);
    let mut seen = 0.0f32;
    for _ in 0..100 {
        let mut b = vec![0.0; 256];
        engine.process(&mut b, 2);
        seen = seen.max(controller.telemetry.sidechain.get());
    }
    assert!(seen > 0.5, "sidechain never engaged: {seen}");
}

#[test]
fn replacing_the_project_hands_the_old_one_back() {
    let base = demo();
    let (mut engine, mut controller) = Engine::new(Arc::new(base.share()), SR);
    engine.apply(Command::Play);
    engine.render_offline(0.1);
    let mut edited = base.project.clone();
    edited.tempo_bpm = 140.0;
    let next = Arc::new(crate::offline::with_project(&base, edited));
    let weak = Arc::downgrade(&next);
    assert!(controller.send(Command::SetProject(next)));
    let mut buf = vec![0.0; 512];
    engine.process(&mut buf, 2);
    controller.collect_garbage();
    assert!(weak.upgrade().is_some(), "the new project is in use");
    assert!((controller.telemetry.bpm.get() - 140.0).abs() < 1e-3);
}

#[test]
fn locked_layers_open_when_their_event_fires() {
    let base = demo();
    let p = &base.project;
    let bells = p
        .track_index(p.tracks.iter().find(|t| t.name == "Bells").unwrap().id)
        .unwrap();
    let (mut engine, controller) = Engine::new(Arc::new(base.share()), SR);
    engine.apply(Command::RequestMode(p.mode_index("Puzzle").unwrap()));
    engine.apply(Command::JumpParam {
        index: p.param_index("PuzzleProgress").unwrap(),
        value: 1.0,
    });
    engine.apply(Command::Play);
    engine.render_offline(3.0);
    assert!(controller.telemetry.tracks[bells].level.get() < 1e-4);
    engine.apply(Command::TriggerEvent(
        p.event_index("PuzzleSolved").unwrap(),
    ));
    engine.render_offline(4.0);
    assert!(controller.telemetry.tracks[bells].gain_mod.get() > 0.9);
}
