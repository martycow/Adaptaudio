//! The Simulator: play the game's role. Drag the XY pad, press modes and events, run scenarios.

use crate::actions;
use crate::live::{PAD_BIG, PAD_SMALL};
use crate::state::App;
use crate::theme::{self, font};
use crate::widgets::{button, caps, live_bar, screen_header};
use rune::{Color, Drag, El, Phase, Shape, Tx, canvas, col, el, row, text};

/// The right-hand panel shown next to every screen.
pub fn panel(s: &App, accent: Color) -> El {
    col()
        .key("sim-panel")
        .w(PAD_SMALL + 40.0)
        .h_full()
        .fill(theme::panel())
        .border_sides([1.0, 0.0, 0.0, 0.0], theme::line())
        .child(
            col()
                .grow(1.0)
                .min_h(0.0)
                .scroll(rune::el::ScrollSpec {
                    stick_bottom: false,
                })
                .child(
                    col()
                        .padding(20.0)
                        .gap(16.0)
                        .child(now_playing(s, accent))
                        .child(pad(s, accent, false))
                        .child(params(s, accent, PAD_SMALL))
                        .child(modes(s, 2))
                        .child(events(s, accent))
                        .child(scenarios(s, accent))
                        .child(log(s, 6)),
                ),
        )
}

/// The full Simulator screen.
pub fn view(s: &App, accent: Color) -> El {
    let left = col()
        .gap(16.0)
        .child(pad(s, accent, true))
        .child(params(s, accent, PAD_BIG));
    let right = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(18.0)
        .child(now_playing(s, accent))
        .child(modes(s, 4))
        .child(events(s, accent))
        .child(engine_view(s, accent))
        .child(scenarios(s, accent))
        .child(log(s, 14));
    col()
        .key("sim-screen")
        .w_full()
        .h_full()
        .padding(22.0)
        .gap(16.0)
        .min_h(0.0)
        .child(screen_header("Simulator", "The game without the game. Drag the pad, switch modes, fire events, or run a whole scenario.", accent))
        .child(col().grow(1.0).min_h(0.0).scroll(rune::el::ScrollSpec { stick_bottom: false }).child(row().gap(28.0).items_start().child(left).child(right)))
}

fn now_playing(s: &App, accent: Color) -> El {
    let p = &s.project;
    let mode = s.mode.and_then(|m| p.game_modes.get(m));
    let pending = s
        .pending_mode
        .filter(|m| Some(*m) != s.mode)
        .and_then(|m| p.game_modes.get(m));
    let mut c = col().gap(4.0).child(caps("Now playing"));
    c = c.child(
        row()
            .gap(10.0)
            .items_center()
            .child(
                el().size(14.0, 14.0)
                    .radius(7.0)
                    .fill(accent)
                    .shadow(0.0, 12.0, accent.alpha(0.8))
                    .live_scale(&s.live.beat_scale),
            )
            .child(
                text(mode.map_or("—".to_string(), |m| m.name.clone()))
                    .font(font::HERO)
                    .color(theme::text()),
            ),
    );
    c = c.child(
        text(
            mode.map_or("Pick a mode below, then press Play".to_string(), |m| {
                m.about.clone()
            }),
        )
        .font(font::BODY)
        .color(theme::muted()),
    );
    if let Some(next) = pending {
        let from = mode.map(|m| m.name.as_str()).unwrap_or("");
        let q = p.transition_between(from, &next.name).quantize.name();
        c = c.child(
            row()
                .gap(6.0)
                .items_center()
                .child(
                    text("→")
                        .font(font::HEAD)
                        .color(theme::mode_color(&next.color)),
                )
                .child(
                    text(format!("{} on next {}", next.name, q.to_lowercase()))
                        .font(font::LABEL)
                        .color(theme::mode_color(&next.color)),
                ),
        );
    }
    c
}

