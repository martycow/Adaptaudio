//! Sound Lab: the built-in synth, sampler zones and hardware routing for the selected track.

use super::common::empty_state;
use crate::state::App;
use crate::theme::{self, font};
use crate::widgets::{
    body, button, caps, dropdown, field_row, knob, pill, screen_header, segmented, text_field,
    vscroll,
};
use aa_core::{Adsr, FilterMode, Instrument, LfoTarget, SynthPatch, TrackId, TrackRole, Waveform};
use aa_engine::Command;
use rune::{Color, El, Shape, canvas, col, el, row, text};

pub fn view(s: &App, accent: Color) -> El {
    let head = screen_header(
        "Sound Lab",
        "Choose a track, shape its sound, and keep the sounds you like in the library to use anywhere.",
        accent,
    );
    let Some(track) = s.selected_track.and_then(|t| s.project.track(t)) else {
        return col().padding(22.0).gap(16.0).child(head).child(empty_state(
            "This project has no tracks yet",
            "Add a track on the Session screen, then come back to shape its sound.",
        ));
    };
    let tid = track.id;
    let ti = s.project.track_index(tid).unwrap_or(0);
    let kinds = ["Built-in synth", "Sampler", "Hardware (MIDI)"];
    let current = match track.instrument {
        Instrument::Synth(_) => 0,
        Instrument::Sampler(_) => 1,
        Instrument::External(_) => 2,
    };
    let role = track.role;
    let names: Vec<String> = s.project.tracks.iter().map(|t| t.name.clone()).collect();
    let midi_out = s.settings.midi_out.clone().unwrap_or_default();
    let top = row()
        .gap(12.0)
        .items_center()
        .child(text("Track").font(font::LABEL).color(theme::muted()))
        .child(
            el().size(10.0, 10.0)
                .radius(5.0)
                .fill(theme::role_color(track.role)),
        )
        .child(dropdown(
            s,
            "sound-track",
            &names,
            Some(ti),
            200.0,
            |s: &mut App, i, _| {
                if let Some(id) = s.project.tracks.get(i).map(|t| t.id) {
                    s.select_clip(id, None);
                    s.ext.confirm_delete = None;
                }
            },
        ))
        .child(el().w(8.0))
        .child(
            text("Plays through")
                .font(font::LABEL)
                .color(theme::muted()),
        )
        .child(segmented(
            "ikind",
            &kinds,
            current,
            accent,
            move |s: &mut App, i| {
                if i == current {
                    return;
                }
                let port = midi_out.clone();
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
                            _ => Instrument::External(aa_midi::external_instrument_for(&port)),
                        };
                    }
                });
            },
        ));
    let body = match &track.instrument {
        Instrument::Synth(patch) => synth(tid, ti, role, patch, accent),
        Instrument::Sampler(inst) => sampler(inst),
        Instrument::External(ext) => external(s, tid, ti, role, ext, accent),
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
            row()
                .grow(1.0)
                .min_h(0.0)
                .gap(18.0)
                .child(library(s, track, accent))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .min_h(0.0)
                        .gap(14.0)
                        .child(vscroll(body))
                        .child(keyboard(s, ti, accent)),
                ),
        )
}

