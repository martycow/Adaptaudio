//! The pattern editor: a step grid for drums, a scale-locked piano roll for everything else,
//! and helpers that write notes for you.

use super::common::empty_state;
use crate::state::App;
use crate::theme::{self, font};
use crate::widgets::{button, caps, label, live_bar, screen_header, segmented, slider_row, toggle};
use aa_core::music::note_name;
use aa_core::{Clip, ClipId, Note, Project, TrackId};
use aa_engine::Command;
use aa_theory::{ArpPattern, BassStyle, Contour, Mood, Rng};
use rune::{Color, El, KeyEvent, col, el, row, text};

pub fn view(s: &App, accent: Color) -> El {
    col()
        .key("pattern-screen")
        .w_full()
        .h_full()
        .padding(22.0)
        .gap(16.0)
        .min_h(0.0)
        .child(screen_header("Pattern", "Click cells to add notes. Notes snap to the scale while Scale lock is on — no wrong notes.", accent))
        .child(col().grow(1.0).min_h(0.0).child(editor(s, accent, false)))
}

/// Rows of the grid, top to bottom.
fn rows_for(p: &Project, track: &aa_core::Track, clip: &Clip, compact: bool) -> Vec<u8> {
    if track.role.is_drum() {
        let mut pitches: Vec<u8> = clip.notes.iter().map(|n| n.pitch).collect();
        pitches.push(track.role.default_note());
        pitches.sort_unstable();
        pitches.dedup();
        pitches.reverse();
        return pitches;
    }
    let center = if clip.notes.is_empty() {
        track.role.default_note()
    } else {
        let sum: u32 = clip.notes.iter().map(|n| u32::from(n.pitch)).sum();
        (sum / clip.notes.len() as u32) as u8
    };
    let span: i32 = if compact { 9 } else { 14 };
    let lo = clip
        .notes
        .iter()
        .map(|n| n.pitch)
        .min()
        .unwrap_or(center)
        .min(center.saturating_sub(span as u8));
    let hi = clip
        .notes
        .iter()
        .map(|n| n.pitch)
        .max()
        .unwrap_or(center)
        .max(center.saturating_add(span as u8));
    (lo..=hi.min(127))
        .rev()
        .filter(|&n| {
            !p.scale_lock || p.scale.contains(p.key, n) || clip.notes.iter().any(|x| x.pitch == n)
        })
        .collect()
}