fn pad(s: &App, accent: Color, big: bool) -> El {
    let size = if big { PAD_BIG } else { PAD_SMALL };
    let p = &s.project;
    let xi = p.param_index("Intensity").unwrap_or(0);
    let yi = p.param_index("Tension").unwrap_or(1);
    let mut grid = Shape::new();
    for k in 1..4 {
        let v = size * k as f32 / 4.0;
        grid = grid
            .line(
                [v, 0.0],
                [v, size],
                1.0,
                rune::shape::LineCap::Butt,
                theme::line(),
            )
            .line(
                [0.0, v],
                [size, v],
                1.0,
                rune::shape::LineCap::Butt,
                theme::line(),
            );
    }
    let orb_r = if big { 22.0 } else { 15.0 };
    let (orb, target) = if big {
        (&s.live.orb_big, &s.live.target_big)
    } else {
        (&s.live.orb_small, &s.live.target_small)
    };
    // Mode markers: where each mode's preset sits on the pad.
    let mut marks = el().size(size, size).absolute(0.0, 0.0).inert();
    for m in &p.game_modes {
        let get = |n: &str| {
            m.param_presets
                .iter()
                .find(|(k, _)| k == n)
                .map(|(_, v)| *v)
        };
        if let (Some(x), Some(y)) = (get("Intensity"), get("Tension")) {
            let c = theme::mode_color(&m.color);
            marks = marks.child(
                row()
                    .absolute(
                        10.0 + x * (size - 20.0) - 4.0,
                        10.0 + (1.0 - y) * (size - 20.0) - 4.0,
                    )
                    .gap(4.0)
                    .items_center()
                    .child(el().size(8.0, 8.0).radius(4.0).fill(c.alpha(0.7)))
                    .child(if big {
                        text(m.name.clone()).font(font::SMALL).color(c.alpha(0.8))
                    } else {
                        el()
                    }),
            );
        }
    }
    let surface = el()
        .key(if big { "pad-big" } else { "pad" })
        .role("slider")
        .label("Intensity and Tension")
        .size(size, size)
        .radius(18.0)
        .fill(theme::bg())
        .radial_gradient(
            [0.85, 0.15],
            size * 0.9,
            [
                (0.0, accent.alpha(0.32)),
                (0.6, accent.alpha(0.06)),
                (1.0, theme::bg().alpha(0.0)),
            ],
        )
        .border(1.0, theme::line_strong())
        .clip_rounded(18.0)
        .cursor(rune::Cursor::Crosshair)
        .child(canvas(grid).size(size, size).absolute(0.0, 0.0))
        .child(marks)
        .child(
            el().size(18.0, 18.0)
                .radius(9.0)
                .border(2.0, theme::text().alpha(0.7))
                .absolute(-9.0, -9.0)
                .live_offset(target.smooth(rune::Spring::new(0.15, 0.0))),
        )
        .child(
            el().size(orb_r * 2.0, orb_r * 2.0)
                .radius(orb_r)
                .fill(accent)
                .radial_gradient(
                    [0.35, 0.3],
                    orb_r * 1.4,
                    [
                        (0.0, theme::text()),
                        (0.45, accent),
                        (1.0, accent.alpha(0.6)),
                    ],
                )
                .shadow(0.0, orb_r * 2.2, accent.alpha(0.85))
                .absolute(-orb_r, -orb_r)
                .live_offset(orb)
                .inert(),
        )
        .on_drag(move |s: &mut App, d: &mut Drag, _: &mut Tx| {
            if d.cancelled() {
                return;
            }
            let x = (d.x / d.w).clamp(0.0, 1.0);
            let y = 1.0 - (d.y / d.h).clamp(0.0, 1.0);
            s.set_param(xi, x);
            s.set_param(yi, y);
            if d.phase == Phase::Up {
                s.log_line(format!("Intensity {x:.2} · Tension {y:.2}"), None);
            }
        });
    col()
        .gap(6.0)
        .child(
            row()
                .child(crate::widgets::term("Tension", font::CAPS, theme::faint()))
                .child(rune::spacer())
                .child(text("drag me").font(font::SMALL).color(theme::faint())),
        )
        .child(surface)
        .child(
            row()
                .child(rune::spacer())
                .child(crate::widgets::term(
                    "Intensity",
                    font::CAPS,
                    theme::faint(),
                ))
                .child(text(" →").font(font::CAPS).color(theme::faint())),
        )
}

