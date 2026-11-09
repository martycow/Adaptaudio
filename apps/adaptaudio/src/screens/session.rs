//! Session: tracks × sections. Each cell is a clip; the game decides which column plays.

use super::common::clip_preview;
use crate::state::App;
use crate::theme::{self, font};
use crate::widgets::{caps, live_bar, live_meter, screen_header, slider, toggle, xyscroll};
use aa_core::{Clip, TrackRole};
use aa_engine::Command;
use rune::{Color, Drag, El, Phase, Tx, col, el, row, text};

const TRACK_W: f32 = 236.0;
const CELL_W: f32 = 148.0;
const CELL_H: f32 = 78.0;

pub fn view(s: &App, accent: Color) -> El {
    let p = &s.project;
    let mut grid = col().key("session-grid").gap(6.0);

    // Column headers: sections, with the modes that play them.
    let mut head = row().gap(6.0).child(
        el().w(TRACK_W)
            .h(44.0)
            .child(caps("Tracks / Sections"))
            .items_end()
            .pb(6.0),
    );
    for (si, sec) in p.sections.iter().enumerate() {
        let modes: Vec<_> = p
            .game_modes
            .iter()
            .filter(|m| m.section == Some(sec.id))
            .collect();
        let color = modes
            .first()
            .map_or(theme::muted(), |m| theme::mode_color(&m.color));
        let live = s.active_section == Some(sec.id) && s.playing;
        let id = sec.id;
        let mut h = col()
            .key(format!("sec-{si}"))
            .role("button")
            .label(format!("Launch {}", sec.name))
            .help(format!(
                "{} — {} bars. Click to launch on the next bar.",
                sec.name, sec.length_bars
            ))
            .w(CELL_W)
            .h(44.0)
            .px(10.0)
            .justify_center()
            .gap(2.0)
            .radius(10.0)
            .fill(if live {
                color.alpha(0.2)
            } else {
                theme::panel()
            })
            .border(1.0, if live { color } else { theme::line() })
            .hover(theme::hover())
            .press_scale(0.96)
            .on_press(move |s: &mut App| {
                s.send(Command::LaunchSection(id));
                if !s.playing && s.settings.play_on_launch {
                    s.set_playing(true);
                }
            })
            .child(
                row()
                    .gap(6.0)
                    .items_center()
                    .child(el().size(8.0, 8.0).radius(4.0).fill(color))
                    .child(text(sec.name.clone()).font(font::HEAD).color(theme::text())),
            );
        h = h.child(
            text(format!("{} bars · ▶", sec.length_bars))
                .font(font::SMALL)
                .color(theme::faint()),
        );
        if live {
            h = h.shadow(0.0, 18.0, color.alpha(0.35));
        }
        head = head.child(h);
    }
    grid = grid.child(head);

    for (ti, track) in p.tracks.iter().enumerate() {
        let mut line = row()
            .key(format!("trk-row-{}", track.id))
            .gap(6.0)
            .child(track_header(s, ti, accent));
        for sec in &p.sections {
            let clip = sec.clip_for(track.id).and_then(|c| track.clip(c));
            line = line.child(cell(s, ti, sec.id, clip, accent));
        }
        grid = grid.child(line);
    }
    grid = grid.child(add_track_row(accent));

    let mut out = col()
        .key("session")
        .w_full()
        .h_full()
        .padding(22.0)
        .pb(0.0)
        .gap(14.0)
        .min_h(0.0)
        .child(screen_header(
            "Session",
            "Columns are sections the game switches between. Rows are tracks. Click a clip to edit its notes below.",
            accent,
        ))
        .child(xyscroll(grid).pb(14.0));
    // The editor appears only while a clip is selected; the bar above it resizes it.
    if let Some(editor) = super::editor::editor(s, accent) {
        out = out.child(divider(accent)).child(
            col()
                .key("session-editor")
                .h(s.settings.editor_height)
                .min_h(0.0)
                .pt(12.0)
                .pb(16.0)
                .transition(rune::Transition::offset(0.0, 16.0))
                .child(editor),
        );
    }
    out
}

