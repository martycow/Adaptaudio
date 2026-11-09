//! Sound Lab: the built-in synth, sampler zones and hardware routing for the selected track.

use super::common::empty_state;
use crate::state::App;
use crate::theme::{self, font};
use crate::widgets::{button, caps, knob, screen_header, segmented};
use aa_core::{Adsr, FilterMode, Instrument, LfoTarget, SynthPatch, TrackId, TrackRole, Waveform};
use aa_engine::Command;
use rune::{Color, El, Shape, canvas, col, el, row, text};

pub fn view(s: &App, accent: Color) -> El {
    let head = screen_header(
        "Sound Lab",
        "Shape how each note sounds. Hover any word to learn what it does.",
        accent,
    );
    let Some(track) = s.selected_track.and_then(|t| s.project.track(t)) else {
        return col().padding(22.0).gap(16.0).child(head).child(empty_state(
            "No track selected",
            "Pick a track on the Session screen.",
        ));
    };
    let tid = track.id;
    let ti = s.project.track_index(tid).unwrap_or(0);
    let kinds = ["Synth", "Sampler", "External MIDI"];
    let current = match track.instrument {
        Instrument::Synth(_) => 0,
        Instrument::Sampler(_) => 1,
        Instrument::External(_) => 2,
    };
    let role = track.role;
    let top = row()
        .gap(12.0)
        .items_center()
        .child(
            el().size(12.0, 12.0)
                .radius(6.0)
                .fill(theme::role_color(track.role)),
        )
        .child(
            text(track.name.clone())
                .font(font::TITLE)
                .color(theme::text()),
        )
        .child(track_picker(s, accent))
        .child(rune::spacer())
        .child(segmented(
            "ikind",
            &kinds,
            current,
            accent,
            move |s: &mut App, i| {
                s.edit(|p| {
                    if let Some(t) = p.track_mut(tid) {
                        t.instrument = match i {
                            0 => Instrument::Synth(SynthPatch::preset(role)),
                            1 => Instrument::Sampler(aa_core::SamplerInstrument {
                                name: "Empty sampler".into(),
                                zones: Vec::new(),
                                amp_env: Adsr::new(0.002, 0.3, 1.0, 0.3),
                                gain_db: 0.0,
                            }),
                            _ => Instrument::External(aa_core::ExternalInstrument {
                                port_name: s_midi_default(),
                                channel: 1,
                                latency_ms: 0.0,
                            }),
                        };
                    }
                });
            },
        ));
    let body = match &track.instrument {
        Instrument::Synth(patch) => synth(tid, ti, role, patch, accent),
        Instrument::Sampler(inst) => col()
            .gap(10.0)
            .child(caps(format!("{} · {} zones", inst.name, inst.zones.len())))
            .child(crate::widgets::body("Sampler zones come from the Capture screen (record a hardware synth) or from WAV files in the project's samples folder."))
            .children(inst.zones.iter().map(|z| text(format!("{}  {}–{}  root {}", z.path, aa_core::music::note_name(z.low_note), aa_core::music::note_name(z.high_note), aa_core::music::note_name(z.root_note))).font(font::SMALL.mono()).color(theme::muted()))),
        Instrument::External(ext) => col()
            .gap(10.0)
            .child(caps("Hardware synth over MIDI"))
            .child(text(format!("Port: {} · channel {} · latency {:.1} ms", if ext.port_name.is_empty() { "(none)" } else { &ext.port_name }, ext.channel, ext.latency_ms)).font(font::BODY).color(theme::text()))
            .child(crate::widgets::body("Notes go to the synth live. To hear it in the game, record it into a sampler on the Capture screen.")),
    };
    col()
        .key("sound")
        .w_full()
        .h_full()
        .padding(22.0)
        .gap(16.0)
        .min_h(0.0)
        .child(head)
        .child(top)
        .child(
            col()
                .grow(1.0)
                .min_h(0.0)
                .scroll(rune::el::ScrollSpec {
                    stick_bottom: false,
                })
                .child(body),
        )
        .child(keyboard(ti, accent))
}

