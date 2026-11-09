//! Adaptive: the rules that connect the game to the music.

use crate::state::App;
use crate::theme::{self, font};
use crate::widgets::{
    body, button, caps, card, dropdown, screen_header, segmented, slider, slider_row, term, vscroll,
};
use aa_core::{CurveTarget, Project, Quantize, ResponseCurve, TransitionRule, TransitionSpec};
use rune::{Color, Drag, El, Phase, Shape, Tx, canvas, col, el, row, text};

const QUANTIZES: [Quantize; 5] = [
    Quantize::Immediate,
    Quantize::Beat,
    Quantize::Bar,
    Quantize::Phrase(2),
    Quantize::Phrase(4),
];
const FADES: [f32; 6] = [0.0, 0.5, 1.0, 2.0, 4.0, 8.0];

pub const TABS: [&str; 5] = [
    "Game modes",
    "Events",
    "Transitions",
    "Curves",
    "Parameters",
];

pub fn view(s: &App, accent: Color) -> El {
    let tab = s.tabs.adaptive.min(TABS.len() - 1);
    let about = [
        "A game mode is what the game is doing: exploring, hiding, being chased. Each mode plays one section of the music (a column on the Session screen) and moves the game parameters to its own values.",
        "An event is a single moment in the game: a door slams, a puzzle is solved. It can play a short musical accent (a stinger) and switch on tracks that were silent until then.",
        "A transition is how the music gets from one mode to another: when the switch may happen (right away, on the next beat, on the next bar) and how long the two sections fade into each other.",
        "A curve makes one track react to one game parameter: as Intensity rises, the drums get louder, the pad gets brighter. Influence says how strongly the track listens.",
        "Parameters are numbers from 0 to 1 that the game sends all the time. The music follows them smoothly: the slew is how long it takes to catch up.",
    ][tab];
    let page = match tab {
        0 => modes(s, accent),
        1 => events(s, accent),
        2 => matrix(s, accent),
        3 => curves(s, accent),
        _ => params(s, accent),
    };
    col()
        .key("adaptive")
        .w_full()
        .h_full()
        .padding(22.0)
        .gap(16.0)
        .min_h(0.0)
        .child(screen_header(
            "Adaptive",
            "The rules that connect the game to the music. The game sends modes, events and parameters; these rules decide what you hear.",
            accent,
        ))
        .child(flow(accent))
        .child(segmented("adaptive-tab", &TABS, tab, accent, |s: &mut App, i| {
            s.tabs.adaptive = i;
            s.open_menu = None;
        }))
        .child(body(about).max_w(860.0))
        .child(vscroll(page))
}

/// The whole idea in one line: what the game sends and what the music does with it.
fn flow(accent: Color) -> El {
    let step = |title: &str, words: &str, color: Color| {
        col()
            .gap(2.0)
            .px(12.0)
            .py(8.0)
            .radius(10.0)
            .fill(theme::card())
            .child(text(title.to_string()).font(font::LABEL).color(color))
            .child(
                text(words.to_string())
                    .font(font::SMALL)
                    .color(theme::faint()),
            )
    };
    let arrow = || text("→").font(font::TITLE).color(theme::faint());
    row()
        .key("adaptive-flow")
        .gap(10.0)
        .items_center()
        .child(step("The game", "says what is happening", theme::text()))
        .child(arrow())
        .child(step("Mode", "picks the section", accent))
        .child(step("Event", "plays an accent", theme::warn()))
        .child(step("Parameters", "shape each track", theme::ok()))
        .child(arrow())
        .child(step("The music", "changes in time", theme::text()))
}