pub fn editor(s: &App, accent: Color, compact: bool) -> El {
    let p = &s.project;
    let (Some(tid), Some(cid)) = (s.selected_track, s.selected_clip) else {
        return empty_state("No clip selected", "Pick a clip on the Session screen.");
    };
    let Some(track) = p.track(tid) else {
        return empty_state("No track", "");
    };
    let Some(clip) = track.clip(cid) else {
        return empty_state(
            "This track has no clip yet",
            "Click a “+” cell on the Session screen to make one.",
        );
    };
    let ti = p.track_index(tid).unwrap_or(0);
    let color = theme::role_color(track.role);
    let rows = rows_for(p, track, clip, compact);
    let len = clip.length_steps;
    let cell_h: f32 = if compact { 15.0 } else { 20.0 };
    let cell_w: f32 = if len > 32 {
        20.0
    } else if compact {
        26.0
    } else {
        34.0
    };
    let label_w = 52.0;
    let grid_w = cell_w * f32::from(len);

    let mut grid = col().key("pgrid").gap(1.0);
    // Playhead
    if let Some(ph) = s.live.track_playhead.get(ti) {
        let playing_this = s.clip_playing.get(ti) == Some(&cid);
        if playing_this {
            grid = grid.child(
                row()
                    .child(el().w(label_w))
                    .child(live_bar(grid_w, 4.0, accent, ph)),
            );
        } else {
            grid = grid.child(el().h(4.0));
        }
    }
    for &pitch in &rows {
        let in_scale = p.scale.contains(p.key, pitch);
        let is_root = (i32::from(pitch) - i32::from(p.key.0)).rem_euclid(12) == 0;
        let mut line = row().key(format!("prow-{pitch}")).gap(0.0).child(
            row()
                .key(format!("plab-{pitch}"))
                .role("button")
                .label(format!("Play {}", note_name(pitch)))
                .w(label_w)
                .h(cell_h)
                .px(6.0)
                .items_center()
                .hover(theme::hover())
                .on_press(move |s: &mut App| {
                    s.send(Command::PreviewNote {
                        track: ti,
                        pitch,
                        velocity: 0.85,
                        seconds: 0.35,
                    })
                })
                .child(
                    text(if track.role.is_drum() {
                        format!("{} {}", track.role.name(), pitch)
                    } else {
                        note_name(pitch)
                    })
                    .font(font::SMALL.mono())
                    .color(if is_root {
                        accent
                    } else if in_scale {
                        theme::muted()
                    } else {
                        theme::faint()
                    }),
                ),
        );
        for step in 0..len {
            let note = clip
                .notes
                .iter()
                .position(|n| n.pitch == pitch && n.step == step);
            let covered = note.is_none()
                && clip.notes.iter().any(|n| {
                    n.pitch == pitch
                        && f32::from(n.step) < f32::from(step)
                        && f32::from(n.step) + n.length_steps > f32::from(step)
                });
            let beat = (step / 4) % 2 == 0;
            let base = if is_root {
                theme::card_hi()
            } else if beat {
                theme::card()
            } else {
                theme::panel()
            };
            let selected = note.is_some() && note == s.selected_note;
            let fill = match note {
                Some(i) => color.alpha(0.35 + 0.65 * clip.notes[i].velocity),
                None if covered => color.alpha(0.22),
                None => base,
            };
            let mut c = el()
                .key(format!("c-{pitch}-{step}"))
                .w(cell_w - 2.0)
                .h(cell_h - 1.0)
                .mx(1.0)
                .radius(4.0)
                .fill(fill)
                .hover(theme::hover())
                .on_press(move |s: &mut App| press_cell(s, tid, cid, step, pitch));
            if selected {
                c = c.border(1.5, theme::text());
            }
            if let Some(i) = note
                && clip.notes[i].probability < 0.999
            {
                c = c
                    .border_dash(2.0, 2.0)
                    .border(1.0, theme::text().alpha(0.6));
            }
            line = line.child(c);
        }
        grid = grid.child(line);
    }
    let grid = col()
        .grow(1.0)
        .min_h(0.0)
        .min_w(0.0)
        .scroll_xy()
        .child(grid);

    let audition = s.clip_playing.get(ti) == Some(&cid);
    let header = row()
        .gap(10.0)
        .items_center()
        .child(el().size(10.0, 10.0).radius(5.0).fill(color))
        .child(
            text(format!("{} · {}", track.name, clip.name))
                .font(font::TITLE)
                .color(theme::text()),
        )
        .child(label(format!("{} notes", clip.notes.len())))
        .child(rune::spacer())
        .child(caps("Length"))
        .child(segmented(
            "len",
            &["1 bar", "2 bars", "4 bars"],
            match len {
                0..=16 => 0,
                17..=32 => 1,
                _ => 2,
            },
            accent,
            move |s: &mut App, i| {
                let steps = [16u16, 32, 64][i];
                s.edit(|p| {
                    if let Some(c) = p.clip_mut(tid, cid) {
                        c.length_steps = steps;
                        c.notes.retain(|n| n.step < steps)
                    }
                });
            },
        ))
        .child(
            toggle(
                "scalelock",
                "Scale lock",
                p.scale_lock,
                accent,
                |s: &mut App| s.edit(|p| p.scale_lock = !p.scale_lock),
            )
            .help(crate::glossary::lookup("Scale").unwrap_or_default()),
        )
        .child(
            toggle(
                "audition",
                "Audition",
                s.ext.puzzle.audition == Some((ti, cid)),
                accent,
                move |s: &mut App| {
                    let on = s.ext.puzzle.audition == Some((ti, cid));
                    s.ext.puzzle.audition = if on { None } else { Some((ti, cid)) };
                    s.send(Command::SetClipOverride {
                        track: ti,
                        clip: if on { None } else { Some(cid) },
                    });
                    if !on && !s.playing {
                        s.toggle_play();
                    }
                },
            )
            .help("Play this clip in place of the section's clip on this track"),
        );
    let _ = audition;

    let mut out = col()
        .key("editor")
        .w_full()
        .h_full()
        .gap(10.0)
        .min_h(0.0)
        .child(header)
        .child(helpers(s, tid, cid, clip, accent));
    if compact {
        out = out.child(grid);
    } else {
        out = out.child(
            row()
                .grow(1.0)
                .min_h(0.0)
                .gap(14.0)
                .child(col().grow(1.0).min_w(0.0).min_h(0.0).child(grid))
                .child(inspector(s, tid, cid, clip, accent)),
        );
    }
    out
}