fn params(s: &App, accent: Color, w: f32) -> El {
    let mut c = col().gap(10.0).child(caps("Game parameters"));
    for (i, def) in s
        .project
        .game_params
        .iter()
        .enumerate()
        .take(crate::live::MAX_PARAMS)
    {
        let color = if def.name == "Paranoia" {
            theme::mode_color("#c77dff")
        } else {
            accent
        };
        let bar_w = w - 8.0;
        let track = el()
            .key(format!("param-{i}"))
            .role("slider")
            .label(def.name.clone())
            .size(bar_w, 22.0)
            .radius(7.0)
            .fill(theme::card())
            .border(1.0, theme::line())
            .cursor(rune::Cursor::Pointer)
            .child(el().absolute(4.0, 7.0).child(live_bar(
                bar_w - 8.0,
                8.0,
                color,
                &s.live.params[i],
            )))
            .child(el().absolute(4.0, 17.0).child(live_bar(
                bar_w - 8.0,
                2.0,
                theme::text().alpha(0.8),
                &s.live.targets[i],
            )))
            .on_drag(move |s: &mut App, d: &mut Drag, _: &mut Tx| {
                if !d.cancelled() {
                    s.set_param(i, (d.x / d.w).clamp(0.0, 1.0));
                }
            });
        c = c.child(
            col()
                .gap(4.0)
                .child(
                    row()
                        .gap(6.0)
                        .child(crate::widgets::term(&def.name, font::LABEL, theme::text()))
                        .child(rune::spacer())
                        .child(
                            text(format!("slew {:.1}s", def.slew_seconds))
                                .font(font::SMALL)
                                .color(theme::faint()),
                        ),
                )
                .child(track),
        );
    }
    c.child(
        text("Bar = what the music hears now; line = where you asked it to go.")
            .font(font::SMALL)
            .color(theme::faint())
            .wrap(),
    )
}

fn modes(s: &App, columns: usize) -> El {
    let mut grid = col().gap(6.0).child(caps("Game modes · keys 1–8"));
    let mut line = row().gap(6.0);
    for (i, m) in s.project.game_modes.iter().enumerate() {
        let color = theme::mode_color(&m.color);
        let playing = s.mode == Some(i);
        let pending = s.pending_mode == Some(i) && !playing;
        let mut b = col()
            .key(format!("simmode-{i}"))
            .role("button")
            .label(m.name.clone())
            .help(m.about.clone())
            .grow(1.0)
            .h(46.0)
            .px(10.0)
            .justify_center()
            .gap(2.0)
            .radius(11.0)
            .fill(if playing {
                color.alpha(0.28)
            } else {
                theme::card()
            })
            .border(
                1.0,
                if playing || pending {
                    color
                } else {
                    theme::line()
                },
            )
            .hover(theme::hover())
            .press_scale(0.95)
            .motion(rune::tok::motion::SNAPPY)
            .on_press(move |s: &mut App| s.request_mode(i))
            .child(
                row()
                    .gap(6.0)
                    .items_center()
                    .child(el().size(8.0, 8.0).radius(4.0).fill(color))
                    .child(text(m.name.clone()).font(font::HEAD).color(theme::text())),
            )
            .child(
                text(if pending {
                    "queued…".to_string()
                } else {
                    format!("key {}", i + 1)
                })
                .font(font::SMALL)
                .color(if pending { color } else { theme::faint() }),
            );
        if pending {
            b = b.border_dash(5.0, 3.0);
        }
        if playing {
            b = b.shadow(0.0, 14.0, color.alpha(0.45));
        }
        line = line.child(b);
        if (i + 1) % columns == 0 {
            grid = grid.child(line);
            line = row().gap(6.0);
        }
    }
    grid.child(line)
}

fn events(s: &App, accent: Color) -> El {
    let mut r = row().gap(6.0).flex_wrap();
    for (i, e) in s.project.events.iter().enumerate() {
        let fired = s.fired & (1 << i.min(63)) != 0;
        r = r.child(
            row()
                .key(format!("event-{i}"))
                .role("button")
                .label(e.name.clone())
                .help(format!(
                    "{} — {} (lands on the next {})",
                    e.name,
                    e.about,
                    e.quantize.name().to_lowercase()
                ))
                .h(30.0)
                .px(12.0)
                .gap(6.0)
                .center()
                .radius(15.0)
                .fill(theme::card_hi())
                .border(1.0, if fired { accent } else { theme::line_strong() })
                .hover(theme::hover())
                .press_scale(0.9)
                .on_press(move |s: &mut App| s.trigger_event(i))
                .child(text("⚡").font(font::LABEL).color(theme::warn()))
                .child(text(e.name.clone()).font(font::LABEL).color(theme::text())),
        );
    }
    col().gap(8.0).child(caps("Events")).child(r)
}

