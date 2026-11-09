//! Capture: play hardware synths live (MIDI out) and bake them into sampler instruments.
//!
//! Listing audio inputs can block while macOS asks for microphone access, so every audio-input
//! call runs on a worker thread and reports back through a channel.

use crate::state::App;
use crate::theme::{self, font};
use crate::widgets::{body, button, caps, card, live_bar, screen_header, segmented, slider_row};
use aa_capture::{CapturePlan, CaptureProgress, CaptureSession};
use aa_core::{ExternalInstrument, Instrument};
use aa_engine::{Engine, ExternalNote};
use aa_midi::{MidiMessage, MidiOut, MidiScheduler};
use rune::{Color, El, Live, col, row, text};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

pub enum Job {
    Inputs(Result<Vec<String>, String>),
    Latency(Result<f32, String>),
    Captured(Result<(aa_core::SamplerInstrument, usize), String>),
}

pub struct CaptureUi {
    pub outputs: Option<Result<Vec<String>, String>>,
    pub inputs: Option<Result<Vec<String>, String>>,
    pub busy: Option<String>,
    pub rx: Option<Receiver<Job>>,
    pub progress: Option<Arc<CaptureProgress>>,
    pub progress_live: Live<[f32; 2]>,
    pub low: u8,
    pub high: u8,
    pub step: u8,
    pub layers: usize,
    pub latency_ms: Option<f32>,
    /// Keeps the MIDI scheduler thread alive while the audio engine sends to it.
    pub scheduler: Option<MidiScheduler>,
}

impl Default for CaptureUi {
    fn default() -> Self {
        CaptureUi {
            outputs: None,
            inputs: None,
            busy: None,
            rx: None,
            progress: None,
            progress_live: Live::new([0.0, 0.0]),
            low: 36,
            high: 84,
            step: 3,
            layers: 2,
            latency_ms: None,
            scheduler: None,
        }
    }
}

impl CaptureUi {
    fn plan(&self, channel: u8) -> CapturePlan {
        let vels: &[u8] = match self.layers {
            1 => &[100],
            2 => &[60, 115],
            _ => &[45, 90, 127],
        };
        let mut p = CapturePlan::every_nth_semitone(
            self.low,
            self.high.max(self.low),
            self.step.max(1),
            vels,
        );
        p.channel = channel;
        p
    }
}

/// Connects the engine's External MIDI tracks to the chosen MIDI output.
pub fn wire_external(engine: &mut Engine, app: &mut App) {
    let Some(port) = app.settings.midi_out.clone() else {
        return;
    };
    let scheduler = match MidiOut::connect(&port)
        .map_err(|e| e.to_string())
        .and_then(|out| MidiScheduler::start(out).map_err(|e| e.to_string()))
    {
        Ok(s) => s,
        Err(e) => {
            app.audio_error = Some(format!("MIDI out “{port}”: {e}"));
            return;
        }
    };
    let mut producer = scheduler.producer();
    let epoch = engine.epoch();
    engine.set_external_sink(Box::new(move |n: ExternalNote| {
        let msg = if n.on {
            MidiMessage::NoteOn {
                channel: n.channel,
                note: n.pitch,
                velocity: n.velocity,
            }
        } else {
            MidiMessage::NoteOff {
                channel: n.channel,
                note: n.pitch,
                velocity: 0,
            }
        };
        let _ = producer.schedule(epoch + n.at, msg);
    }));
    app.ext.capture.scheduler = Some(scheduler);
}

