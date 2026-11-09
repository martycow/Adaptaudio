//! Puzzle Lab: design rhythm puzzles and play-test them against the music's beat.
//!
//! Judging uses `aa-puzzle`, the same rules the Unity port follows (checked by the exported
//! test vectors). Here time comes from the UI clock, so it is a prototype: accurate to a frame.

use crate::state::App;
use crate::theme::{self, font};
use crate::widgets::{body, button, caps, card, screen_header};
use aa_core::{ClipId, PuzzleKind, TrackRole};
use aa_engine::Command;
use aa_puzzle::{EchoSession, Judgement, PatternSession, PressResult, TimingSession};
use rune::{Color, El, Key, KeyEvent, col, el, row, text};
use std::time::{Duration, Instant};

const COUNT_IN: f64 = 2.0;
const LANE_KEYS: [char; 4] = ['f', 'j', 'd', 'k'];

pub enum Play {
    Timing(TimingSession),
    Pattern(PatternSession),
    Echo(EchoSession),
}

#[derive(Default)]
pub struct PuzzlePlay {
    pub audition: Option<(usize, ClipId)>,
    pub play: Option<Play>,
    pub puzzle: usize,
    pub start: Option<Instant>,
    /// Sounds to play: (ms since start, track, pitch).
    pub queue: Vec<(f64, usize, u8)>,
    pub last: Option<PressResult>,
    pub history: Vec<PressResult>,
    pub done_reported: bool,
}

fn ms_since(start: Option<Instant>) -> f64 {
    start.map_or(0.0, |s| {
        Instant::now().saturating_duration_since(s).as_secs_f64() * 1000.0 - COUNT_IN * 1000.0
    })
}

fn sound_tracks(s: &App) -> (usize, usize) {
    let p = &s.project;
    let find = |pred: &dyn Fn(&aa_core::Track) -> bool| p.tracks.iter().position(pred);
    let click = find(&|t| t.role == TrackRole::Perc)
        .or_else(|| find(&|t| t.role.is_drum()))
        .unwrap_or(0);
    let melodic = find(&|t| t.name == "Bells")
        .or_else(|| find(&|t| matches!(t.role, TrackRole::Lead | TrackRole::Arp)))
        .unwrap_or(0);
    (click, melodic)
}

fn paranoia(s: &App) -> f32 {
    s.project
        .param_index("Paranoia")
        .map_or(0.0, |i| s.controller.telemetry.params[i].get())
}

pub fn start(s: &mut App, index: usize) {
    let Some(z) = s.project.puzzles.get(index).cloned() else {
        return;
    };
    let bpm = s.project.tempo_bpm;
    let par = paranoia(s);
    let (click, melodic) = sound_tracks(s);
    let mut queue = Vec::new();
    let beat_ms = 60_000.0 / f64::from(bpm);
    for k in 0..(COUNT_IN * 1000.0 / beat_ms) as i32 {
        queue.push((-COUNT_IN * 1000.0 + f64::from(k) * beat_ms, click, 76));
    }
    let play = match &z.kind {
        PuzzleKind::Timing { .. } => TimingSession::new(&z, bpm).map(|t| {
            for i in 0..t.targets().len() {
                if let Some(ms) = t.heard_time_ms(i, par) {
                    queue.push((ms, click, 75));
                }
            }
            Play::Timing(t)
        }),
        PuzzleKind::Pattern { .. } => PatternSession::new(&z).map(Play::Pattern),
        PuzzleKind::Echo { .. } => EchoSession::new(&z, bpm).map(|e| {
            for (ms, _lane, pitch) in e.call_notes_heard(par) {
                queue.push((ms, melodic, pitch));
            }
            Play::Echo(e)
        }),
    };
    s.ext.puzzle = PuzzlePlay {
        audition: s.ext.puzzle.audition,
        play,
        puzzle: index,
        start: Some(Instant::now()),
        queue,
        last: None,
        history: Vec::new(),
        done_reported: false,
    };
    if let Some(m) = s.project.mode_index("Puzzle") {
        s.request_mode(m);
    }
}