fn scenarios(s: &App, accent: Color) -> El {
    let mut c = col().gap(8.0).child(caps("Scenarios"));
    for (i, sc) in s.project.scenarios.iter().enumerate() {
        let running = s.scenario.as_ref().is_some_and(|r| r.index == i);
        let mut card = col()
            .key(format!("scen-{i}"))
            .gap(6.0)
            .padding(10.0)
            .radius(11.0)
            .fill(theme::card())
            .border(1.0, if running { accent } else { theme::line() })
            .child(
                row()
                    .gap(8.0)
                    .items_center()
                    .child(text(sc.name.clone()).font(font::HEAD).color(theme::text()))
                    .child(
                        text(format!("{:.0}s · {} steps", sc.duration(), sc.steps.len()))
                            .font(font::SMALL)
                            .color(theme::faint()),
                    )
                    .child(rune::spacer())
                    .child(if running {
                        button(format!("scstop-{i}"), "■ Stop", accent, false)
                            .on_press(actions::stop_scenario)
                    } else {
                        button(format!("scplay-{i}"), "▶ Run", accent, true)
                            .on_press(move |s: &mut App| actions::start_scenario(s, i))
                    }),
            );
        // A strip of the scenario's mode changes.
        let dur = sc.duration().max(1.0);
        let mut strip = el().h(8.0).w_full().radius(4.0).fill(theme::panel());
        let mut marks: Vec<(f32, Color)> = Vec::new();
        for st in &sc.steps {
            if let aa_core::ScenarioAction::SetMode(m) = &st.action
                && let Some(g) = s.project.game_modes.iter().find(|g| g.name == *m)
            {
                marks.push((st.at_seconds / dur, theme::mode_color(&g.color)));
            }
        }
        let stops: Vec<(f32, Color)> = marks
            .iter()
            .flat_map(|(t, c)| [(*t, *c), ((*t + 0.0001).min(1.0), *c)])
            .collect();
        if stops.len() >= 2 {
            strip = strip.linear_gradient(
                std::f32::consts::FRAC_PI_2,
                stops.into_iter().take(8).collect::<Vec<_>>(),
            );
        }
        card = card.child(strip);
        if running {
            card = card.child(live_bar(
                PAD_SMALL - 30.0,
                3.0,
                theme::text(),
                &s.live.scenario,
            ));
        }
        c = c.child(card);
    }
    c
}

fn log(s: &App, lines: usize) -> El {
    let mut c = col().gap(3.0).child(caps("Log"));
    for l in s.log.iter().rev().take(lines) {
        let color = l.color.as_deref().map_or(theme::muted(), theme::mode_color);
        c = c.child(
            row()
                .gap(8.0)
                .child(
                    text(format!("{:>6.1}s", l.at))
                        .font(font::SMALL.mono())
                        .color(theme::faint()),
                )
                .child(
                    text(l.text.clone())
                        .font(font::SMALL)
                        .color(color)
                        .truncate(),
                ),
        );
    }
    c
}

/// Under the hood: the two section decks crossfading, and the sidechain.
fn engine_view(s: &App, accent: Color) -> El {
    let deck = |i: usize| {
        row()
            .gap(8.0)
            .items_center()
            .child(
                text(format!("Deck {}", if i == 0 { "A" } else { "B" }))
                    .font(font::SMALL)
                    .color(theme::faint())
                    .w(48.0),
            )
            .child(live_bar(220.0, 6.0, accent, &s.live.deck_gain[i]))
    };
    col()
        .gap(6.0)
        .child(caps("Under the hood"))
        .child(deck(0))
        .child(deck(1))
        .child(
            row()
                .gap(8.0)
                .items_center()
                .child(crate::widgets::term("Sidechain", font::SMALL, theme::faint()).w(48.0))
                .child(live_bar(220.0, 6.0, theme::warn(), &s.live.sidechain)),
        )
        .child(
            row()
                .gap(8.0)
                .child(text("Audio load").font(font::SMALL).color(theme::faint()))
                .child(
                    text("100%")
                        .font(font::SMALL.mono())
                        .color(theme::muted())
                        .w(60.0)
                        .live_text(&s.live.load),
                ),
        )
}