fn s_midi_default() -> String {
    String::new()
}

fn track_picker(s: &App, accent: Color) -> El {
    let mut r = row().gap(4.0);
    for t in &s.project.tracks {
        let id = t.id;
        let on = s.selected_track == Some(id);
        r = r.child(
            crate::widgets::pill(
                format!("tp-{id}"),
                t.name.clone(),
                theme::role_color(t.role),
                on,
            )
            .on_press(move |s: &mut App| {
                let first = s
                    .project
                    .track(id)
                    .and_then(|t| t.clips.first())
                    .map(|c| c.id);
                s.select_clip(id, first);
            }),
        );
    }
    let _ = accent;
    row().min_w(0.0).max_w(640.0).scroll_x().child(r)
}

/// Edits the selected track's synth patch.
fn patch_edit(s: &mut App, tid: TrackId, start: bool, f: impl FnOnce(&mut SynthPatch)) {
    if start {
        s.begin_gesture();
    }
    s.edit_quiet(|p| {
        if let Some(Instrument::Synth(patch)) = p.track_mut(tid).map(|t| &mut t.instrument) {
            f(patch);
        }
    });
}

/// Log mapping for Hz knobs.
fn hz_to_unit(hz: f32) -> f32 {
    ((hz.max(20.0) / 20.0).ln() / (1000.0f32).ln()).clamp(0.0, 1.0)
}
fn unit_to_hz(u: f32) -> f32 {
    20.0 * 1000f32.powf(u)
}
/// Seconds 0.001..10 on a log scale.
fn sec_to_unit(s: f32) -> f32 {
    ((s.max(0.001) / 0.001).ln() / 10_000f32.ln()).clamp(0.0, 1.0)
}
fn unit_to_sec(u: f32) -> f32 {
    0.001 * 10_000f32.powf(u)
}