pub fn poll(s: &mut App, _now: f64, tx: &mut rune::Tx) -> bool {
    if s.ext.puzzle.start.is_none() {
        return false;
    }
    let now = ms_since(s.ext.puzzle.start);
    let due: Vec<(f64, usize, u8)> = s
        .ext
        .puzzle
        .queue
        .iter()
        .filter(|(t, _, _)| *t <= now)
        .copied()
        .collect();
    s.ext.puzzle.queue.retain(|(t, _, _)| *t > now);
    for (_, track, pitch) in &due {
        s.send(Command::PreviewNote {
            track: *track,
            pitch: *pitch,
            velocity: 0.9,
            seconds: 0.2,
        });
    }
    let mut changed = !due.is_empty();
    let over = match &mut s.ext.puzzle.play {
        Some(Play::Timing(t)) => {
            if !t.finish(now).is_empty() {
                changed = true;
            }
            t.is_over()
        }
        Some(Play::Echo(e)) => {
            if !e.finish(now).is_empty() {
                changed = true;
            }
            e.is_over()
        }
        _ => false,
    };
    if over && !s.ext.puzzle.done_reported {
        s.ext.puzzle.done_reported = true;
        let solved = match &s.ext.puzzle.play {
            Some(Play::Timing(t)) => t.is_solved(),
            Some(Play::Echo(e)) => e.is_solved(),
            _ => false,
        };
        finish(s, solved);
        changed = true;
    }
    if !over || !s.ext.puzzle.queue.is_empty() {
        tx.wake_after(Duration::from_millis(8));
    }
    changed
}

fn finish(s: &mut App, solved: bool) {
    let progress = progress_of(s);
    if let Some(i) = s.project.param_index("PuzzleProgress") {
        s.set_param(i, progress);
    }
    if solved {
        if let Some(e) = s.project.event_index("PuzzleSolved") {
            s.trigger_event(e);
        }
        s.toast("Solved! The music unlocks a layer.");
    } else {
        if let Some(e) = s.project.event_index("PuzzleFail") {
            s.trigger_event(e);
        }
        s.toast_error("Not this time — try again");
    }
}

fn progress_of(s: &App) -> f32 {
    match &s.ext.puzzle.play {
        Some(Play::Timing(t)) => t.progress(),
        Some(Play::Echo(e)) => e.progress(),
        Some(Play::Pattern(p)) => p.progress(),
        None => 0.0,
    }
}

pub fn press(s: &mut App, lane: u8) {
    let now = ms_since(s.ext.puzzle.start);
    let par = paranoia(s);
    let (click, melodic) = sound_tracks(s);
    let result = match &mut s.ext.puzzle.play {
        Some(Play::Timing(t)) => Some((t.press(now, lane, par), click, 75 + lane)),
        Some(Play::Echo(e)) => {
            let pitch = e
                .targets()
                .iter()
                .find(|x| x.lane == lane)
                .map_or(74, |x| x.pitch);
            Some((e.press(now, lane, par), melodic, pitch))
        }
        _ => None,
    };
    if let Some((r, track, pitch)) = result {
        // A hit plays a note in the scale; a mistake rubs.
        let pitch = if r.judgement.is_hit() {
            pitch
        } else {
            pitch.saturating_add(1)
        };
        s.send(Command::PreviewNote {
            track,
            pitch,
            velocity: if r.judgement.is_hit() { 0.95 } else { 0.5 },
            seconds: 0.2,
        });
        if let Some(i) = s.project.param_index("PuzzleProgress") {
            let p = progress_of(s);
            s.set_param(i, p);
        }
        s.ext.puzzle.last = Some(r);
        s.ext.puzzle.history.push(r);
    }
}

