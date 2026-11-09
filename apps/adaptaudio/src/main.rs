//! Adaptaudio: compose, prototype and export the adaptive music of Cedar Station.
//!
//! `cargo run -p adaptaudio` opens the window. Headless: `-- --snap shot.png --scene chase`.

mod actions;
mod ext;
mod glossary;
mod live;
mod screens;
mod settings;
mod shell;
mod state;
mod theme;
mod widgets;

#[cfg(test)]
mod tests;

use rune::{AppSpec, Cx, El, KeyEvent, Tx};
use state::{App, Screen};

/// Headless runs (snapshots, trees) don't open audio devices.
fn headless() -> bool {
    std::env::args().any(|a| {
        matches!(
            a.as_str(),
            "--snap"
                | "--trace"
                | "--tree"
                | "--frames"
                | "--strip"
                | "--first-pixels"
                | "--list-scenes"
        )
    })
}

fn init() -> App {
    let settings = settings::Settings::load();
    let (project, path) = settings
        .last_project
        .clone()
        .filter(|_| !headless())
        .and_then(|p| aa_core::io::load(&p).ok().map(|proj| (proj, Some(p))))
        .unwrap_or_else(|| (aa_core::demo::cedar_station(), None));
    let mut app = App::new(project, path, settings, !headless());
    app.metronome = app.settings.metronome;
    actions::after_project_change(&mut app);
    app
}

fn setup(s: &mut App, wake: rune::Waker) {
    s.live.set_waker(wake);
    if !headless() {
        s.live.start();
    }
}

fn poll(s: &mut App, t: f64, tx: &mut Tx) -> bool {
    s.now = t;
    s.run_offline(t);
    let mut changed = s.sync_from_engine();
    changed |= actions::tick(s, t, tx);
    if s.offline.is_some() {
        s.live.tick();
        if s.playing || s.scenario.is_some() {
            tx.wake_after(std::time::Duration::from_millis(16));
        }
    }
    changed
}

fn view(s: &App, cx: &mut Cx<App>) -> El {
    shell::view(s, cx)
}

fn on_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    actions::on_key(s, ev, tx)
}

fn play_mode(s: &mut App, mode: &str, intensity: f32, tension: f32) {
    if let Some(m) = s.project.mode_index(mode) {
        s.request_mode(m);
    }
    for (name, v) in [("Intensity", intensity), ("Tension", tension)] {
        if let Some(i) = s.project.param_index(name) {
            s.send(aa_engine::Command::JumpParam { index: i, value: v });
        }
    }
    s.toggle_play();
}

type Scene = (&'static str, fn(&mut App));

const SCENES: &[Scene] = &[
    ("chase", |s| play_mode(s, "Chase", 0.95, 0.85)),
    ("explore", |s| play_mode(s, "Explore", 0.1, 0.15)),
    ("pattern", |s| s.screen = Screen::Pattern),
    ("sound", |s| s.screen = Screen::Sound),
    ("mixer", |s| {
        play_mode(s, "Chase", 1.0, 0.9);
        s.screen = Screen::Mixer;
    }),
    ("adaptive", |s| s.screen = Screen::Adaptive),
    ("simulator", |s| {
        play_mode(s, "Chase", 0.8, 0.7);
        s.screen = Screen::Simulator;
    }),
    ("puzzle", |s| s.screen = Screen::Puzzle),
    ("learn", |s| s.screen = Screen::Learn),
    ("settings", |s| s.screen = Screen::Settings),
    ("capture", |s| s.screen = Screen::Capture),
    ("assistant", |s| s.assistant_open = true),
];

fn main() {
    rune::run(AppSpec::<App> {
        min_size: Some((1100.0, 700.0)),
        scenes: SCENES,
        families: ("Inter", "ui-monospace"),
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        ..AppSpec::new("Adaptaudio", (1480.0, 920.0), init, view)
    });
}
