//! Adaptive: the rules that connect the game to the music.

use crate::state::App;
use crate::theme::{self, font};
use crate::widgets::{button, caps, card, screen_header, slider, slider_row, term};
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

pub fn view(s: &App, accent: Color) -> El {
    let left = col()
        .w(430.0)
        .gap(16.0)
        .child(params(s, accent))
        .child(modes(s, accent))
        .child(events(s, accent));
    let right = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(16.0)
        .child(curves(s, accent))
        .child(matrix(s, accent));
    col()
        .key("adaptive")
        .w_full()
        .h_full()
        .padding(22.0)
        .gap(16.0)
        .min_h(0.0)
        .child(screen_header("Adaptive", "How the music listens: parameters glide, modes switch on the grid, events fire stingers, curves shape each track.", accent))
        .child(col().grow(1.0).min_h(0.0).scroll_xy().child(row().gap(18.0).items_start().child(left).child(right)))
}

fn params(s: &App, accent: Color) -> El {
    let mut c = card().child(caps("Game parameters"));
    for (i, p) in s.project.game_params.iter().enumerate() {
        c = c.child(
            col()
                .gap(4.0)
                .child(
                    row()
                        .gap(8.0)
                        .child(term(&p.name, font::HEAD, theme::text()))
                        .child(
                            text(p.about.clone())
                                .font(font::SMALL)
                                .color(theme::faint())
                                .truncate(),
                        ),
                )
                .child(slider_row(
                    &format!("pdef-{i}"),
                    "Default",
                    p.default,
                    format!("{:.2}", p.default),
                    180.0,
                    accent,
                    move |s: &mut App, v, st| {
                        if st {
                            s.begin_gesture();
                        }
                        s.edit_quiet(|p| p.game_params[i].default = (v * 100.0).round() / 100.0);
                    },
                ))
                .child(slider_row(
                    &format!("pslew-{i}"),
                    "Slew",
                    p.slew_seconds / 10.0,
                    format!("{:.1} s", p.slew_seconds),
                    180.0,
                    accent,
                    move |s: &mut App, v, st| {
                        if st {
                            s.begin_gesture();
                        }
                        s.edit_quiet(|p| {
                            p.game_params[i].slew_seconds = (v * 100.0).round() / 10.0
                        });
                    },
                )),
        );
    }
    c
}

fn modes(s: &App, accent: Color) -> El {
    let p = &s.project;
    let mut c = card().child(caps("Game modes → sections"));
    for (i, m) in p.game_modes.iter().enumerate() {
        let color = theme::mode_color(&m.color);
        let section = m
            .section
            .and_then(|id| p.section(id))
            .map_or("—".to_string(), |x| x.name.clone());
        c = c.child(
            row()
                .gap(10.0)
                .items_center()
                .child(el().size(10.0, 10.0).radius(5.0).fill(color))
                .child(
                    text(m.name.clone())
                        .font(font::HEAD)
                        .color(theme::text())
                        .w(80.0),
                )
                .child(text("plays").font(font::SMALL).color(theme::faint()))
                .child(
                    button(format!("msec-{i}"), section, accent, false)
                        .help("Click to pick the next section")
                        .on_press(move |s: &mut App| {
                            s.edit(|p| {
                                let ids: Vec<u32> = p.sections.iter().map(|x| x.id).collect();
                                let cur = p.game_modes[i]
                                    .section
                                    .and_then(|id| ids.iter().position(|x| *x == id));
                                let next = cur.map_or(0, |k| (k + 1) % ids.len().max(1));
                                p.game_modes[i].section = ids.get(next).copied();
                            });
                        }),
                )
                .child(
                    text(m.about.clone())
                        .font(font::SMALL)
                        .color(theme::faint())
                        .truncate(),
                ),
        );
    }
    c
}

fn events(s: &App, accent: Color) -> El {
    let p = &s.project;
    let mut c = card().child(caps("Events"));
    for (i, e) in p.events.iter().enumerate() {
        let stinger = e
            .stinger
            .and_then(|st| p.clip(st.track, st.clip))
            .map_or("no stinger".to_string(), |c| {
                format!("stinger “{}”", c.name)
            });
        c = c.child(
            row()
                .gap(8.0)
                .items_center()
                .child(text("⚡").font(font::LABEL).color(theme::warn()))
                .child(
                    text(e.name.clone())
                        .font(font::HEAD)
                        .color(theme::text())
                        .w(110.0),
                )
                .child(
                    button(format!("eq-{i}"), e.quantize.name(), accent, false)
                        .help("When it lands. Click to change.")
                        .on_press(move |s: &mut App| {
                            s.edit(|p| p.events[i].quantize = next_quantize(p.events[i].quantize));
                        }),
                )
                .child(
                    text(stinger)
                        .font(font::SMALL)
                        .color(theme::muted())
                        .truncate(),
                )
                .child(if e.unlocks {
                    text("unlocks layers").font(font::SMALL).color(theme::ok())
                } else {
                    el()
                }),
        );
    }
    c
}