/// The left column: sounds to load into this track, and a way to keep the current one.
fn library(s: &App, track: &aa_core::Track, accent: Color) -> El {
    let tid = track.id;
    let patch = match &track.instrument {
        Instrument::Synth(p) => Some(p),
        _ => None,
    };
    let mut c = col().gap(8.0);
    c = c.child(caps("Keep this sound"));
    match patch {
        Some(patch) => {
            c = c
                .child(text_field(
                    "sound-name",
                    &s.ext.sound_name,
                    &patch.name,
                    228.0,
                    |s: &mut App| &mut s.ext.sound_name,
                ))
                .child(
                    button("sound-save", "Save to library", accent, true)
                        .help("Saves this track's sound under the name above (or its current name)")
                        .on_press(move |s: &mut App| save_sound(s, tid)),
                );
        }
        None => {
            c = c.child(body(
                "Only built-in synth sounds go into the library. A sampler's recordings stay with its project.",
            ));
        }
    }
    c = c.child(caps("Your sounds").mt(10.0));
    if s.ext.sounds.is_empty() {
        c = c.child(body("Nothing saved yet."));
    }
    for (i, sound) in s.ext.sounds.iter().enumerate() {
        let (load_path, delete_path) = (sound.path.clone(), sound.path.clone());
        let confirming = s.ext.confirm_delete.as_ref() == Some(&sound.path);
        let current = patch.is_some_and(|p| p.name == sound.name);
        c = c.child(
            row()
                .gap(4.0)
                .items_center()
                .child(
                    row()
                        .key(format!("sound-load-{i}"))
                        .role("button")
                        .label(format!("Load {}", sound.name))
                        .help("Load this sound into the track")
                        .grow(1.0)
                        .min_w(0.0)
                        .h(30.0)
                        .px(10.0)
                        .items_center()
                        .radius(8.0)
                        .fill(if current {
                            accent.alpha(0.14)
                        } else {
                            theme::panel()
                        })
                        .hover(theme::hover())
                        .on_press(move |s: &mut App| load_sound(s, tid, &load_path))
                        .child(
                            text(sound.name.clone())
                                .font(font::LABEL)
                                .color(theme::text())
                                .truncate(),
                        ),
                )
                .child(
                    button(
                        format!("sound-del-{i}"),
                        if confirming { "Delete?" } else { "✕" },
                        theme::danger(),
                        confirming,
                    )
                    .help("Remove this sound from the library (asks once more)")
                    .on_press(move |s: &mut App| delete_sound(s, &delete_path)),
                ),
        );
    }
    c = c.child(caps("Starting points").mt(10.0));
    let mut presets = row().gap(4.0).flex_wrap();
    for r in TrackRole::ALL {
        presets = presets.child(
            pill(
                format!("pre-{}", r.name()),
                r.name(),
                theme::role_color(r),
                false,
            )
            .help(format!("A basic {} sound to start from", r.name()))
            .on_press(move |s: &mut App| {
                s.edit(|p| {
                    if let Some(t) = p.track_mut(tid) {
                        t.instrument = Instrument::Synth(SynthPatch::preset(r));
                    }
                })
            }),
        );
    }
    c = c.child(presets);
    col()
        .key("sound-library")
        .w(260.0)
        .h_full()
        .min_h(0.0)
        .radius(12.0)
        .fill(theme::card())
        .child(vscroll(c.padding(16.0)))
}

fn save_sound(s: &mut App, tid: TrackId) {
    let Some(Instrument::Synth(patch)) = s.project.track(tid).map(|t| &t.instrument) else {
        return;
    };
    let Some(dir) = crate::library::dir() else {
        s.toast_error("No home folder to keep sounds in");
        return;
    };
    let mut patch = patch.clone();
    let typed = s.ext.sound_name.text.trim().to_string();
    if !typed.is_empty() {
        patch.name = typed;
    }
    match crate::library::save(&dir, &patch) {
        Ok(saved) => {
            // The track takes the saved name, so it is clear which library sound it uses.
            let name = saved.name.clone();
            patch_edit(s, tid, true, |p| p.name = name);
            s.ext.sounds = crate::library::list(&dir);
            s.ext.sound_name = rune::TextEdit::new("");
            s.toast(&format!("Saved “{}” to the sound library", saved.name));
        }
        Err(e) => s.toast_error(&format!("Can't save the sound: {e}")),
    }
}

fn load_sound(s: &mut App, tid: TrackId, path: &std::path::Path) {
    match crate::library::load(path) {
        Ok(patch) => {
            let name = patch.name.clone();
            s.edit(|p| {
                if let Some(t) = p.track_mut(tid) {
                    t.instrument = Instrument::Synth(patch);
                }
            });
            s.ext.confirm_delete = None;
            if let Some(ti) = s.project.track_index(tid) {
                let pitch = s.project.tracks[ti].role.default_note();
                s.send(Command::PreviewNote {
                    track: ti,
                    pitch,
                    velocity: 0.9,
                    seconds: 0.4,
                });
            }
            s.toast(&format!("Loaded “{name}” (⌘Z brings the old sound back)"));
        }
        Err(e) => s.toast_error(&format!("Can't load the sound: {e}")),
    }
}

fn delete_sound(s: &mut App, path: &std::path::Path) {
    if s.ext.confirm_delete.as_deref() != Some(path) {
        s.ext.confirm_delete = Some(path.to_path_buf());
        return;
    }
    s.ext.confirm_delete = None;
    match crate::library::delete(path) {
        Ok(()) => {
            if let Some(dir) = crate::library::dir() {
                s.ext.sounds = crate::library::list(&dir);
            }
            s.toast("Removed from the sound library");
        }
        Err(e) => s.toast_error(&format!("Can't delete: {e}")),
    }
}

fn sampler(inst: &aa_core::SamplerInstrument) -> El {
    let mut c = crate::widgets::card()
        .child(caps(format!("{} · {} recordings", inst.name, inst.zones.len())))
        .child(body(
            "A sampler plays recordings. Make them on the Capture screen: record a hardware synth, a groovebox or a microphone, and they land here.",
        ));
    for z in &inst.zones {
        c = c.child(
            text(format!(
                "{}   {}–{}   root {}",
                z.path,
                aa_core::music::note_name(z.low_note),
                aa_core::music::note_name(z.high_note),
                aa_core::music::note_name(z.root_note)
            ))
            .font(font::SMALL.mono())
            .color(theme::muted()),
        );
    }
    c
}