/// The line between the grid and the editor. Drag it to resize the editor.
fn divider(accent: Color) -> El {
    col()
        .key("session-divider")
        .role("separator")
        .label("Resize the editor")
        .help("Drag to resize the editor")
        .w_full()
        .h(11.0)
        .justify_center()
        .items_center()
        .cursor(rune::Cursor::ResizeRow)
        .hover(accent.alpha(0.12))
        .child(el().w_full().h(1.0).fill(theme::line_strong()))
        .child(el().size(44.0, 4.0).radius(2.0).fill(theme::faint()).pin(
            None,
            Some(3.5),
            None,
            None,
        ))
        .on_drag(|s: &mut App, d: &mut Drag, _: &mut Tx| {
            if d.phase == Phase::Down {
                d.data = s.settings.editor_height;
            }
            if d.cancelled() {
                s.settings.editor_height = d.data;
                return;
            }
            s.settings.editor_height = (d.data - d.dy).clamp(180.0, 640.0);
            if d.phase == Phase::Up {
                let _ = s.settings.save();
            }
        })
}

fn track_header(s: &App, index: usize, accent: Color) -> El {
    let t = &s.project.tracks[index];
    let color = theme::role_color(t.role);
    let selected = s.selected_track == Some(t.id);
    let id = t.id;
    let influence = t.adaptive.influence;
    let hit = s.live.track_hit.get(index);
    let level = s.live.track_level.get(index);
    let mut dot = el().size(9.0, 9.0).radius(4.5).fill(color);
    if let Some(h) = hit {
        dot = dot.live_opacity(h);
    }
    let meter = level.map_or_else(el, |l| live_meter(5.0, CELL_H - 16.0, color, l));
    row()
        .key(format!("trk-{id}"))
        .w(TRACK_W)
        .h(CELL_H)
        .px(10.0)
        .gap(10.0)
        .items_center()
        .radius(12.0)
        .fill(if selected {
            theme::card_hi()
        } else {
            theme::card()
        })
        .border(
            1.0,
            if selected {
                accent.alpha(0.7)
            } else {
                theme::line()
            },
        )
        .on_press(move |s: &mut App| {
            if s.selected_track != Some(id) {
                s.select_clip(id, None);
            }
        })
        .child(el().w(4.0).h(CELL_H - 18.0).radius(2.0).fill(color))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(
                    row().gap(6.0).items_center().child(dot).child(
                        text(t.name.clone())
                            .font(font::HEAD)
                            .color(theme::text())
                            .truncate(),
                    ),
                )
                .child(
                    row()
                        .gap(6.0)
                        .items_center()
                        .child(
                            {
                                let external = matches!(t.instrument, aa_core::Instrument::External(_));
                                let label = text(format!("{} · {}", t.role.name(), t.instrument.kind_name()))
                                    .font(font::SMALL)
                                    .color(if external { theme::warn() } else { theme::faint() })
                                    .truncate()
                                    .max_w(92.0);
                                if external {
                                    label.help("Prototype only: a hardware synth can't go into the game. Capture it into a sampler before exporting to FMOD.")
                                } else {
                                    label
                                }
                            },
                        )
                        .child(rune::spacer())
                        .child(
                            toggle(
                                format!("m-{id}"),
                                "M",
                                t.mixer.mute,
                                theme::warn(),
                                move |s: &mut App| {
                                    s.edit(|p| {
                                        if let Some(t) = p.track_mut(id) {
                                            t.mixer.mute = !t.mixer.mute
                                        }
                                    })
                                },
                            )
                            .help("Mute"),
                        )
                        .child(
                            toggle(
                                format!("s-{id}"),
                                "S",
                                t.mixer.solo,
                                theme::ok(),
                                move |s: &mut App| {
                                    s.edit(|p| {
                                        if let Some(t) = p.track_mut(id) {
                                            t.mixer.solo = !t.mixer.solo
                                        }
                                    })
                                },
                            )
                            .help("Solo"),
                        ),
                )
                .child(
                    row()
                        .gap(6.0)
                        .items_center()
                        .help(crate::glossary::lookup("Influence").unwrap_or_default())
                        .child(text("Influence").font(font::SMALL).color(theme::faint()))
                        .child(slider(
                            format!("infl-{id}"),
                            influence,
                            92.0,
                            accent,
                            move |s: &mut App, v, start| {
                                if start {
                                    s.begin_gesture();
                                }
                                s.edit_quiet(|p| {
                                    if let Some(t) = p.track_mut(id) {
                                        t.adaptive.influence = (v * 20.0).round() / 20.0
                                    }
                                });
                            },
                        )),
                ),
        )
        .child(meter)
}