fn synth(tid: TrackId, ti: usize, role: TrackRole, p: &SynthPatch, accent: Color) -> El {
    let osc = |i: usize| {
        let o = p.osc[i];
        let wave = Waveform::ALL.iter().position(|w| *w == o.wave).unwrap_or(0);
        crate::widgets::card()
            .child(caps(format!("Oscillator {}", i + 1)))
            .child(segmented(
                &format!("wave{i}"),
                &["Sine", "Tri", "Saw", "Square", "Noise"],
                wave,
                accent,
                move |s: &mut App, w| {
                    patch_edit(s, tid, true, |p| p.osc[i].wave = Waveform::ALL[w])
                },
            ))
            .child(wave_preview(o.wave, o.pulse_width, accent))
            .child(
                row()
                    .gap(14.0)
                    .child(knob(
                        format!("lvl{i}"),
                        "Level",
                        o.level,
                        format!("{:.0}%", o.level * 100.0),
                        accent,
                        move |s: &mut App, v, st| patch_edit(s, tid, st, |p| p.osc[i].level = v),
                    ))
                    .child(knob(
                        format!("oct{i}"),
                        "Octave",
                        (f32::from(o.octave) + 2.0) / 4.0,
                        format!("{:+}", o.octave),
                        accent,
                        move |s: &mut App, v, st| {
                            patch_edit(s, tid, st, |p| {
                                p.osc[i].octave = ((v * 4.0).round() - 2.0) as i8
                            })
                        },
                    ))
                    .child(knob(
                        format!("det{i}"),
                        "Detune",
                        (o.detune_cents + 50.0) / 100.0,
                        format!("{:+.0}c", o.detune_cents),
                        accent,
                        move |s: &mut App, v, st| {
                            patch_edit(s, tid, st, |p| {
                                p.osc[i].detune_cents = (v * 100.0 - 50.0).round()
                            })
                        },
                    ))
                    .child(knob(
                        format!("pw{i}"),
                        "Pulse width",
                        o.pulse_width,
                        format!("{:.0}%", o.pulse_width * 100.0),
                        accent,
                        move |s: &mut App, v, st| {
                            patch_edit(s, tid, st, |p| p.osc[i].pulse_width = v.clamp(0.05, 0.95))
                        },
                    )),
            )
    };
    let fmode = FilterMode::ALL
        .iter()
        .position(|m| *m == p.filter.mode)
        .unwrap_or(0);
    let filter = crate::widgets::card()
        .child(
            row()
                .gap(8.0)
                .items_center()
                .child(caps("Filter"))
                .child(rune::spacer())
                .child(segmented(
                    "fmode",
                    &["LP", "HP", "BP"],
                    fmode,
                    accent,
                    move |s: &mut App, m| {
                        patch_edit(s, tid, true, |p| p.filter.mode = FilterMode::ALL[m])
                    },
                )),
        )
        .child(
            row()
                .gap(14.0)
                .child(knob(
                    "cut",
                    "Cutoff",
                    hz_to_unit(p.filter.cutoff_hz),
                    format!("{:.0} Hz", p.filter.cutoff_hz),
                    accent,
                    move |s: &mut App, v, st| {
                        patch_edit(s, tid, st, |p| p.filter.cutoff_hz = unit_to_hz(v))
                    },
                ))
                .child(knob(
                    "res",
                    "Resonance",
                    p.filter.resonance,
                    format!("{:.0}%", p.filter.resonance * 100.0),
                    accent,
                    move |s: &mut App, v, st| patch_edit(s, tid, st, |p| p.filter.resonance = v),
                ))
                .child(knob(
                    "fenv",
                    "Env amount",
                    p.filter.env_octaves / 4.0,
                    format!("{:.1} oct", p.filter.env_octaves),
                    accent,
                    move |s: &mut App, v, st| {
                        patch_edit(s, tid, st, |p| p.filter.env_octaves = v * 4.0)
                    },
                ))
                .child(knob(
                    "noise",
                    "Noise",
                    p.noise,
                    format!("{:.0}%", p.noise * 100.0),
                    accent,
                    move |s: &mut App, v, st| patch_edit(s, tid, st, |p| p.noise = v),
                )),
        );
    let env = |name: &'static str, which: u8, a: Adsr| {
        crate::widgets::card()
            .child(caps(name))
            .child(adsr_preview(a, accent))
            .child(
                row()
                    .gap(14.0)
                    .child(knob(
                        format!("a{which}"),
                        "Attack",
                        sec_to_unit(a.attack),
                        fmt_sec(a.attack),
                        accent,
                        move |s: &mut App, v, st| {
                            patch_edit(s, tid, st, |p| env_mut(p, which).attack = unit_to_sec(v))
                        },
                    ))
                    .child(knob(
                        format!("d{which}"),
                        "Decay",
                        sec_to_unit(a.decay),
                        fmt_sec(a.decay),
                        accent,
                        move |s: &mut App, v, st| {
                            patch_edit(s, tid, st, |p| env_mut(p, which).decay = unit_to_sec(v))
                        },
                    ))
                    .child(knob(
                        format!("s{which}"),
                        "Sustain",
                        a.sustain,
                        format!("{:.0}%", a.sustain * 100.0),
                        accent,
                        move |s: &mut App, v, st| {
                            patch_edit(s, tid, st, |p| env_mut(p, which).sustain = v)
                        },
                    ))
                    .child(knob(
                        format!("r{which}"),
                        "Release",
                        sec_to_unit(a.release),
                        fmt_sec(a.release),
                        accent,
                        move |s: &mut App, v, st| {
                            patch_edit(s, tid, st, |p| env_mut(p, which).release = unit_to_sec(v))
                        },
                    )),
            )
    };
    let lt = LfoTarget::ALL
        .iter()
        .position(|t| *t == p.lfo.target)
        .unwrap_or(0);
    let modulation = crate::widgets::card()
        .child(
            row()
                .gap(8.0)
                .items_center()
                .child(caps("LFO and pitch"))
                .child(rune::spacer())
                .child(segmented(
                    "lfot",
                    &["Pitch", "Cutoff", "Volume", "PW"],
                    lt,
                    accent,
                    move |s: &mut App, i| {
                        patch_edit(s, tid, true, |p| p.lfo.target = LfoTarget::ALL[i])
                    },
                )),
        )
        .child(
            row()
                .gap(14.0)
                .child(knob(
                    "lrate",
                    "LFO",
                    p.lfo.rate_hz / 12.0,
                    format!("{:.2} Hz", p.lfo.rate_hz),
                    accent,
                    move |s: &mut App, v, st| {
                        patch_edit(s, tid, st, |p| p.lfo.rate_hz = (v * 12.0).max(0.05))
                    },
                ))
                .child(knob(
                    "ldepth",
                    "Depth",
                    p.lfo.depth,
                    format!("{:.0}%", p.lfo.depth * 100.0),
                    accent,
                    move |s: &mut App, v, st| patch_edit(s, tid, st, |p| p.lfo.depth = v),
                ))
                .child(knob(
                    "pitchenv",
                    "Pitch envelope",
                    (p.pitch_env.semitones + 48.0) / 96.0,
                    format!("{:+.0} st", p.pitch_env.semitones),
                    accent,
                    move |s: &mut App, v, st| {
                        patch_edit(s, tid, st, |p| {
                            p.pitch_env.semitones = (v * 96.0 - 48.0).round()
                        })
                    },
                ))
                .child(knob(
                    "pdecay",
                    "Pitch decay",
                    sec_to_unit(p.pitch_env.decay),
                    fmt_sec(p.pitch_env.decay),
                    accent,
                    move |s: &mut App, v, st| {
                        patch_edit(s, tid, st, |p| p.pitch_env.decay = unit_to_sec(v))
                    },
                ))
                .child(knob(
                    "glide",
                    "Glide",
                    p.glide / 0.5,
                    fmt_sec(p.glide),
                    accent,
                    move |s: &mut App, v, st| patch_edit(s, tid, st, |p| p.glide = v * 0.5),
                ))
                .child(knob(
                    "gain",
                    "Gain",
                    (p.gain_db + 24.0) / 36.0,
                    format!("{:+.1} dB", p.gain_db),
                    accent,
                    move |s: &mut App, v, st| {
                        patch_edit(s, tid, st, |p| p.gain_db = (v * 36.0 - 24.0).round())
                    },
                )),
        );
    let presets = {
        let mut r = row()
            .gap(6.0)
            .flex_wrap()
            .child(caps("Start from"))
            .items_center();
        for r2 in TrackRole::ALL {
            r = r.child(
                crate::widgets::pill(
                    format!("pre-{}", r2.name()),
                    r2.name(),
                    theme::role_color(r2),
                    r2 == role,
                )
                .on_press(move |s: &mut App| {
                    patch_edit(s, tid, true, |p| *p = SynthPatch::preset(r2))
                }),
            );
        }
        r.child(
            button("mutate", "✦ Mutate", accent, true)
                .help("Nudge every knob a little at random: new ideas, same character")
                .on_press(move |s: &mut App| {
                    s.ext.seed = s
                        .ext
                        .seed
                        .wrapping_mul(2862933555777941757)
                        .wrapping_add(3037000493);
                    let seed = s.ext.seed;
                    patch_edit(s, tid, true, |p| mutate(p, seed));
                    s.send(Command::PreviewNote {
                        track: ti,
                        pitch: role.default_note(),
                        velocity: 0.9,
                        seconds: 0.4,
                    });
                }),
        )
    };
    col()
        .gap(14.0)
        .child(
            row()
                .gap(8.0)
                .items_center()
                .child(text(p.name.clone()).font(font::HEAD).color(theme::text())),
        )
        .child(presets)
        .child(
            row()
                .gap(14.0)
                .flex_wrap()
                .child(osc(0))
                .child(osc(1))
                .child(filter),
        )
        .child(
            row()
                .gap(14.0)
                .flex_wrap()
                .child(env("Amp envelope (ADSR)", 0, p.amp_env))
                .child(env("Filter envelope", 1, p.filter_env))
                .child(modulation),
        )
}