pub fn poll(s: &mut App, _now: f64, tx: &mut rune::Tx) -> bool {
    if let Some(p) = &s.ext.capture.progress {
        s.ext.capture.progress_live.set([0.0, p.fraction()]);
    }
    let Some(rx) = &s.ext.capture.rx else {
        return false;
    };
    let job = match rx.try_recv() {
        Ok(j) => j,
        Err(TryRecvError::Empty) => {
            tx.wake_after(Duration::from_millis(150));
            return false;
        }
        Err(TryRecvError::Disconnected) => {
            s.ext.capture.rx = None;
            s.ext.capture.busy = None;
            return true;
        }
    };
    s.ext.capture.rx = None;
    s.ext.capture.busy = None;
    s.ext.capture.progress = None;
    match job {
        Job::Inputs(r) => s.ext.capture.inputs = Some(r),
        Job::Latency(Ok(ms)) => {
            s.ext.capture.latency_ms = Some(ms);
            if let Some(tid) = s.selected_track {
                s.edit(|p| {
                    if let Some(Instrument::External(e)) =
                        p.track_mut(tid).map(|t| &mut t.instrument)
                    {
                        e.latency_ms = ms
                    }
                });
            }
            s.toast(&format!("Round-trip latency: {ms:.1} ms"));
        }
        Job::Latency(Err(e)) => s.toast_error(&e),
        Job::Captured(Ok((inst, zones))) => {
            if let Some(tid) = s.selected_track {
                s.edit(|p| {
                    if let Some(t) = p.track_mut(tid) {
                        t.instrument = Instrument::Sampler(inst)
                    }
                });
            }
            s.toast(&format!(
                "Captured {zones} samples into a sampler instrument"
            ));
        }
        Job::Captured(Err(e)) => s.toast_error(&e),
    }
    true
}

fn spawn(s: &mut App, what: &str, f: impl FnOnce() -> Job + Send + 'static) {
    if s.ext.capture.busy.is_some() {
        return;
    }
    let (tx, rx) = channel();
    s.ext.capture.rx = Some(rx);
    s.ext.capture.busy = Some(what.to_string());
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
}

fn channel_of(s: &App) -> u8 {
    s.selected_track
        .and_then(|t| s.project.track(t))
        .and_then(|t| match &t.instrument {
            Instrument::External(e) => Some(e.channel),
            _ => None,
        })
        .unwrap_or(1)
}