fn params(s: &App, accent: Color) -> El {
    let mut c = col().gap(12.0);
    for (i, p) in s.project.game_params.iter().enumerate() {
        c = c.child(
            card()
                .w(560.0)
                .child(
                    row()
                        .gap(10.0)
                        .items_center()
                        .child(term(&p.name, font::TITLE, theme::text()))
                        .child(
                            text(p.about.clone())
                                .font(font::SMALL)
                                .color(theme::muted())
                                .wrap()
                                .grow(1.0)
                                .min_w(0.0),
                        ),
                )
                .child(
                    slider_row(
                        &format!("pdef-{i}"),
                        "Starts at",
                        p.default,
                        format!("{:.2}", p.default),
                        240.0,
                        accent,
                        move |s: &mut App, v, st| {
                            if st {
                                s.begin_gesture();
                            }
                            s.edit_quiet(|p| {
                                p.game_params[i].default = (v * 100.0).round() / 100.0
                            });
                        },
                    )
                    .help("The value when the music starts, before the game says anything"),
                )
                .child(
                    slider_row(
                        &format!("pslew-{i}"),
                        "Slew",
                        p.slew_seconds / 10.0,
                        format!("{:.1} s", p.slew_seconds),
                        240.0,
                        accent,
                        move |s: &mut App, v, st| {
                            if st {
                                s.begin_gesture();
                            }
                            s.edit_quiet(|p| {
                                p.game_params[i].slew_seconds = (v * 100.0).round() / 10.0
                            });
                        },
                    )
                    .help("How long the music takes to follow a jump in this parameter"),
                ),
        );
    }
    c
}

fn modes(s: &App, accent: Color) -> El {
    let p = &s.project;
    let sections: Vec<String> = p.sections.iter().map(|x| x.name.clone()).collect();
    let mut c = col().gap(8.0);
    for (i, m) in p.game_modes.iter().enumerate() {
        let color = theme::mode_color(&m.color);
        let current = m
            .section
            .and_then(|id| p.sections.iter().position(|x| x.id == id));
        let presets = m
            .param_presets
            .iter()
            .map(|(name, v)| format!("{name} {v:.2}"))
            .collect::<Vec<_>>()
            .join("  ·  ");
        let playing = s.mode == Some(i);
        c = c.child(
            row()
                .key(format!("mode-card-{i}"))
                .gap(14.0)
                .items_center()
                .px(16.0)
                .py(12.0)
                .radius(12.0)
                .fill(theme::card())
                .border(1.0, if playing { color } else { theme::card() })
                .child(el().size(12.0, 12.0).radius(6.0).fill(color))
                .child(
                    col()
                        .w(230.0)
                        .gap(2.0)
                        .child(text(m.name.clone()).font(font::TITLE).color(theme::text()))
                        .child(
                            text(m.about.clone())
                                .font(font::SMALL)
                                .color(theme::muted())
                                .wrap(),
                        ),
                )
                .child(
                    text("plays section")
                        .font(font::SMALL)
                        .color(theme::faint()),
                )
                .child(dropdown(
                    s,
                    &format!("msec-{i}"),
                    &sections,
                    current,
                    170.0,
                    move |s: &mut App, k, _| {
                        s.edit(|p| p.game_modes[i].section = p.sections.get(k).map(|x| x.id));
                    },
                ))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(2.0)
                        .child(text("and sets").font(font::SMALL).color(theme::faint()))
                        .child(
                            text(if presets.is_empty() {
                                "nothing".to_string()
                            } else {
                                presets
                            })
                            .font(font::SMALL.mono())
                            .color(theme::muted())
                            .truncate(),
                        ),
                )
                .child(
                    button(format!("mode-try-{i}"), "▶ Try", accent, false)
                        .help("Switch the music to this mode now")
                        .on_press(move |s: &mut App| {
                            s.request_mode(i);
                            if !s.playing {
                                s.set_playing(true);
                            }
                        }),
                ),
        );
    }
    c
}

