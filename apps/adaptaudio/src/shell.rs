//! The window frame: sidebar, project bar, transport bar, the screen, the game panel, the
//! assistant drawer, the project chooser and toasts.

use crate::state::{App, Screen};
use crate::theme::{self, font};
use crate::widgets::{dropdown, hscroll, pill, tool_button};
use crate::{actions, screens};
use aa_core::{PitchClass, ScaleKind};
use rune::{Cx, El, col, el, row, text};

pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let accent = s.accent();
    let content = col()
        .key("content")
        .grow(1.0)
        .h_full()
        .min_w(0.0)
        .min_h(0.0)
        .child(
            col()
                .key(format!("screen-{}", s.screen.name()))
                .w_full()
                .h_full()
                .min_h(0.0)
                .transition(rune::Transition::offset(0.0, 8.0))
                .child(screens::view(s, cx, accent)),
        );
    let mut body = row()
        .key("body")
        .grow(1.0)
        .w_full()
        .min_h(0.0)
        .child(content);
    if s.sim_open && s.screen != Screen::Playtest {
        body = body.child(screens::playtest::panel(s, accent));
    }
    if s.assistant_open {
        body = body.child(screens::assistant::drawer(s, accent));
    }
    let main = col()
        .key("main")
        .grow(1.0)
        .h_full()
        .min_w(0.0)
        .child(project_bar(s, accent))
        .child(transport_bar(s, accent))
        .child(body);
    let mut root = el().key("root").w_full().h_full().fill(theme::bg()).child(
        row()
            .w_full()
            .h_full()
            .child(sidebar(s, accent))
            .child(main),
    );
    // A soft pulse around the window edge on the beat, in the mode's colour.
    if !s.settings.calm_ui {
        root = root.child(
            el().key("glow")
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .box_shadow(rune::Shadow {
                    x: 0.0,
                    y: 0.0,
                    blur: 70.0,
                    spread: -24.0,
                    color: accent.alpha(0.4),
                    inset: true,
                })
                .inert()
                .live_opacity(&s.live.glow),
        );
    }
    if let Some(w) = &s.wizard {
        root = root.child(screens::wizard::view(s, w, accent));
    }
    // Open dropdown lists go above everything, toasts last.
    let layer = crate::widgets::take_layer();
    rune_kit::root(root, layer.into_iter().chain([toasts(s)]))
}

fn sidebar(s: &App, accent: rune::Color) -> El {
    let mut c = col()
        .key("sidebar")
        .w(76.0)
        .h_full()
        .fill(theme::panel())
        .pt(38.0)
        .pb(14.0)
        .gap(2.0)
        .items_center();
    c = c.child(logo(accent)).child(el().h(14.0));
    for screen in Screen::ALL {
        let on = s.screen == screen;
        c = c.child(
            col()
                .key(format!("nav-{}", screen.name()))
                .role("tab")
                .checked(on)
                .label(screen.name())
                .help(screen.about())
                .w(66.0)
                .h(50.0)
                .center()
                .gap(3.0)
                .radius(10.0)
                .fill(if on {
                    accent.alpha(0.14)
                } else {
                    theme::panel()
                })
                .hover(theme::hover())
                .press_scale(0.95)
                .motion(rune::tok::motion::SNAPPY)
                .on_press(move |s: &mut App| {
                    s.screen = screen;
                    s.open_menu = None;
                })
                .child(
                    text(screen.glyph())
                        .font(rune::TextStyle::new(16.0, 600.0))
                        .color(if on { accent } else { theme::muted() }),
                )
                .child(text(screen.name()).font(font::SMALL).color(if on {
                    theme::text()
                } else {
                    theme::faint()
                })),
        );
    }
    c.child(rune::spacer()).child(
        text(format!("v{}", env!("CARGO_PKG_VERSION")))
            .font(font::SMALL)
            .color(theme::faint()),
    )
}