pub fn on_key(s: &mut App, ev: &KeyEvent) -> bool {
    if s.screen != crate::state::Screen::Puzzle || s.ext.puzzle.start.is_none() {
        return false;
    }
    if let Key::Text(t) = &ev.key
        && let Some(lane) = t
            .chars()
            .next()
            .and_then(|c| LANE_KEYS.iter().position(|k| *k == c.to_ascii_lowercase()))
    {
        press(s, lane as u8);
        return true;
    }
    false
}

fn judgement_color(j: Judgement) -> Color {
    match j {
        Judgement::Perfect => theme::ok(),
        Judgement::Good => theme::moss(),
        Judgement::Early | Judgement::Late => theme::warn(),
        _ => theme::danger(),
    }
}

pub fn view(s: &App, accent: Color) -> El {
    let p = &s.project;
    let mut list = row().gap(6.0).flex_wrap();
    for (i, z) in p.puzzles.iter().enumerate() {
        list = list.child(
            crate::widgets::pill(
                format!("pz-{i}"),
                format!("{} · {}", z.kind.name(), z.name),
                accent,
                s.selected_puzzle == i,
            )
            .on_press(move |s: &mut App| {
                s.selected_puzzle = i;
                s.ext.puzzle.play = None;
                s.ext.puzzle.start = None;
            }),
        );
    }
    let body_el = match p.puzzles.get(s.selected_puzzle) {
        None => body("No puzzles in this project."),
        Some(z) => {
            let mut c = card()
                .child(
                    row()
                        .gap(10.0)
                        .items_center()
                        .child(text(z.name.clone()).font(font::TITLE).color(theme::text()))
                        .child(
                            text(aa_puzzle::describe(z))
                                .font(font::BODY)
                                .color(theme::muted())
                                .wrap(),
                        ),
                )
                .child(
                    row()
                        .gap(14.0)
                        .child(
                            text(format!("Window ±{:.0} ms", z.window_ms))
                                .font(font::LABEL)
                                .color(theme::muted()),
                        )
                        .child(
                            text(format!(
                                "Paranoia shift up to {:.0} ms",
                                z.paranoia_shift_ms
                            ))
                            .font(font::LABEL)
                            .color(theme::muted()),
                        )
                        .child(
                            text(format!("Paranoia now {:.2}", paranoia(s)))
                                .font(font::LABEL)
                                .color(theme::mode_color("#c77dff")),
                        ),
                );
            let i = s.selected_puzzle;
            c = c.child(
                row()
                    .gap(8.0)
                    .child(
                        button(
                            "pz-start",
                            if s.ext.puzzle.start.is_some() {
                                "↻ Restart"
                            } else {
                                "▶ Play-test"
                            },
                            accent,
                            true,
                        )
                        .on_press(move |s: &mut App| start(s, i)),
                    )
                    .child(
                        button("pz-stop", "Stop", accent, false).on_press(|s: &mut App| {
                            s.ext.puzzle.start = None;
                            s.ext.puzzle.play = None;
                        }),
                    ),
            );
            c = c.child(match &s.ext.puzzle.play {
                Some(Play::Timing(t)) => timing_view(s, t, accent),
                Some(Play::Echo(e)) => echo_view(s, e, accent),
                Some(Play::Pattern(pt)) => pattern_view(s, pt, accent),
                None => body("Press Play-test. Timing and Echo use the keys F, J, D, K (or click the pads) — two beats of count-in first."),
            });
            c
        }
    };
    let export_note = card().child(caps("For Unity")).child(body("Export (⇥ in the top bar) writes puzzles.json and puzzle_vectors.json: the C# port replays the vectors to prove it judges exactly like this lab."));
    col()
        .key("puzzle")
        .w_full()
        .h_full()
        .padding(22.0)
        .gap(16.0)
        .min_h(0.0)
        .child(screen_header("Puzzle Lab", "Rhythm puzzles are musical phrases. Solve one and the music grows; paranoia makes the beat lie.", accent))
        .child(list)
        .child(col().grow(1.0).min_h(0.0).scroll(rune::el::ScrollSpec { stick_bottom: false }).child(col().gap(14.0).child(body_el).child(export_note)))
}

