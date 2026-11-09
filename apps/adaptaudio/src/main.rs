//! Adaptaudio: compose, prototype and export the adaptive music of Cedar Station.
//!
//! `cargo run -p adaptaudio` opens the window. Headless: `-- --snap shot.png --scene chase`.

mod actions;
mod devices;
mod ext;
mod glossary;
mod library;
mod live;
mod screens;
mod settings;
mod shell;
mod state;
mod theme;
mod update;
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
    let ask = settings.ask_project_on_start;
    // Without the project chooser the last project opens by itself.
    let (project, path) = settings
        .last_project
        .clone()
        .filter(|_| !headless() && !ask)
        .and_then(|p| aa_core::io::load(&p).ok().map(|proj| (proj, Some(p))))
        .unwrap_or_else(|| (aa_core::demo::cedar_station(), None));
    let mut app = App::new(project, path, settings, !headless());
    app.metronome = app.settings.metronome;
    if ask && !headless() {
        app.wizard = Some(screens::wizard::Wizard::start());
    }
    actions::after_project_change(&mut app);
    app
}

fn setup(s: &mut App, wake: rune::Waker) {
    s.live.set_waker(wake);
    if !headless() {
        s.live.start();
        let wake = s.live.waker_fn();
        s.ext.devices.start(wake);
        if s.settings.check_updates_on_start {
            screens::settings::check_updates(s, true);
        }
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
    ("editor", |s| {
        let pick = s
            .project
            .tracks
            .iter()
            .find(|t| t.name == "Lead")
            .and_then(|t| t.clips.first().map(|c| (t.id, c.id)));
        if let Some((t, c)) = pick {
            s.select_clip(t, Some(c));
        }
    }),
    ("sound", |s| s.screen = Screen::Sound),
    ("mixer", |s| {
        play_mode(s, "Chase", 1.0, 0.9);
        s.screen = Screen::Mixer;
    }),
    ("adaptive", |s| s.screen = Screen::Adaptive),
    ("adaptive-events", |s| {
        s.screen = Screen::Adaptive;
        s.tabs.adaptive = 1;
    }),
    ("adaptive-transitions", |s| {
        s.screen = Screen::Adaptive;
        s.tabs.adaptive = 2;
    }),
    ("adaptive-curves", |s| {
        s.screen = Screen::Adaptive;
        s.tabs.adaptive = 3;
    }),
    ("playtest", |s| {
        play_mode(s, "Chase", 0.8, 0.7);
        s.screen = Screen::Playtest;
    }),
    ("learn", |s| s.screen = Screen::Learn),
    ("learn-moods", |s| {
        s.screen = Screen::Learn;
        s.tabs.learn = 1;
    }),
    ("settings", |s| s.screen = Screen::Settings),
    ("capture", |s| s.screen = Screen::Capture),
    ("assistant", |s| s.assistant_open = true),
    ("wizard", |s| {
        s.wizard = Some(screens::wizard::Wizard::start())
    }),
    ("wizard-new", |s| {
        s.wizard = Some(screens::wizard::Wizard::new_project())
    }),
    ("scale-menu", |s| s.open_menu = Some("scale".into())),
    ("game-panel", |s| s.sim_open = true),
];

/// The menu bar: every file command, the transport and the screens.
fn menu() -> Vec<rune::MenuItem<App>> {
    use rune::MenuItem;
    let item = |menu: &str, title: &str, keys: &str, action: fn(&mut App, &mut Tx)| {
        let mut m = MenuItem::new(title, action)
            .id(format!("{menu}/{title}"))
            .menu(menu)
            .keys_first();
        if let Ok(shortcut) = keys.parse::<rune::Shortcut>() {
            m = m.key_equivalent(shortcut);
        }
        m
    };
    vec![
        item("File", "New Project…", "mod+n", actions::new_project),
        item("File", "Open…", "mod+o", actions::open_dialog),
        MenuItem::separator().menu("File"),
        item("File", "Save", "mod+s", actions::save),
        item("File", "Save As…", "mod+shift+s", actions::save_as),
        MenuItem::separator().menu("File"),
        item("File", "Export to FMOD…", "mod+e", actions::export_dialog),
        item("Transport", "Play / Stop", "", |s, _| s.toggle_play()),
        item("Transport", "Metronome", "", |s, _| {
            actions::toggle_metronome(s)
        }),
        item("Transport", "Undo", "", |s, _| s.undo()),
        item("Transport", "Redo", "", |s, _| s.redo()),
        item("View", "Session", "mod+1", |s, _| {
            s.screen = Screen::Session
        }),
        item("View", "Sound Lab", "mod+2", |s, _| {
            s.screen = Screen::Sound
        }),
        item("View", "Capture", "mod+3", |s, _| {
            s.screen = Screen::Capture
        }),
        item("View", "Mixer", "mod+4", |s, _| s.screen = Screen::Mixer),
        item("View", "Adaptive", "mod+5", |s, _| {
            s.screen = Screen::Adaptive
        }),
        item("View", "Playtest", "mod+6", |s, _| {
            s.screen = Screen::Playtest
        }),
        item("View", "Learn", "mod+7", |s, _| s.screen = Screen::Learn),
        item("View", "Settings", "mod+8", |s, _| {
            s.screen = Screen::Settings
        }),
        MenuItem::separator().menu("View"),
        item("View", "Game Controls", "", |s, _| s.sim_open = !s.sim_open),
        item("View", "Assistant", "mod+/", |s, _| {
            s.assistant_open = !s.assistant_open
        }),
        item("Help", "Check for Updates…", "", |s, _| {
            s.screen = Screen::Settings;
            s.tabs.settings = 5;
            screens::settings::check_updates(s, false);
        }),
    ]
}

fn main() {
    let spec = AppSpec::<App> {
        min_size: Some((1100.0, 700.0)),
        scenes: SCENES,
        families: ("Inter", "ui-monospace"),
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        ..AppSpec::new("Adaptaudio", (1480.0, 920.0), init, view)
    };
    // Scroll bars show whenever something overflows, whatever the system preference says,
    // unless the command line asks otherwise.
    let mut argv: Vec<String> = std::env::args().skip(1).collect();
    if !argv.iter().any(|a| a == "--scroll-bars") {
        argv.extend(["--scroll-bars".to_string(), "always".to_string()]);
    }
    let mut app = rune::App::new(spec).standard_menus().args(argv);
    for item in menu() {
        app = app.menu_item(item);
    }
    app.run();
}