pub fn view(s: &App, accent: Color) -> El {
    let cu = &s.ext.capture;
    let track = s.selected_track.and_then(|t| s.project.track(t));
    let midi = {
        let mut c = card().child(caps("1 · MIDI out (to the synth)"));
        match &cu.outputs {
            None => {
                c = c.child(
                    button("scan-midi", "Find MIDI devices", accent, true).on_press(
                        |s: &mut App| {
                            s.ext.capture.outputs =
                                Some(aa_midi::list_outputs().map_err(|e| e.to_string()))
                        },
                    ),
                )
            }
            Some(Err(e)) => c = c.child(text(e.clone()).font(font::BODY).color(theme::danger())),
            Some(Ok(list)) if list.is_empty() => c = c
                .child(body(
                    "No MIDI outputs. Connect the Hydrasynth or MiniFreak over USB and scan again.",
                ))
                .child(
                    button("rescan-midi", "Scan again", accent, false)
                        .on_press(|s: &mut App| s.ext.capture.outputs = None),
                ),
            Some(Ok(list)) => {
                for name in list {
                    let on = s.settings.midi_out.as_deref() == Some(name.as_str());
                    let hint = aa_midi::guess_device(name)
                        .map_or(String::new(), |d| format!(" · {}", d.friendly_name));
                    let n = name.clone();
                    c = c.child(
                        crate::widgets::pill(
                            format!("mo-{name}"),
                            format!("{name}{hint}"),
                            accent,
                            on,
                        )
                        .on_press(move |s: &mut App| {
                            s.settings.midi_out = Some(n.clone());
                            let _ = s.settings.save();
                            s.restart_audio();
                            s.toast("MIDI out connected; External tracks play through it");
                        }),
                    );
                }
            }
        }
        c.child(body("Notes on External MIDI tracks go here, timed ahead by the measured latency so they land on the beat."))
    };
    let live = {
        let can = s.settings.midi_out.is_some() && track.is_some();
        let mut c = card().child(caps("2 · Play it live"));
        c = c.child(body("Turn the selected track into an External MIDI track: the sequencer plays your hardware synth."));
        let port = s.settings.midi_out.clone().unwrap_or_default();
        c.child(
            button("make-ext", "Use hardware on this track", accent, can)
                .disabled(!can)
                .on_press(move |s: &mut App| {
                    let Some(tid) = s.selected_track else { return };
                    let mut ext = aa_midi::external_instrument_for(&port);
                    if let Some(ms) = s.ext.capture.latency_ms {
                        ext.latency_ms = ms;
                    }
                    let inst: ExternalInstrument = ext;
                    s.edit(|p| {
                        if let Some(t) = p.track_mut(tid) {
                            t.instrument = Instrument::External(inst)
                        }
                    });
                }),
        )
    };
    let input = {
        let mut c = card().child(caps("3 · Audio in (from the H4essential)"));
        match &cu.inputs {
            None => {
                c = c.child(body("macOS may ask for microphone access the first time. That is the audio input, not a microphone recording you.")).child(button("scan-in", "Find audio inputs", accent, true).on_press(|s: &mut App| spawn(s, "Looking for audio inputs…", || Job::Inputs(aa_capture::list_input_devices().map_err(|e| e.to_string())))));
            }
            Some(Err(e)) => {
                c = c
                    .child(
                        text(e.clone())
                            .font(font::BODY)
                            .color(theme::danger())
                            .wrap(),
                    )
                    .child(
                        button("rescan-in", "Try again", accent, false)
                            .on_press(|s: &mut App| s.ext.capture.inputs = None),
                    )
            }
            Some(Ok(list)) => {
                for name in list {
                    let on = s.settings.audio_in.as_deref() == Some(name.as_str());
                    let n = name.clone();
                    c = c.child(
                        crate::widgets::pill(format!("ai-{name}"), name.clone(), accent, on)
                            .on_press(move |s: &mut App| {
                                s.settings.audio_in = Some(n.clone());
                                let _ = s.settings.save();
                            }),
                    );
                }
            }
        }
        let ready = s.settings.midi_out.is_some();
        c.child(
            button("latency", "Measure latency", accent, false)
                .disabled(!ready)
                .help("Plays one note and listens for it: the round trip from MIDI out to audio in")
                .on_press(|s: &mut App| {
                    let (port, input, ch) = (
                        s.settings.midi_out.clone().unwrap_or_default(),
                        s.settings.audio_in.clone(),
                        channel_of(s),
                    );
                    spawn(s, "Measuring latency…", move || {
                        let r = MidiOut::connect(&port).map_err(|e| e.to_string()).and_then(
                            |mut out| {
                                CaptureSession::new(input.as_deref())
                                    .measure_midi_latency(&mut out, ch, 60, 100)
                                    .map_err(|e| e.to_string())
                            },
                        );
                        Job::Latency(r)
                    });
                }),
        )
        .child(
            text(
                cu.latency_ms
                    .map_or("Latency not measured yet".into(), |ms| {
                        format!("Latency: {ms:.1} ms")
                    }),
            )
            .font(font::LABEL)
            .color(theme::muted()),
        )
    };
    let plan = cu.plan(channel_of(s));
    let capture = {
        let mut c = card().child(caps("4 · Capture into a sampler"));
        c = c
            .child(slider_row(
                "lo",
                "Lowest",
                f32::from(cu.low - 24) / 60.0,
                aa_core::music::note_name(cu.low),
                160.0,
                accent,
                |s: &mut App, v, _| s.ext.capture.low = 24 + (v * 60.0) as u8,
            ))
            .child(slider_row(
                "hi",
                "Highest",
                f32::from(cu.high - 24) / 72.0,
                aa_core::music::note_name(cu.high),
                160.0,
                accent,
                |s: &mut App, v, _| s.ext.capture.high = 24 + (v * 72.0) as u8,
            ))
            .child(
                row()
                    .gap(8.0)
                    .items_center()
                    .child(text("Every").font(font::LABEL).color(theme::muted()))
                    .child(segmented(
                        "cstep",
                        &["1", "2", "3", "4", "6"],
                        [1u8, 2, 3, 4, 6]
                            .iter()
                            .position(|x| *x == cu.step)
                            .unwrap_or(2),
                        accent,
                        |s: &mut App, i| s.ext.capture.step = [1u8, 2, 3, 4, 6][i],
                    ))
                    .child(text("semitones").font(font::LABEL).color(theme::muted())),
            )
            .child(
                row()
                    .gap(8.0)
                    .items_center()
                    .child(
                        text("Velocity layers")
                            .font(font::LABEL)
                            .color(theme::muted()),
                    )
                    .child(segmented(
                        "clay",
                        &["1", "2", "3"],
                        cu.layers.clamp(1, 3) - 1,
                        accent,
                        |s: &mut App, i| s.ext.capture.layers = i + 1,
                    )),
            )
            .child(
                text(format!(
                    "{} takes · about {:.0} s",
                    plan.take_count(),
                    plan.total_duration().as_secs_f32()
                ))
                .font(font::LABEL)
                .color(theme::text()),
            );
        let saved = s.path.is_some();
        let ready = s.settings.midi_out.is_some() && saved && track.is_some();
        let why = if !saved {
            "Save the project first: samples go into its folder."
        } else if s.settings.midi_out.is_none() {
            "Pick a MIDI output first."
        } else {
            ""
        };
        c = c.child(
            button("capture", "◉ Capture", accent, ready)
                .disabled(!ready)
                .on_press(|s: &mut App| start_capture(s)),
        );
        if !why.is_empty() {
            c = c.child(text(why).font(font::SMALL).color(theme::warn()));
        }
        if cu.progress.is_some() {
            c = c
                .child(live_bar(300.0, 6.0, accent, &cu.progress_live))
                .child(button("cancel", "Cancel", theme::danger(), false).on_press(
                    |s: &mut App| {
                        if let Some(p) = &s.ext.capture.progress {
                            p.cancel()
                        }
                    },
                ));
        }
        c
    };
    let mut status = row().gap(8.0);
    if let Some(b) = &cu.busy {
        status = status.child(text(b.clone()).font(font::LABEL).color(accent));
    }
    if let Some(e) = &s.audio_error {
        status = status.child(text(e.clone()).font(font::LABEL).color(theme::danger()));
    }
    col()
        .key("capture")
        .w_full()
        .h_full()
        .padding(22.0)
        .gap(16.0)
        .min_h(0.0)
        .child(screen_header("Capture", "Your Hydrasynth can't ship with the game: play it live here, then bake it into samples.", accent))
        .child(row().gap(8.0).child(text(format!("Track: {}", track.map_or("— pick one on Session".into(), |t| t.name.clone()))).font(font::HEAD).color(theme::text())))
        .child(status)
        .child(col().grow(1.0).min_h(0.0).scroll(rune::el::ScrollSpec { stick_bottom: false }).child(row().gap(14.0).items_start().flex_wrap().child(col().w(420.0).gap(14.0).child(midi).child(live)).child(col().w(420.0).gap(14.0).child(input).child(capture))))
}