fn events(s: &App, accent: Color) -> El {
    let p = &s.project;
    let names: Vec<String> = QUANTIZES.iter().map(|q| q.name()).collect();
    let mut c = col().gap(8.0);
    if p.events.is_empty() {
        c = c.child(body("This project has no events."));
    }
    for (i, e) in p.events.iter().enumerate() {
        let stinger = e.stinger.and_then(|st| {
            let track = p.track(st.track)?;
            Some(format!("“{}” on {}", track.clip(st.clip)?.name, track.name))
        });
        let unlocked: Vec<String> = p
            .tracks
            .iter()
            .filter(|t| {
                (e.unlocks && t.adaptive.locked_until_event.as_deref() == Some(&e.name))
                    || e.unlock_track == Some(t.id)
            })
            .map(|t| t.name.clone())
            .collect();
        let mut does = col().grow(1.0).min_w(0.0).gap(3.0);
        does = does.child(
            text(match &stinger {
                Some(st) => format!("plays the stinger {st}"),
                None => "plays no stinger".to_string(),
            })
            .font(font::LABEL)
            .color(if stinger.is_some() {
                theme::text()
            } else {
                theme::faint()
            })
            .truncate(),
        );
        if !unlocked.is_empty() {
            does = does.child(
                text(format!("switches on: {}", unlocked.join(", ")))
                    .font(font::LABEL)
                    .color(theme::ok())
                    .truncate(),
            );
        }
        c = c.child(
            row()
                .key(format!("event-card-{i}"))
                .gap(14.0)
                .items_center()
                .px(16.0)
                .py(12.0)
                .radius(12.0)
                .fill(theme::card())
                .child(text("⚡").font(font::TITLE).color(theme::warn()))
                .child(
                    col()
                        .w(230.0)
                        .gap(2.0)
                        .child(text(e.name.clone()).font(font::TITLE).color(theme::text()))
                        .child(
                            text(e.about.clone())
                                .font(font::SMALL)
                                .color(theme::muted())
                                .wrap(),
                        ),
                )
                .child(
                    text("lands on the next")
                        .font(font::SMALL)
                        .color(theme::faint()),
                )
                .child(dropdown(
                    s,
                    &format!("eq-{i}"),
                    &names,
                    QUANTIZES.iter().position(|q| *q == e.quantize),
                    130.0,
                    move |s: &mut App, k, _| s.edit(|p| p.events[i].quantize = QUANTIZES[k]),
                ))
                .child(text("and").font(font::SMALL).color(theme::faint()))
                .child(does)
                .child(
                    button(format!("event-try-{i}"), "⚡ Fire", accent, false)
                        .help("Send this event now, as the game would")
                        .on_press(move |s: &mut App| s.trigger_event(i)),
                ),
        );
    }
    c.child(body(
        "Stingers are clips on a track. To change one, edit its clip on the Session screen.",
    ))
}

fn set_rule(p: &mut Project, from: &str, to: &str, f: impl FnOnce(&mut TransitionSpec)) {
    let current = p.transition_between(from, to).clone();
    match p
        .transitions
        .iter_mut()
        .find(|r| r.from.as_deref() == Some(from) && r.to.as_deref() == Some(to))
    {
        Some(r) => f(&mut r.spec),
        None => {
            let mut spec = current;
            f(&mut spec);
            p.transitions.push(TransitionRule {
                from: Some(from.into()),
                to: Some(to.into()),
                spec,
            });
        }
    }
}

