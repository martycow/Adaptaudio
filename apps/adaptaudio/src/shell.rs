//! The window frame: sidebar, transport bar, screen, Simulator panel, assistant drawer, toasts.

use crate::state::{App, Screen};
use crate::theme::{self, font};
use crate::widgets::{glyph_button, pill};
use crate::{actions, screens};
use rune::{Cx, El, col, el, row, text};

pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let accent = s.accent();
    let content = col()
        .key("content")
        .grow(1.0)
        .h_full()
        .min_w(0.0)
        .min_h(0.0)
        .child(screens::view(s, cx, accent));
    let mut body = row()
        .key("body")
        .grow(1.0)
        .w_full()
        .min_h(0.0)
        .child(content);
    if s.sim_open && s.screen != Screen::Simulator {
        body = body.child(screens::simulator::panel(s, accent));
    }
    if s.assistant_open {
        body = body.child(screens::assistant::drawer(s, accent));
    }
    let main = col()
        .key("main")
        .grow(1.0)
        .h_full()
        .min_w(0.0)
        .child(topbar(s, accent))
        .child(body);
    let mut root = el().key("root").w_full().h_full().fill(theme::bg()).child(
        row()
            .w_full()
            .h_full()
            .child(sidebar(s, accent))
            .child(main),
    );
    // The beat glow around the window edge, in the mode's colour.
    if !s.settings.calm_ui {
        root = root.child(
            el().key("glow")
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .border(2.0, accent.alpha(0.9))
                .box_shadow(rune::Shadow {
                    x: 0.0,
                    y: 0.0,
                    blur: 60.0,
                    spread: -10.0,
                    color: accent.alpha(0.55),
                    inset: true,
                })
                .inert()
                .live_opacity(&s.live.glow),
        );
    }
    root.child(toasts(s))
}

fn sidebar(s: &App, accent: rune::Color) -> El {
    let mut c = col()
        .key("sidebar")
        .w(76.0)
        .h_full()
        .fill(theme::panel())
        .border_sides([0.0, 0.0, 1.0, 0.0], theme::line())
        .pt(38.0)
        .pb(14.0)
        .gap(4.0)
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
                .w(64.0)
                .h(52.0)
                .center()
                .gap(3.0)
                .radius(12.0)
                .fill(if on {
                    accent.alpha(0.16)
                } else {
                    theme::panel()
                })
                .hover(theme::hover())
                .press_scale(0.94)
                .motion(rune::tok::motion::SNAPPY)
                .on_press(move |s: &mut App| s.screen = screen)
                .child(
                    text(screen.glyph())
                        .font(rune::TextStyle::new(17.0, 600.0))
                        .color(if on { accent } else { theme::muted() }),
                )
                .child(text(screen.name()).font(font::SMALL).color(if on {
                    theme::text()
                } else {
                    theme::faint()
                })),
        );
    }
    c.child(rune::spacer())
        .child(text("v0.1").font(font::SMALL).color(theme::faint()))
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

fn topbar(s: &App, accent: rune::Color) -> El {
    let p = &s.project;
    let title = col()
        .gap(2.0)
        .min_w(160.0)
        .child(text("Adaptaudio").font(font::CAPS).color(accent))
        .child(
            row()
                .gap(6.0)
                .items_center()
                .child(
                    text(p.name.clone())
                        .font(font::HEAD)
                        .color(theme::text())
                        .truncate()
                        .max_w(260.0),
                )
                .child(if s.dirty {
                    el().size(7.0, 7.0)
                        .radius(3.5)
                        .fill(theme::warn())
                        .help("Unsaved changes")
                } else {
                    el()
                }),
        );

    let play = row()
        .key("play")
        .role("button")
        .label(if s.playing { "Stop" } else { "Play" })
        .size(44.0, 36.0)
        .center()
        .radius(11.0)
        .fill(if s.playing { accent } else { theme::card_hi() })
        .border(
            1.0,
            if s.playing {
                accent
            } else {
                theme::line_strong()
            },
        )
        .press_scale(0.92)
        .hover(theme::hover())
        .on_press(App::toggle_play)
        .child(
            text(if s.playing { "■" } else { "▶" })
                .font(rune::TextStyle::new(15.0, 700.0))
                .color(if s.playing {
                    theme::bg()
                } else {
                    theme::text()
                }),
        );
    let beat_dot = el()
        .size(10.0, 10.0)
        .radius(5.0)
        .fill(accent)
        .live_opacity(&s.live.beat);
    let bpm = crate::screens::common::bpm_drag(s, accent);
    let pos = col()
        .w(76.0)
        .gap(1.0)
        .child(text("BAR").font(font::CAPS).color(theme::faint()))
        .child(
            text("")
                .font(font::NUM_BIG)
                .color(theme::text())
                .live_text(&s.live.position),
        );
    let scale_label = format!("{} {}", p.key.name(), p.scale.name());
    let key = row()
        .key("keyscale")
        .role("button")
        .label("Key and scale")
        .help(format!(
            "{} — {}. Click to change scale; Shift-click to change key.",
            scale_label,
            p.scale.mood()
        ))
        .h(30.0)
        .px(10.0)
        .center()
        .radius(9.0)
        .fill(theme::card())
        .border(1.0, theme::line_strong())
        .hover(theme::hover())
        .on_press(|s: &mut App| actions::cycle_scale(s, 1))
        .on_secondary_press(|s: &mut App, _, _| actions::cycle_key(s, 1))
        .child(text(scale_label).font(font::LABEL).color(theme::text()));

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

    let right = row()
        .gap(6.0)
        .items_center()
        .child(
            glyph_button("undo", "↶", "Undo (⌘Z)", false, accent)
                .disabled(s.undo.is_empty())
                .on_press(App::undo),
        )
        .child(
            glyph_button("redo", "↷", "Redo (⇧⌘Z)", false, accent)
                .disabled(s.redo.is_empty())
                .on_press(App::redo),
        )
        .child(
            glyph_button("metro", "♩", "Metronome", s.metronome, accent)
                .on_press(actions::toggle_metronome),
        )
        .child(
            glyph_button("open", "⇪", "Open project (⌘O)", false, accent)
                .on_press_tx(actions::open_dialog),
        )
        .child(
            glyph_button("save", "⤓", "Save project (⌘S)", false, accent)
                .on_press_tx(actions::save),
        )
        .child(
            glyph_button(
                "export",
                "⇥",
                "Export to FMOD (⌘E)",
                s.ext.export_busy,
                accent,
            )
            .on_press_tx(actions::export_dialog),
        )
        .child(
            glyph_button("sim", "✦", "Simulator panel", s.sim_open, accent)
                .on_press(|s: &mut App| s.sim_open = !s.sim_open),
        )
        .child(
            glyph_button("ai", "✧", "Assistant (⌘/)", s.assistant_open, accent)
                .on_press(|s: &mut App| s.assistant_open = !s.assistant_open),
        );

    row()
        .key("topbar")
        .w_full()
        .h(64.0)
        .px(18.0)
        .gap(14.0)
        .items_center()
        .fill(theme::panel())
        .border_sides([0.0, 0.0, 0.0, 1.0], theme::line())
        .window_drag()
        .child(title)
        .child(
            row()
                .gap(10.0)
                .items_center()
                .child(play)
                .child(beat_dot)
                .child(pos)
                .child(bpm)
                .child(key),
        )
        .child(row().grow(1.0).min_w(0.0).scroll_x().child(modes))
        .child(right)
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
                .radius(12.0)
                .fill(theme::card_hi())
                .border(1.0, color.alpha(0.6))
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