fn cell(s: &App, index: usize, section: u32, clip: Option<&Clip>, accent: Color) -> El {
    let track = &s.project.tracks[index];
    let color = theme::role_color(track.role);
    let tid = track.id;
    let role = track.role;
    let track_name = track.name.clone();
    let section_name = s
        .project
        .section(section)
        .map(|x| x.name.clone())
        .unwrap_or_default();
    let key = format!("cell-{tid}-{section}");
    match clip {
        None => row()
            .key(key)
            .role("button")
            .label(format!("New clip for {track_name} in {section_name}"))
            .w(CELL_W)
            .h(CELL_H)
            .center()
            .radius(10.0)
            .border(1.0, theme::line())
            .border_dash(4.0, 4.0)
            .hover(theme::hover())
            .on_press(move |s: &mut App| {
                new_clip(s, tid, section, role, &track_name, &section_name)
            })
            .child(text("+").font(font::TITLE).color(theme::faint())),
        Some(c) => {
            let cid = c.id;
            let selected = s.selected_clip == Some(cid) && s.selected_track == Some(tid);
            let playing = s.playing
                && s.active_section == Some(section)
                && s.clip_playing.get(index) == Some(&cid);
            let mut card = col()
                .key(key)
                .role("button")
                .label(c.name.clone())
                .help(format!("{} — {} steps, {} notes. Click to edit its notes; right-click to take it out of this section.", c.name, c.length_steps, c.notes.len()))
                .w(CELL_W)
                .h(CELL_H)
                .px(10.0)
                .py(8.0)
                .gap(5.0)
                .radius(12.0)
                .fill(if playing { color.alpha(0.13) } else { theme::card() })
                .border(if selected { 1.5 } else { 1.0 }, if selected { accent } else if playing { color.alpha(0.8) } else { theme::line() })
                .hover(theme::hover())
                .press_scale(0.97)
                .motion(rune::tok::motion::SNAPPY)
                .on_press(move |s: &mut App| {
                    // A second click on the open clip closes the editor.
                    let open = s.selected_track == Some(tid) && s.selected_clip == Some(cid);
                    s.select_clip(tid, (!open).then_some(cid));
                })
                .on_secondary_press(move |s: &mut App, _, _| s.edit(|p| p.set_slot(section, tid, None)))
                .child(text(c.name.clone()).font(font::LABEL).color(theme::text()).truncate())
                .child(clip_preview(c, CELL_W - 20.0, 16.0, color));
            if playing {
                card = card.shadow(0.0, 16.0, color.alpha(0.4));
                if let Some(progress) = s.live.track_progress.get(index) {
                    card = card.child(live_bar(CELL_W - 20.0, 3.0, color, progress));
                }
            } else {
                card = card.child(el().h(3.0));
            }
            card
        }
    }
}

fn new_clip(
    s: &mut App,
    track: u32,
    section: u32,
    role: TrackRole,
    track_name: &str,
    section_name: &str,
) {
    let len = if role.is_drum() { 16 } else { 32 };
    let name = format!("{track_name} · {section_name}");
    let mut new_id = None;
    s.edit(|p| {
        let clip = Clip::new(name, len);
        if let Some(id) = p.add_clip(track, clip) {
            p.set_slot(section, track, Some(id));
            new_id = Some(id);
        }
    });
    s.select_clip(track, new_id);
}

fn add_track_row(accent: Color) -> El {
    let mut r = row()
        .key("add-track")
        .gap(6.0)
        .pt(6.0)
        .items_center()
        .child(caps("Add track").w(90.0));
    for role in TrackRole::ALL {
        r = r.child(
            row()
                .key(format!("add-{}", role.name()))
                .role("button")
                .label(format!("Add {} track", role.name()))
                .h(26.0)
                .px(10.0)
                .center()
                .gap(6.0)
                .radius(13.0)
                .fill(theme::card())
                .border(1.0, theme::line())
                .hover(theme::hover())
                .press_scale(0.94)
                .on_press(move |s: &mut App| {
                    let mut id = 0;
                    s.edit(|p| id = p.add_track(role.name(), role));
                    s.select_clip(id, None);
                })
                .child(
                    el().size(7.0, 7.0)
                        .radius(3.5)
                        .fill(theme::role_color(role)),
                )
                .child(text(role.name()).font(font::SMALL).color(theme::muted())),
        );
    }
    let _ = accent;
    r
}