fn next_quantize(q: Quantize) -> Quantize {
    let i = QUANTIZES.iter().position(|x| *x == q).unwrap_or(0);
    QUANTIZES[(i + 1) % QUANTIZES.len()]
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
    let cw = 74.0;
    let mut head = row().gap(4.0).child(
        el().w(90.0)
            .child(text("from ↓  to →").font(font::SMALL).color(theme::faint())),
    );
    for m in &p.game_modes {
        head = head.child(
            text(m.name.clone())
                .font(font::SMALL)
                .color(theme::mode_color(&m.color))
                .w(cw)
                .truncate(),
        );
    }
    let mut grid = col().gap(4.0).child(head);
    for from in &p.game_modes {
        let mut line = row().gap(4.0).child(
            text(from.name.clone())
                .font(font::LABEL)
                .color(theme::mode_color(&from.color))
                .w(90.0),
        );
        for to in &p.game_modes {
            if from.name == to.name {
                line = line.child(el().size(cw, 40.0).radius(8.0).fill(theme::panel()));
                continue;
            }
            let spec = p.transition_between(&from.name, &to.name);
            let exact = p.transitions.iter().any(|r| {
                r.from.as_deref() == Some(&from.name) && r.to.as_deref() == Some(&to.name)
            });
            let (f1, t1, f2, t2) = (
                from.name.clone(),
                to.name.clone(),
                from.name.clone(),
                to.name.clone(),
            );
            line = line.child(
                col()
                    .key(format!("tr-{}-{}", from.name, to.name))
                    .role("button")
                    .label(format!("{} to {}", from.name, to.name))
                    .help(format!("{} → {}: on the next {}, crossfade {} beats{}. Click: change the grid. Right-click: change the crossfade.", from.name, to.name, spec.quantize.name().to_lowercase(), spec.crossfade_beats, spec.stinger_event.as_ref().map(|e| format!(", stinger {e}")).unwrap_or_default()))
                    .size(cw, 40.0)
                    .justify_center()
                    .px(6.0)
                    .radius(8.0)
                    .fill(if exact { accent.alpha(0.16) } else { theme::card() })
                    .border(1.0, if exact { accent.alpha(0.6) } else { theme::line() })
                    .hover(theme::hover())
                    .press_scale(0.95)
                    .on_press(move |s: &mut App| s.edit(|p| set_rule(p, &f1, &t1, |sp| sp.quantize = next_quantize(sp.quantize))))
                    .on_secondary_press(move |s: &mut App, _, _| {
                        s.edit(|p| {
                            set_rule(p, &f2, &t2, |sp| {
                                let i = FADES.iter().position(|x| (*x - sp.crossfade_beats).abs() < 1e-3).unwrap_or(0);
                                sp.crossfade_beats = FADES[(i + 1) % FADES.len()];
                            })
                        })
                    })
                    .child(text(spec.quantize.name()).font(font::SMALL).color(theme::text()))
                    .child(text(format!("{}{}", if spec.crossfade_beats > 0.0 { format!("fade {}", spec.crossfade_beats) } else { "cut".into() }, if spec.stinger_event.is_some() { " ⚡" } else { "" })).font(font::SMALL).color(theme::faint())),
            );
        }
        grid = grid.child(line);
    }
    card().child(caps("Transitions")).child(crate::widgets::body("Each cell is “from row → to column”. Lit cells have their own rule; the rest use the default.")).child(grid)
}

fn target_max(t: CurveTarget) -> f32 {
    if t == CurveTarget::Detune { 100.0 } else { 1.0 }
}

fn curves(s: &App, accent: Color) -> El {
    let p = &s.project;
    let Some(track) = s.selected_track.and_then(|t| p.track(t)) else {
        return card()
            .child(caps("Response curves"))
            .child(crate::widgets::body("Pick a track on the Session screen."));
    };
    let tid = track.id;
    let color = theme::role_color(track.role);
    let mut picker = row().gap(4.0).flex_wrap();
    for t in &p.tracks {
        let id = t.id;
        picker = picker.child(
            crate::widgets::pill(
                format!("ctp-{id}"),
                t.name.clone(),
                theme::role_color(t.role),
                id == tid,
            )
            .on_press(move |s: &mut App| s.selected_track = Some(id)),
        );
    }
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
    let mut add = row().gap(4.0).flex_wrap().items_center().child(caps("Add"));
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