fn env_mut(p: &mut SynthPatch, which: u8) -> &mut Adsr {
    if which == 0 {
        &mut p.amp_env
    } else {
        &mut p.filter_env
    }
}

fn fmt_sec(s: f32) -> String {
    if s < 1.0 {
        format!("{:.0} ms", s * 1000.0)
    } else {
        format!("{s:.2} s")
    }
}

fn mutate(p: &mut SynthPatch, seed: u64) {
    let mut rng = aa_theory::Rng::new(seed);
    let mut nudge = |v: f32, amount: f32, lo: f32, hi: f32| {
        (v + (rng.next_f32() * 2.0 - 1.0) * amount).clamp(lo, hi)
    };
    p.filter.cutoff_hz = nudge(p.filter.cutoff_hz.ln(), 0.5, 20f32.ln(), 18_000f32.ln()).exp();
    p.filter.resonance = nudge(p.filter.resonance, 0.15, 0.0, 0.9);
    p.osc[1].detune_cents = nudge(p.osc[1].detune_cents, 8.0, -50.0, 50.0);
    p.osc[0].pulse_width = nudge(p.osc[0].pulse_width, 0.15, 0.1, 0.9);
    p.amp_env.decay = nudge(p.amp_env.decay, p.amp_env.decay * 0.4, 0.005, 4.0);
    p.lfo.depth = nudge(p.lfo.depth, 0.1, 0.0, 0.8);
    p.filter.env_octaves = nudge(p.filter.env_octaves, 0.6, 0.0, 4.0);
    p.name = format!("{} (mutated)", p.name.trim_end_matches(" (mutated)"));
}