fn matrix(s: &App, accent: Color) -> El {
    let p = &s.project;
    let cw = 92.0;
    let selected = s
        .tabs
        .transition
        .filter(|(f, t)| *f < p.game_modes.len() && *t < p.game_modes.len() && f != t);
    let mut head = row().gap(4.0).child(
        el().w(100.0).child(
            text("from ↓   to →")
                .font(font::SMALL)
                .color(theme::faint()),
        ),
    );
    for m in &p.game_modes {
        head = head.child(
            text(m.name.clone())
                .font(font::LABEL)
                .color(theme::mode_color(&m.color))
                .w(cw)
                .truncate(),
        );
    }
    let mut grid = col().gap(4.0).child(head);
    for (fi, from) in p.game_modes.iter().enumerate() {
        let mut line = row().gap(4.0).items_center().child(
            text(from.name.clone())
                .font(font::LABEL)
                .color(theme::mode_color(&from.color))
                .w(100.0),
        );
        for (ti, to) in p.game_modes.iter().enumerate() {
            if fi == ti {
                line = line.child(el().size(cw, 42.0).radius(8.0).fill(theme::panel()));
                continue;
            }
            let spec = p.transition_between(&from.name, &to.name);
            let exact = p.transitions.iter().any(|r| {
                r.from.as_deref() == Some(&from.name) && r.to.as_deref() == Some(&to.name)
            });
            let on = selected == Some((fi, ti));
            line = line.child(
                col()
                    .key(format!("tr-{}-{}", from.name, to.name))
                    .role("button")
                    .label(format!("{} to {}", from.name, to.name))
                    .help(if exact {
                        "This pair has its own rule. Click to edit it."
                    } else {
                        "This pair uses the default rule. Click to give it its own."
                    })
                    .size(cw, 42.0)
                    .justify_center()
                    .px(8.0)
                    .radius(8.0)
                    .fill(if on {
                        accent.alpha(0.22)
                    } else if exact {
                        theme::card_hi()
                    } else {
                        theme::card()
                    })
                    .border(1.0, if on { accent } else { theme::card() })
                    .hover(theme::hover())
                    .press_scale(0.96)
                    .on_press(move |s: &mut App| {
                        s.tabs.transition = Some((fi, ti));
                        s.open_menu = None;
                    })
                    .child(
                        text(spec.quantize.name())
                            .font(font::SMALL)
                            .color(if exact { theme::text() } else { theme::muted() }),
                    )
                    .child(
                        text(format!(
                            "{}{}",
                            fade_cell(spec.crossfade_beats),
                            if spec.stinger_event.is_some() {
                                " ⚡"
                            } else {
                                ""
                            }
                        ))
                        .font(font::SMALL)
                        .color(theme::faint()),
                    ),
            );
        }
        grid = grid.child(line);
    }
    let editor = match selected {
        None => card().child(body(
            "Click a cell to edit how the music moves from the row's mode to the column's mode. Brighter cells have their own rule; the rest share the default.",
        )),
        Some((fi, ti)) => {
            let (from, to) = (&p.game_modes[fi], &p.game_modes[ti]);
            let spec = p.transition_between(&from.name, &to.name);
            let names: Vec<String> = QUANTIZES.iter().map(|q| q.name()).collect();
            let fades: Vec<String> = FADES.iter().map(|f| fade_name(*f)).collect();
            let (f1, t1, f2, t2) = (
                from.name.clone(),
                to.name.clone(),
                from.name.clone(),
                to.name.clone(),
            );
            card()
                .child(
                    row()
                        .gap(8.0)
                        .items_center()
                        .child(
                            text(from.name.clone())
                                .font(font::TITLE)
                                .color(theme::mode_color(&from.color)),
                        )
                        .child(text("→").font(font::TITLE).color(theme::faint()))
                        .child(
                            text(to.name.clone())
                                .font(font::TITLE)
                                .color(theme::mode_color(&to.color)),
                        ),
                )
                .child(
                    row()
                        .gap(10.0)
                        .items_center()
                        .child(text("Switch on the next").font(font::LABEL).color(theme::muted()))
                        .child(dropdown(
                            s,
                            "tr-quantize",
                            &names,
                            QUANTIZES.iter().position(|q| *q == spec.quantize),
                            130.0,
                            move |s: &mut App, k, _| {
                                s.edit(|p| set_rule(p, &f1, &t1, |sp| sp.quantize = QUANTIZES[k]))
                            },
                        ))
                        .child(text("and fade over").font(font::LABEL).color(theme::muted()))
                        .child(dropdown(
                            s,
                            "tr-fade",
                            &fades,
                            FADES
                                .iter()
                                .position(|x| (*x - spec.crossfade_beats).abs() < 1e-3),
                            130.0,
                            move |s: &mut App, k, _| {
                                s.edit(|p| {
                                    set_rule(p, &f2, &t2, |sp| sp.crossfade_beats = FADES[k])
                                })
                            },
                        )),
                )
                .child(body(match &spec.stinger_event {
                    Some(e) => format!("The event “{e}” fires on the way."),
                    None => "No event fires on the way.".to_string(),
                }))
        }
    };
    col()
        .gap(14.0)
        .child(crate::widgets::hscroll(grid))
        .child(editor.w(620.0))
}

fn fade_cell(beats: f32) -> String {
    if beats <= 0.0 {
        "hard cut".into()
    } else {
        format!("fade {beats}")
    }
}