/// A track that plays a hardware synth: where the notes go and a way to check they arrive.
fn external(
    s: &App,
    tid: TrackId,
    ti: usize,
    role: TrackRole,
    ext: &aa_core::ExternalInstrument,
    accent: Color,
) -> El {
    let port = s.settings.midi_out.clone();
    let connected = s.ext.capture.scheduler.is_some();
    let status = match (&port, connected) {
        (Some(p), true) => (format!("Connected to {p}"), theme::ok()),
        (Some(p), false) => (format!("{p} is not connected"), theme::warn()),
        (None, _) => ("No MIDI output chosen".to_string(), theme::warn()),
    };
    let outs = s.ext.devices.midi_out.clone();
    let channels: Vec<String> = (1..=16).map(|c| format!("Channel {c}")).collect();
    crate::widgets::card()
        .child(caps("Hardware synth over MIDI"))
        .child(
            row()
                .gap(8.0)
                .items_center()
                .child(el().size(8.0, 8.0).radius(4.0).fill(status.1))
                .child(text(status.0).font(font::HEAD).color(theme::text())),
        )
        .child(field_row(
            "MIDI output",
            dropdown(
                s,
                "ext-port",
                &outs,
                port.as_ref().and_then(|p| outs.iter().position(|o| o == p)),
                280.0,
                |s: &mut App, i, _| {
                    if let Some(name) = s.ext.devices.midi_out.get(i).cloned() {
                        s.settings.midi_out = Some(name);
                        let _ = s.settings.save();
                        s.restart_audio();
                    }
                },
            ),
        ))
        .child(field_row(
            "MIDI channel",
            dropdown(
                s,
                "ext-channel",
                &channels,
                Some(usize::from(ext.channel.clamp(1, 16)) - 1),
                140.0,
                move |s: &mut App, i, _| {
                    s.edit(|p| {
                        if let Some(Instrument::External(e)) =
                            p.track_mut(tid).map(|t| &mut t.instrument)
                        {
                            e.channel = i as u8 + 1;
                        }
                    })
                },
            ),
        ))
        .child(
            row()
                .gap(10.0)
                .items_center()
                .child(
                    button("ext-test", "♪ Send a test note", accent, true)
                        .disabled(!connected)
                        .on_press(move |s: &mut App| {
                            s.send(Command::PreviewNote {
                                track: ti,
                                pitch: role.default_note().max(48),
                                velocity: 0.8,
                                seconds: 0.5,
                            })
                        }),
                )
                .child(
                    text(format!("Latency {:.1} ms", ext.latency_ms))
                        .font(font::LABEL)
                        .color(theme::muted())
                        .help("How early notes are sent so the synth lands on the beat. Measure it on the Capture screen."),
                ),
        )
        .child(body(
            "Prototype only: a hardware synth can't ship inside the game. When the part sounds right, record it into a sampler on the Capture screen, and it will export to FMOD.",
        ))
        .child(body(
            "No sound? Check that the synth listens on the channel above, and that its audio reaches your interface.",
        ))
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
    let mutate = button("mutate", "✦ Mutate", accent, false)
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
        });
    col()
        .gap(14.0)
        .child(
            row()
                .gap(8.0)
                .items_center()
                .child(text(p.name.clone()).font(font::TITLE).color(theme::text()))
                .child(rune::spacer())
                .child(mutate),
        )
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

/// Two octaves of keys that play the selected track, and whether a hardware keyboard is heard.
fn keyboard(s: &App, ti: usize, accent: Color) -> El {
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
                .h(if black { 44.0 } else { 64.0 })
                .radius(5.0)
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
    let hardware = match s.ext.devices.keyboard_port() {
        Some(port) => {
            let alive = s.ext.devices.last_key.is_some_and(|t| s.now - t < 1.5);
            row()
                .gap(6.0)
                .items_center()
                .help("Keys you press on the hardware keyboard play this track. Choose the keyboard in Settings → MIDI.")
                .child(
                    el().size(8.0, 8.0)
                        .radius(4.0)
                        .fill(if alive { theme::ok() } else { theme::faint() }),
                )
                .child(
                    text(format!("Keyboard: {port}"))
                        .font(font::SMALL)
                        .color(theme::muted()),
                )
        }
        None => row().child(
            text("No hardware keyboard connected")
                .font(font::SMALL)
                .color(theme::faint()),
        ),
    };
    col()
        .gap(6.0)
        .child(
            row()
                .gap(12.0)
                .items_center()
                .child(caps("Play it"))
                .child(hardware),
        )
        .child(r)
}