fn pads(lanes: u8, accent: Color) -> El {
    let mut r = row().gap(10.0);
    for lane in 0..lanes {
        r = r.child(
            col()
                .key(format!("pad-{lane}"))
                .role("button")
                .label(format!("Lane {}", lane + 1))
                .size(96.0, 96.0)
                .center()
                .gap(4.0)
                .radius(18.0)
                .fill(theme::card_hi())
                .border(1.5, accent.alpha(0.6))
                .hover(accent.alpha(0.15))
                .press(accent.alpha(0.45))
                .press_scale(0.9)
                .on_press(move |s: &mut App| press(s, lane))
                .child(
                    text(
                        LANE_KEYS
                            .get(lane as usize)
                            .map_or("?".into(), |c| c.to_ascii_uppercase().to_string()),
                    )
                    .font(font::HERO)
                    .color(theme::text()),
                )
                .child(
                    text(format!("lane {}", lane + 1))
                        .font(font::SMALL)
                        .color(theme::faint()),
                ),
        );
    }
    r
}

fn verdict(
    s: &App,
    progress: f32,
    solved: bool,
    over: bool,
    mistakes: u32,
    fooled: u32,
    accent: Color,
) -> El {
    let mut c = row().gap(14.0).items_center();
    c = c.child(
        text(format!("{:.0}%", progress * 100.0))
            .font(font::NUM_BIG)
            .color(accent),
    );
    if let Some(r) = s.ext.puzzle.last {
        c = c.child(
            text(format!("{} {:+.0} ms", r.judgement.name(), r.offset_ms))
                .font(font::HEAD)
                .color(judgement_color(r.judgement)),
        );
    }
    c = c.child(
        text(format!("mistakes {mistakes} · fooled by paranoia {fooled}"))
            .font(font::LABEL)
            .color(theme::muted()),
    );
    if over {
        c = c.child(
            text(if solved { "SOLVED" } else { "FAILED" })
                .font(font::TITLE)
                .color(if solved { theme::ok() } else { theme::danger() }),
        );
    }
    c
}

fn timing_view(s: &App, t: &TimingSession, accent: Color) -> El {
    let total = t.targets().last().map_or(1.0, |x| x.time_ms).max(1.0) + 500.0;
    let w = 640.0;
    let mut lane_rows = col().gap(6.0);
    for lane in 0..t.lane_count() {
        let mut line = el().size(w, 26.0).radius(8.0).fill(theme::panel());
        for target in t.targets().iter().filter(|x| x.lane == lane) {
            let x = (target.time_ms / total) as f32 * w;
            let color = target.judgement.map_or(accent, judgement_color);
            line = line.child(
                el().size(14.0, 14.0)
                    .radius(7.0)
                    .fill(color)
                    .absolute(x - 7.0, 6.0),
            );
        }
        lane_rows = lane_rows.child(
            row()
                .gap(8.0)
                .items_center()
                .child(
                    text(
                        LANE_KEYS[lane as usize % 4]
                            .to_ascii_uppercase()
                            .to_string(),
                    )
                    .font(font::HEAD)
                    .color(theme::muted())
                    .w(16.0),
                )
                .child(line),
        );
    }
    col()
        .gap(12.0)
        .child(verdict(
            s,
            t.progress(),
            t.is_solved(),
            t.is_over(),
            t.mistakes(),
            t.fooled(),
            accent,
        ))
        .child(lane_rows)
        .child(pads(t.lane_count(), accent))
}