fn wave_preview(wave: Waveform, pw: f32, accent: Color) -> El {
    let (w, h) = (210.0, 36.0);
    let pts: Vec<[f32; 2]> = (0..=120)
        .map(|i| {
            let t = (i as f32 / 60.0).fract();
            let v = match wave {
                Waveform::Sine => (t * std::f32::consts::TAU).sin(),
                Waveform::Triangle => 1.0 - 4.0 * (t - 0.5).abs(),
                Waveform::Saw => 2.0 * t - 1.0,
                Waveform::Square => {
                    if t < pw {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Waveform::Noise => ((i * 7919 % 113) as f32 / 56.0) - 1.0,
            };
            [i as f32 / 120.0 * w, h * 0.5 - v * h * 0.42]
        })
        .collect();
    canvas(Shape::new().stroke(pts, 2.0, accent)).size(w, h)
}

fn adsr_preview(a: Adsr, accent: Color) -> El {
    let (w, h) = (230.0, 46.0);
    let total = a.attack + a.decay + 0.3 + a.release;
    let x = |t: f32| t / total * w;
    let pts = vec![
        [0.0, h],
        [x(a.attack), 2.0],
        [x(a.attack + a.decay), h - a.sustain * (h - 2.0)],
        [x(a.attack + a.decay + 0.3), h - a.sustain * (h - 2.0)],
        [w, h],
    ];
    canvas(
        Shape::new()
            .fill(pts.clone(), accent.alpha(0.18))
            .stroke(pts, 2.0, accent),
    )
    .size(w, h)
}

/// One octave of keys that play the selected track.
fn keyboard(ti: usize, accent: Color) -> El {
    let mut r = row().key("kbd").gap(3.0).items_start();
    let base = 48u8;
    for k in 0..25u8 {
        let pitch = base + k;
        let black = matches!(k % 12, 1 | 3 | 6 | 8 | 10);
        r = r.child(
            el().key(format!("key-{pitch}"))
                .role("button")
                .label(aa_core::music::note_name(pitch))
                .w(if black { 22.0 } else { 30.0 })
                .h(if black { 54.0 } else { 78.0 })
                .radius(6.0)
                .fill(if black { theme::bg() } else { theme::card_hi() })
                .border(1.0, theme::line_strong())
                .hover(accent.alpha(0.3))
                .press(accent.alpha(0.6))
                .press_scale(0.96)
                .on_press(move |s: &mut App| {
                    s.send(Command::PreviewNote {
                        track: ti,
                        pitch,
                        velocity: 0.9,
                        seconds: 0.5,
                    });
                }),
        );
    }
    col().gap(6.0).child(caps("Play it")).child(r)
}