/// A cedar tree merged with a sound wave.
fn logo(accent: rune::Color) -> El {
    use rune::Shape;
    let tree = Shape::new()
        .fill(
            vec![
                [18.0, 2.0],
                [30.0, 18.0],
                [24.0, 18.0],
                [33.0, 30.0],
                [3.0, 30.0],
                [12.0, 18.0],
                [6.0, 18.0],
            ],
            theme::moss(),
        )
        .line(
            [18.0, 30.0],
            [18.0, 35.0],
            3.0,
            rune::shape::LineCap::Round,
            theme::cedar(),
        );
    let wave: Vec<[f32; 2]> = (0..=24)
        .map(|i| [6.0 + i as f32, 22.0 + 3.0 * (i as f32 * 0.8).sin()])
        .collect();
    let wave = Shape::new().stroke(wave, 1.6, accent);
    el().size(36.0, 36.0)
        .child(rune::canvas(tree).size(36.0, 36.0).absolute(0.0, 0.0))
        .child(rune::canvas(wave).size(36.0, 36.0).absolute(0.0, 0.0))
}

/// The top row: which project this is, and what you can do with it as a whole.
fn project_bar(s: &App, accent: rune::Color) -> El {
    let p = &s.project;
    let place = s.path.as_ref().map_or_else(
        || "not saved yet".to_string(),
        |d| {
            d.file_name().map_or_else(
                || d.display().to_string(),
                |n| n.to_string_lossy().to_string(),
            )
        },
    );
    let title = row()
        .gap(8.0)
        .items_center()
        .min_w(0.0)
        .child(
            text(p.name.clone())
                .font(font::TITLE)
                .color(theme::text())
                .truncate()
                .max_w(300.0),
        )
        .child(if s.dirty {
            el().size(7.0, 7.0)
                .radius(3.5)
                .fill(theme::warn())
                .help("There are changes that are not saved yet")
        } else {
            el()
        })
        .child(
            text(place)
                .font(font::SMALL)
                .color(theme::faint())
                .truncate()
                .max_w(220.0),
        );
    let file = row()
        .gap(4.0)
        .items_center()
        .child(
            tool_button("new", "＋", "New", "New project (⌘N)", false, accent)
                .on_press_tx(actions::new_project),
        )
        .child(
            tool_button("open", "▤", "Open", "Open a project (⌘O)", false, accent)
                .on_press_tx(actions::open_dialog),
        )
        .child(
            tool_button(
                "save",
                "⤓",
                "Save",
                "Save the project (⌘S). ⇧⌘S saves a copy somewhere else.",
                s.dirty,
                accent,
            )
            .on_press_tx(actions::save),
        )
        .child(
            tool_button(
                "export",
                "⇥",
                if s.ext.export_busy {
                    "Exporting…"
                } else {
                    "Export"
                },
                "Export stems and a manifest for FMOD Studio (⌘E)",
                s.ext.export_busy,
                accent,
            )
            .on_press_tx(actions::export_dialog),
        );
    let tools = row()
        .gap(4.0)
        .items_center()
        .child(
            tool_button("undo", "↶", "Undo", "Undo (⌘Z)", false, accent)
                .disabled(s.undo.is_empty())
                .on_press(App::undo),
        )
        .child(
            tool_button("redo", "↷", "Redo", "Redo (⇧⌘Z)", false, accent)
                .disabled(s.redo.is_empty())
                .on_press(App::redo),
        )
        .child(el().w(8.0))
        .child(
            tool_button(
                "sim",
                "◐",
                "Game",
                "Game controls beside every screen: modes, parameters, events",
                s.sim_open,
                accent,
            )
            .on_press(|s: &mut App| s.sim_open = !s.sim_open),
        )
        .child(
            tool_button(
                "ai",
                "✧",
                "Assistant",
                "Ask the assistant (⌘/)",
                s.assistant_open,
                accent,
            )
            .on_press(|s: &mut App| s.assistant_open = !s.assistant_open),
        );
    row()
        .key("projectbar")
        .w_full()
        .h(48.0)
        .px(18.0)
        .gap(16.0)
        .items_center()
        .fill(theme::panel())
        .window_drag()
        .child(title)
        .child(file)
        .child(rune::spacer())
        .child(tools)
}