fn press_cell(s: &mut App, track: TrackId, clip: ClipId, step: u16, pitch: u8) {
    let Some(ti) = s.project.track_index(track) else {
        return;
    };
    let existing = s.project.clip(track, clip).and_then(|c| {
        c.notes
            .iter()
            .position(|n| n.step == step && n.pitch == pitch)
    });
    match existing {
        Some(i) if s.selected_note == Some(i) => {
            s.edit(|p| {
                if let Some(c) = p.clip_mut(track, clip) {
                    c.notes.remove(i);
                }
            });
            s.selected_note = None;
        }
        Some(i) => s.selected_note = Some(i),
        None => {
            let p = &s.project;
            let pitch = if p.scale_lock && !p.tracks[ti].role.is_drum() {
                p.scale.snap(p.key, pitch)
            } else {
                pitch
            };
            let mut index = None;
            s.edit(|p| {
                if let Some(c) = p.clip_mut(track, clip) {
                    c.notes.push(Note::new(step, pitch, 0.85, 1.0));
                    c.notes.sort_by_key(|n| n.step);
                    index = c
                        .notes
                        .iter()
                        .position(|n| n.step == step && n.pitch == pitch);
                }
            });
            s.selected_note = index;
            s.send(Command::PreviewNote {
                track: ti,
                pitch,
                velocity: 0.85,
                seconds: 0.25,
            });
        }
    }
}

fn inspector(s: &App, track: TrackId, clip: ClipId, c: &Clip, accent: Color) -> El {
    let mut panel = col()
        .key("note-inspector")
        .w(300.0)
        .gap(12.0)
        .padding(14.0)
        .radius(14.0)
        .fill(theme::card())
        .border(1.0, theme::line());
    panel = panel.child(caps("Clip"));
    panel = panel.child(slider_row(
        "swing",
        "Swing",
        c.swing * 2.0,
        format!("{:.0}%", c.swing * 200.0),
        120.0,
        accent,
        move |s: &mut App, v, start| {
            if start {
                s.begin_gesture();
            }
            s.edit_quiet(|p| {
                if let Some(c) = p.clip_mut(track, clip) {
                    c.swing = v * 0.5
                }
            });
        },
    ));
    panel = panel.child(caps("Selected note"));
    let Some(i) = s.selected_note.filter(|i| *i < c.notes.len()) else {
        return panel.child(crate::widgets::body(
            "Click a note to change its strength, length and chance. Click it again to delete it.",
        ));
    };
    let n = &c.notes[i];
    let edit_note = move |s: &mut App, start: bool, f: &dyn Fn(&mut Note)| {
        if start {
            s.begin_gesture();
        }
        s.edit_quiet(|p| {
            if let Some(note) = p.clip_mut(track, clip).and_then(|c| c.notes.get_mut(i)) {
                f(note)
            }
        });
    };
    panel
        .child(
            text(format!("{} at step {}", note_name(n.pitch), n.step + 1))
                .font(font::HEAD)
                .color(theme::text()),
        )
        .child(slider_row(
            "vel",
            "Velocity",
            n.velocity,
            format!("{:.0}", n.velocity * 127.0),
            120.0,
            accent,
            move |s: &mut App, v, st| edit_note(s, st, &|n| n.velocity = v.max(0.05)),
        ))
        .child(slider_row(
            "nlen",
            "Length",
            n.length_steps / 16.0,
            format!("{:.1} st", n.length_steps),
            120.0,
            accent,
            move |s: &mut App, v, st| {
                edit_note(s, st, &|n| {
                    n.length_steps = (v * 16.0 * 4.0).round() / 4.0 + 0.25
                })
            },
        ))
        .child(slider_row(
            "prob",
            "Probability",
            n.probability,
            format!("{:.0}%", n.probability * 100.0),
            120.0,
            accent,
            move |s: &mut App, v, st| {
                edit_note(s, st, &|n| n.probability = (v * 20.0).round() / 20.0)
            },
        ))
        .child(
            row()
                .gap(6.0)
                .child(
                    button("nup", "▲ up", accent, false)
                        .on_press(move |s: &mut App| shift_note(s, track, clip, i, 1)),
                )
                .child(
                    button("ndown", "▼ down", accent, false)
                        .on_press(move |s: &mut App| shift_note(s, track, clip, i, -1)),
                )
                .child(button("ndel", "Delete", theme::danger(), false).on_press(
                    move |s: &mut App| {
                        s.edit(|p| {
                            if let Some(c) = p.clip_mut(track, clip)
                                && i < c.notes.len()
                            {
                                c.notes.remove(i);
                            }
                        });
                        s.selected_note = None;
                    },
                )),
        )
}