fn fade_name(beats: f32) -> String {
    if beats <= 0.0 {
        "hard cut".into()
    } else if beats == 1.0 {
        "1 beat".into()
    } else {
        format!("{beats} beats")
    }
}

fn target_max(t: CurveTarget) -> f32 {
    if t == CurveTarget::Detune { 100.0 } else { 1.0 }
}

fn curves(s: &App, accent: Color) -> El {
    let p = &s.project;
    let Some(track) = s.selected_track.and_then(|t| p.track(t)) else {
        return card()
            .child(caps("Response curves"))
            .child(crate::widgets::body(
                "This project has no tracks yet. Add one on the Session screen.",
            ));
    };
    let tid = track.id;
    let color = theme::role_color(track.role);
    let names: Vec<String> = p.tracks.iter().map(|t| t.name.clone()).collect();
    let picker = row()
        .gap(10.0)
        .items_center()
        .child(text("Track").font(font::LABEL).color(theme::muted()))
        .child(el().size(10.0, 10.0).radius(5.0).fill(color))
        .child(dropdown(
            s,
            "curve-track",
            &names,
            p.track_index(tid),
            200.0,
            |s: &mut App, i, _| {
                if let Some(id) = s.project.tracks.get(i).map(|t| t.id) {
                    s.select_clip(id, None);
                }
            },
        ));
    let mut c = card()
        .child(
            row()
                .gap(8.0)
                .items_center()
                .child(caps("Response curves"))
                .child(rune::spacer())
                .child(term("Influence", font::LABEL, theme::muted()))
                .child(slider(
                    "infl-big",
                    track.adaptive.influence,
                    160.0,
                    accent,
                    move |s: &mut App, v, st| {
                        if st {
                            s.begin_gesture();
                        }
                        s.edit_quiet(|p| {
                            if let Some(t) = p.track_mut(tid) {
                                t.adaptive.influence = (v * 20.0).round() / 20.0
                            }
                        });
                    },
                ))
                .child(
                    text(format!("{:.0}%", track.adaptive.influence * 100.0))
                        .font(font::NUM)
                        .color(theme::text()),
                ),
        )
        .child(picker);
    if let Some(lock) = &track.adaptive.locked_until_event {
        c = c.child(
            text(format!("🔒 Silent until the event “{lock}” fires"))
                .font(font::LABEL)
                .color(theme::warn()),
        );
    }
    let mut list = row().gap(12.0).flex_wrap();
    for (ci, curve) in track.adaptive.curves.iter().enumerate() {
        list = list.child(curve_card(s, tid, ci, curve, color, accent));
    }
    c = c.child(list);
    // Add a curve: parameter × target.
    let mut add = row()
        .gap(4.0)
        .flex_wrap()
        .items_center()
        .child(caps("Add a curve"));
    for param in p.game_params.iter().map(|g| g.name.clone()) {
        for target in [
            CurveTarget::Volume,
            CurveTarget::FilterCutoff,
            CurveTarget::Density,
            CurveTarget::Drive,
            CurveTarget::Wobble,
        ] {
            let name = param.clone();
            add = add.child(
                button(
                    format!("add-{param}-{}", target.name()),
                    format!("{param} → {}", target.name()),
                    accent,
                    false,
                )
                .on_press(move |s: &mut App| {
                    let n = name.clone();
                    s.edit(|p| {
                        if let Some(t) = p.track_mut(tid) {
                            let max = target_max(target);
                            let pts = if target.neutral() > 0.5 {
                                vec![[0.0, 0.2 * max], [1.0, max]]
                            } else {
                                vec![[0.0, 0.0], [1.0, 0.6 * max]]
                            };
                            t.adaptive.curves.push(ResponseCurve::new(n, target, &pts));
                        }
                    })
                }),
            );
        }
    }
    c.child(add)
}