/// The second row: play, tempo, key and scale, and the game mode that is playing.
fn transport_bar(s: &App, accent: rune::Color) -> El {
    let p = &s.project;
    let play = row()
        .key("play")
        .role("button")
        .label(if s.playing { "Stop" } else { "Play" })
        .help("Play or stop (Space)")
        .h(34.0)
        .w(84.0)
        .gap(7.0)
        .center()
        .radius(9.0)
        .fill(if s.playing { accent } else { theme::card_hi() })
        .press_scale(0.95)
        .hover(theme::hover())
        .motion(rune::tok::motion::SNAPPY)
        .on_press(App::toggle_play)
        .child(
            text(if s.playing { "■" } else { "▶" })
                .font(rune::TextStyle::new(12.0, 700.0))
                .color(if s.playing {
                    theme::bg()
                } else {
                    theme::text()
                }),
        )
        .child(
            text(if s.playing { "Stop" } else { "Play" })
                .font(font::HEAD)
                .color(if s.playing {
                    theme::bg()
                } else {
                    theme::text()
                }),
        );
    let beat_dot = el()
        .size(8.0, 8.0)
        .radius(4.0)
        .fill(accent)
        .live_opacity(&s.live.beat);
    let bpm = crate::screens::common::bpm_drag(s, accent);
    let pos = col()
        .w(76.0)
        .gap(1.0)
        .help("Where the music is: bar . beat . step")
        .child(text("BAR").font(font::CAPS).color(theme::faint()))
        .child(
            text("")
                .font(font::NUM_BIG)
                .color(theme::text())
                .live_text(&s.live.position),
        );
    let keys: Vec<&str> = (0..12).map(|i| PitchClass(i).name()).collect();
    let scales: Vec<&str> = ScaleKind::ALL.iter().map(|k| k.name()).collect();
    let key = row()
        .gap(6.0)
        .items_center()
        .help(format!(
            "Key and scale: the notes the music is built from. {} — {}.",
            p.scale.name(),
            p.scale.mood()
        ))
        .child(dropdown(
            s,
            "key",
            &keys,
            Some(usize::from(p.key.0)),
            74.0,
            |s: &mut App, i, _| actions::set_key(s, i),
        ))
        .child(dropdown(
            s,
            "scale",
            &scales,
            ScaleKind::ALL.iter().position(|k| *k == p.scale),
            168.0,
            |s: &mut App, i, _| actions::set_scale(s, i),
        ));
    let metro = tool_button(
        "metro",
        "♩",
        "Click",
        "Metronome: a click on every beat",
        s.metronome,
        accent,
    )
    .on_press(actions::toggle_metronome);

    let mut modes = row().key("modes").gap(6.0).items_center();
    for (i, m) in p.game_modes.iter().enumerate() {
        let color = theme::mode_color(&m.color);
        let playing = s.mode == Some(i);
        let pending = s.pending_mode == Some(i) && !playing;
        let mut chip = pill(format!("mode-{i}"), m.name.clone(), color, playing)
            .help(format!("{} — {} (key {})", m.name, m.about, i + 1))
            .on_press(move |s: &mut App| s.request_mode(i));
        if pending {
            chip = chip.border(1.0, color.alpha(0.9)).border_dash(4.0, 3.0);
        }
        modes = modes.child(chip);
    }

    row()
        .key("topbar")
        .w_full()
        .h(54.0)
        .px(18.0)
        .gap(14.0)
        .items_center()
        .fill(theme::panel())
        .border_sides([0.0, 0.0, 0.0, 1.0], theme::line())
        .child(play)
        .child(beat_dot)
        .child(pos)
        .child(bpm)
        .child(key)
        .child(metro)
        .child(el().w(1.0).h(24.0).fill(theme::line_strong()))
        .child(
            text("GAME MODE")
                .font(font::CAPS)
                .color(theme::faint())
                .help("What the game is doing right now. Click one to switch the music to it."),
        )
        .child(row().grow(1.0).min_w(0.0).child(hscroll(modes)))
}

fn toasts(s: &App) -> El {
    let mut c = col()
        .key("toasts")
        .gap(8.0)
        .pin(None, None, Some(20.0), Some(20.0))
        .items_end();
    for t in s.toasts.iter().rev().take(4) {
        let color = if t.error {
            theme::danger()
        } else {
            theme::ok()
        };
        let id = t.id;
        c = c.child(
            row()
                .key(format!("toast-{id}"))
                .max_w(420.0)
                .px(14.0)
                .py(10.0)
                .gap(10.0)
                .items_center()
                .radius(10.0)
                .fill(theme::card_hi())
                .border(1.0, color.alpha(0.5))
                .shadow(6.0, 24.0, theme::shadow())
                .transition(rune::Transition::offset(0.0, 12.0))
                .on_press(move |s: &mut App| s.toasts.retain(|t| t.id != id))
                .child(el().size(8.0, 8.0).radius(4.0).fill(color))
                .child(
                    text(t.text.clone())
                        .font(font::BODY)
                        .color(theme::text())
                        .wrap(),
                ),
        );
    }
    c
}