fn shift_note(s: &mut App, track: TrackId, clip: ClipId, i: usize, dir: i32) {
    let (key, scale, lock) = (s.project.key, s.project.scale, s.project.scale_lock);
    s.edit(|p| {
        if let Some(n) = p.clip_mut(track, clip).and_then(|c| c.notes.get_mut(i)) {
            n.pitch = if lock {
                let idx = aa_theory::scale_index(key, scale, n.pitch);
                aa_theory::from_scale_index(key, scale, idx + dir)
            } else {
                (i32::from(n.pitch) + dir).clamp(0, 127) as u8
            };
        }
    });
}

/// The mood that fits the game mode being edited.
pub fn mood_for(s: &App) -> Mood {
    let name = s
        .mode
        .or(Some(s.selected_mode))
        .and_then(|m| s.project.game_modes.get(m))
        .map(|m| m.name.as_str())
        .unwrap_or("");
    match name {
        "Explore" => Mood::Curiosity,
        "Stealth" | "Puzzle" => Mood::Mystery,
        "Search" | "Alert" | "PlayDead" => Mood::Dread,
        "Chase" => Mood::Panic,
        "Relief" => Mood::Relief,
        _ => Mood::Cozy,
    }
}

fn helpers(s: &App, track: TrackId, clip: ClipId, c: &Clip, accent: Color) -> El {
    let role = s.project.track(track).map(|t| t.role);
    let drum = role.is_some_and(|r| r.is_drum());
    let len = c.length_steps;
    let mut r = row()
        .key("helpers")
        .gap(6.0)
        .items_center()
        .child(caps("Helpers"));
    if drum {
        for hits in [3u16, 5, 7] {
            r = r.child(
                button(
                    format!("euclid-{hits}"),
                    format!("Euclid {hits}/{}", len.min(16)),
                    accent,
                    false,
                )
                .help(crate::glossary::lookup("Euclidean rhythm").unwrap_or_default())
                .on_press(move |s: &mut App| {
                    let pitch = s.project.track(track).map_or(36, |t| t.role.default_note());
                    let base = aa_theory::euclid_clip("Euclid", hits, len.min(16), 0, pitch, 0.9);
                    let mut notes = Vec::new();
                    for rep in 0..(len / 16).max(1) {
                        for n in &base.notes {
                            notes.push(Note {
                                step: n.step + rep * 16,
                                ..n.clone()
                            });
                        }
                    }
                    replace_notes(s, track, clip, notes, &format!("Euclid {hits}"));
                }),
            );
        }
    } else {
        r = r
            .child(button("melody", "✦ Melody", accent, true).help("Write a melody in the current key and scale").on_press(move |s: &mut App| {
                let mut rng = next_rng(s);
                let contour = [Contour::Arch, Contour::Wave, Contour::Rising, Contour::Falling][(rng.next_u64() % 4) as usize];
                let m = aa_theory::generate_melody(s.project.key, s.project.scale, (len / 16).max(1), 0.55, contour, &mut rng);
                replace_notes(s, track, clip, m.notes, "Melody");
            }))
            .child(button("chords", "Chords", accent, false).help("Chords that fit the mood of the current game mode").on_press(move |s: &mut App| {
                let mut rng = next_rng(s);
                let prog = aa_theory::suggest_progression(mood_for(s), s.project.key, s.project.scale, &mut rng);
                let spc = (len / prog.len().max(1) as u16).max(4);
                let names: Vec<String> = prog.iter().map(|c| c.name.clone()).collect();
                let cl = aa_theory::chord_clip(&prog, spc);
                replace_notes(s, track, clip, cl.notes, &format!("Chords {}", names.join(" ")));
            }))
            .child(button("arp", "Arp", accent, false).help(crate::glossary::lookup("Arpeggio").unwrap_or_default()).on_press(move |s: &mut App| {
                let mut rng = next_rng(s);
                let prog = aa_theory::suggest_progression(mood_for(s), s.project.key, s.project.scale, &mut rng);
                let spc = (len / prog.len().max(1) as u16).max(4);
                let mut notes = Vec::new();
                for (i, ch) in prog.iter().enumerate() {
                    let lifted: Vec<u8> = ch.notes.iter().map(|n| n.saturating_add(12)).collect();
                    let a = aa_theory::arpeggiate(&lifted, ArpPattern::UpDown, spc, 1);
                    notes.extend(a.notes.into_iter().map(|n| Note { step: n.step + i as u16 * spc, ..n }));
                }
                replace_notes(s, track, clip, notes, "Arpeggio");
            }))
            .child(button("bass", "Bass", accent, false).help("A bass line under the mood's chords").on_press(move |s: &mut App| {
                let mut rng = next_rng(s);
                let prog = aa_theory::suggest_progression(mood_for(s), s.project.key, s.project.scale, &mut rng);
                let spc = (len / prog.len().max(1) as u16).max(4);
                let style = if mood_for(s) == Mood::Panic { BassStyle::DrivingEighths } else { BassStyle::Roots };
                let b = aa_theory::bass_line(&prog, style, spc);
                replace_notes(s, track, clip, b.notes, "Bass line");
            }))
            .child(button("vary", "Vary", accent, false).help("A variation of this motif: mirrored, reversed or shifted, still in the scale").on_press(move |s: &mut App| {
                let mut rng = next_rng(s);
                let Some(c) = s.project.clip(track, clip).cloned() else { return };
                let v = aa_theory::vary(&c, s.project.key, s.project.scale, &mut rng);
                let name = v.name.clone();
                replace_notes(s, track, clip, v.notes, &name);
            }));
    }
    r.child(
        button("clear", "Clear", theme::danger(), false)
            .on_press(move |s: &mut App| replace_notes(s, track, clip, Vec::new(), "Cleared")),
    )
}

fn next_rng(s: &mut App) -> Rng {
    s.ext.seed = s
        .ext
        .seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    Rng::new(s.ext.seed)
}

fn replace_notes(s: &mut App, track: TrackId, clip: ClipId, notes: Vec<Note>, what: &str) {
    s.edit(|p| {
        if let Some(c) = p.clip_mut(track, clip) {
            c.notes = notes
                .into_iter()
                .filter(|n| n.step < c.length_steps)
                .collect();
            c.notes.sort_by_key(|n| n.step);
        }
    });
    s.selected_note = None;
    s.toast(what);
}

pub fn on_key(_s: &mut App, _ev: &KeyEvent) -> bool {
    false
}