fn curve_card(
    s: &App,
    tid: u32,
    ci: usize,
    curve: &ResponseCurve,
    color: Color,
    accent: Color,
) -> El {
    let (w, h) = (230.0, 120.0);
    let max = target_max(curve.target);
    let pts: Vec<[f32; 2]> = (0..=40)
        .map(|k| {
            let x = k as f32 / 40.0;
            [x * w, h - (curve.eval(x) / max).clamp(0.0, 1.0) * h]
        })
        .collect();
    let mut plot = el()
        .key(format!("curve-{tid}-{ci}"))
        .size(w, h)
        .radius(8.0)
        .fill(theme::panel())
        .border(1.0, theme::line())
        .child(
            canvas(
                Shape::new()
                    .fill(
                        [vec![[0.0, h]], pts.clone(), vec![[w, h]]].concat(),
                        color.alpha(0.12),
                    )
                    .stroke(pts, 2.0, color),
            )
            .size(w, h)
            .absolute(0.0, 0.0),
        );
    // The parameter's live value as a vertical line.
    if let Some(pi) = s.project.param_index(&curve.param) {
        let x = s.controller.telemetry.params[pi].get() * w;
        plot = plot.child(el().size(1.5, h).fill(accent.alpha(0.7)).absolute(x, 0.0));
    }
    for (k, pt) in curve.points.iter().enumerate() {
        let (px, py) = (pt[0] * w, h - (pt[1] / max).clamp(0.0, 1.0) * h);
        plot = plot.child(
            el().key(format!("pt-{tid}-{ci}-{k}"))
                .size(14.0, 14.0)
                .radius(7.0)
                .fill(theme::text())
                .border(2.0, color)
                .absolute(px - 7.0, py - 7.0)
                .cursor(rune::Cursor::Grab)
                .on_drag(move |s: &mut App, d: &mut Drag, _: &mut Tx| {
                    if d.cancelled() {
                        return;
                    }
                    if d.phase == Phase::Down {
                        s.begin_gesture();
                    }
                    let nx = ((px + d.dx) / w).clamp(0.0, 1.0);
                    let ny = (1.0 - (py + d.dy) / h).clamp(0.0, 1.0) * max;
                    s.edit_quiet(|p| {
                        if let Some(c) =
                            p.track_mut(tid).and_then(|t| t.adaptive.curves.get_mut(ci))
                        {
                            if let Some(point) = c.points.get_mut(k) {
                                *point =
                                    [(nx * 100.0).round() / 100.0, (ny * 100.0).round() / 100.0];
                            }
                            c.points.sort_by(|a, b| a[0].total_cmp(&b[0]));
                        }
                    });
                }),
        );
    }
    col()
        .gap(6.0)
        .padding(10.0)
        .radius(12.0)
        .fill(theme::card_hi())
        .border(1.0, theme::line())
        .child(
            row()
                .gap(6.0)
                .items_center()
                .child(term(&curve.param, font::LABEL, theme::text()))
                .child(text("→").font(font::LABEL).color(theme::faint()))
                .child(term(curve.target.name(), font::LABEL, color))
                .child(rune::spacer())
                .child(
                    button(format!("ptadd-{tid}-{ci}"), "+pt", accent, false)
                        .help("Add a point in the middle")
                        .on_press(move |s: &mut App| {
                            s.edit(|p| {
                                if let Some(c) =
                                    p.track_mut(tid).and_then(|t| t.adaptive.curves.get_mut(ci))
                                {
                                    let x = 0.5;
                                    let y = c.eval(x);
                                    c.points.push([x, y]);
                                    c.points.sort_by(|a, b| a[0].total_cmp(&b[0]));
                                }
                            })
                        }),
                )
                .child(
                    button(format!("cdel-{tid}-{ci}"), "✕", theme::danger(), false).on_press(
                        move |s: &mut App| {
                            s.edit(|p| {
                                if let Some(t) = p.track_mut(tid)
                                    && ci < t.adaptive.curves.len()
                                {
                                    t.adaptive.curves.remove(ci);
                                }
                            })
                        },
                    ),
                ),
        )
        .child(plot)
        .child(
            row()
                .child(text("0").font(font::SMALL).color(theme::faint()))
                .child(rune::spacer())
                .child(
                    text(format!("{} → 1", curve.param))
                        .font(font::SMALL)
                        .color(theme::faint()),
                ),
        )
}