fn echo_view(s: &App, e: &EchoSession, accent: Color) -> El {
    let lanes = e
        .targets()
        .iter()
        .map(|x| x.lane)
        .max()
        .map_or(1, |m| m + 1);
    let mut seq = row().gap(6.0);
    for (i, target) in e.targets().iter().enumerate() {
        let done = i < e.next_index();
        seq = seq.child(
            col()
                .size(46.0, 46.0)
                .center()
                .radius(10.0)
                .fill(if done {
                    accent.alpha(0.3)
                } else {
                    theme::panel()
                })
                .border(1.0, theme::line())
                .child(
                    text(
                        LANE_KEYS[target.lane as usize % 4]
                            .to_ascii_uppercase()
                            .to_string(),
                    )
                    .font(font::HEAD)
                    .color(if done { accent } else { theme::faint() }),
                ),
        );
    }
    col()
        .gap(12.0)
        .child(verdict(
            s,
            e.progress(),
            e.is_solved(),
            e.is_over(),
            e.mistakes(),
            e.fooled(),
            accent,
        ))
        .child(body(format!(
            "Listen to the call, then answer from {:.1} s — same notes, same rhythm.",
            e.answer_start_ms() / 1000.0
        )))
        .child(seq)
        .child(pads(lanes, accent))
}

fn pattern_view(s: &App, pt: &PatternSession, accent: Color) -> El {
    let mut grid = col().gap(4.0);
    for lane in 0..pt.lanes() {
        let mut line = row().gap(4.0);
        for step in 0..pt.steps() {
            let on = pt.is_on(lane, step);
            line = line.child(
                el().key(format!("pc-{lane}-{step}"))
                    .size(40.0, 40.0)
                    .radius(9.0)
                    .fill(if on {
                        accent
                    } else if step % 4 == 0 {
                        theme::card_hi()
                    } else {
                        theme::panel()
                    })
                    .border(1.0, theme::line())
                    .hover(theme::hover())
                    .press_scale(0.92)
                    .on_press(move |s: &mut App| {
                        let mut solved = false;
                        if let Some(Play::Pattern(p)) = &mut s.ext.puzzle.play {
                            p.toggle(lane, step);
                            solved = p.is_solved();
                        }
                        let (_, melodic) = sound_tracks(s);
                        let pitch = s
                            .project
                            .puzzles
                            .get(s.ext.puzzle.puzzle)
                            .and_then(|z| match &z.kind {
                                PuzzleKind::Pattern { pitches, .. } => {
                                    pitches.get(lane as usize).copied()
                                }
                                _ => None,
                            })
                            .unwrap_or(60);
                        s.send(Command::PreviewNote {
                            track: melodic,
                            pitch,
                            velocity: 0.8,
                            seconds: 0.2,
                        });
                        if let Some(i) = s.project.param_index("PuzzleProgress") {
                            let p = progress_of(s);
                            s.set_param(i, p);
                        }
                        if solved && !s.ext.puzzle.done_reported {
                            s.ext.puzzle.done_reported = true;
                            finish(s, true);
                        }
                    }),
            );
        }
        grid = grid.child(line);
    }
    let listen = button("pz-listen", "♫ Listen", accent, false).on_press(|s: &mut App| {
        let Some(Play::Pattern(p)) = &s.ext.puzzle.play else {
            return;
        };
        let notes = p.notes_for_playback();
        let step_ms = 60_000.0 / f64::from(s.project.tempo_bpm) / 4.0;
        let now = ms_since(s.ext.puzzle.start);
        let (_, melodic) = sound_tracks(s);
        for (step, pitch) in notes {
            s.ext.puzzle.queue.push((
                now + 100.0 + f64::from(step) * step_ms * 2.0,
                melodic,
                pitch,
            ));
        }
    });
    let hint = button("pz-hint", "Hint", accent, false).on_press(|s: &mut App| {
        if let Some(Play::Pattern(p)) = &s.ext.puzzle.play
            && let Some([lane, step]) = p.hint()
        {
            s.toast(&format!("Try lane {} at step {}", lane + 1, step + 1));
        }
    });
    col()
        .gap(12.0)
        .child(verdict(
            s,
            pt.progress(),
            pt.is_solved(),
            pt.is_solved(),
            0,
            0,
            accent,
        ))
        .child(grid)
        .child(row().gap(8.0).child(listen).child(hint))
}