fn start_capture(s: &mut App) {
    let Some(dir) = s.path.clone() else { return };
    let port = s.settings.midi_out.clone().unwrap_or_default();
    let input = s.settings.audio_in.clone();
    let plan = s.ext.capture.plan(channel_of(s));
    let latency = s.ext.capture.latency_ms.unwrap_or(0.0);
    let name = s
        .selected_track
        .and_then(|t| s.project.track(t))
        .map_or("Capture".into(), |t| t.name.clone());
    let session = CaptureSession::new(input.as_deref());
    s.ext.capture.progress = Some(session.progress());
    let base = aa_midi::guess_device(&port)
        .map_or("capture", |d| d.friendly_name)
        .to_lowercase()
        .replace(' ', "_");
    spawn(s, "Recording… keep the room quiet", move || {
        let result = (|| -> Result<(aa_core::SamplerInstrument, usize), String> {
            let mut out = MidiOut::connect(&port).map_err(|e| e.to_string())?;
            let rec = session.run(&plan, &mut out).map_err(|e| e.to_string())?;
            let lat = aa_capture::latency_to_samples(latency, rec.sample_rate);
            let mut notes = aa_capture::slice_recording(&rec.samples, rec.sample_rate, &plan, lat);
            aa_capture::normalize_together(&mut notes, -3.0);
            let prefix = aa_capture::unused_prefix(&dir, &base);
            let inst =
                aa_capture::build_instrument(&format!("{name} (captured)"), &notes, &dir, &prefix)
                    .map_err(|e| e.to_string())?;
            let n = inst.zones.len();
            Ok((inst, n))
        })();
        Job::Captured(result)
    });
}
