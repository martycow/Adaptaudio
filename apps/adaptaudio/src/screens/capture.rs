//! Capture: record sound into the project. Three cases, one tab each:
//!
//! - a hardware synth, played note by note over MIDI and baked into a sampler (auto-sampling);
//! - a groovebox or any loop, recorded for a number of bars, started in time by MIDI clock;
//! - a microphone, a field recorder or a line input, recorded until Stop.
//!
//! Every recording runs on a worker thread and reports back through a channel. Devices are
//! found by the watcher in `crate::devices`.

use crate::state::App;
use crate::theme::{self, font};
use crate::widgets::{
    body, button, caps, card, dropdown, field_row, live_bar, screen_header, segmented, slider_row,
    vscroll,
};
use aa_capture::{CapturePlan, CaptureProgress, CaptureSession, InputChannel};
use aa_core::{Instrument, SamplerInstrument};
use aa_engine::{Engine, ExternalNote};
use aa_midi::{MidiMessage, MidiOut, MidiScheduler};
use rune::{Color, El, Live, col, el, row, text};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

pub const CASES: [&str; 3] = ["Hardware synth", "Groovebox or loop", "Microphone or line"];
const BARS: [u32; 4] = [1, 2, 4, 8];
/// Silence recorded before the groovebox is told to start.
const SYNC_PRE_ROLL_MS: u64 = 300;
/// The longest free recording.
const FREE_MAX_MS: u64 = 120_000;

pub enum Job {
    Latency(Result<f32, String>),
    /// A finished instrument and a line for the toast.
    Recorded(Result<(SamplerInstrument, String), String>),
}

pub struct CaptureUi {
    pub busy: Option<String>,
    pub rx: Option<Receiver<Job>>,
    pub progress: Option<Arc<CaptureProgress>>,
    pub progress_live: Live<[f32; 2]>,
    pub level_live: Live<[f32; 2]>,
    level: f32,
    pub low: u8,
    pub high: u8,
    pub step: u8,
    pub layers: usize,
    pub bars: u32,
    /// Send MIDI Start and clock to the groovebox while recording it.
    pub sync: bool,
    pub latency_ms: Option<f32>,
    /// Keeps the MIDI scheduler thread alive while the audio engine sends to it.
    pub scheduler: Option<MidiScheduler>,
}

impl Default for CaptureUi {
    fn default() -> Self {
        CaptureUi {
            busy: None,
            rx: None,
            progress: None,
            progress_live: Live::new([0.0, 0.0]),
            level_live: Live::new([0.0, 0.0]),
            level: 0.0,
            low: 36,
            high: 84,
            step: 3,
            layers: 2,
            bars: 2,
            sync: true,
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
    app.ext.capture.scheduler = None;
    let Some(port) = app.settings.midi_out.clone() else {
        return;
    };
    let scheduler = match MidiOut::connect(&port)
        .map_err(|e| e.to_string())
        .and_then(|out| MidiScheduler::start(out).map_err(|e| e.to_string()))
    {
        Ok(s) => s,
        Err(e) => {
            app.midi_error = Some(format!("MIDI output “{port}”: {e}"));
            return;
        }
    };
    app.midi_error = None;
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
        // The meter jumps up with the input and falls back slowly.
        let level = (p.take_peak().sqrt()).max(s.ext.capture.level * 0.85);
        s.ext.capture.level = level;
        s.ext.capture.level_live.set([0.0, level.clamp(0.0, 1.0)]);
    }
    let Some(rx) = &s.ext.capture.rx else {
        return false;
    };
    let job = match rx.try_recv() {
        Ok(j) => j,
        Err(TryRecvError::Empty) => {
            tx.wake_after(Duration::from_millis(50));
            return false;
        }
        Err(TryRecvError::Disconnected) => {
            finish(s);
            return true;
        }
    };
    finish(s);
    match job {
        Job::Latency(Ok(ms)) => {
            s.ext.capture.latency_ms = Some(ms);
            if let Some(tid) = s.selected_track
                && matches!(
                    s.project.track(tid).map(|t| &t.instrument),
                    Some(Instrument::External(_))
                )
            {
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
        Job::Recorded(Ok((inst, what))) => {
            if let Some(tid) = s.selected_track {
                s.edit(|p| {
                    if let Some(t) = p.track_mut(tid) {
                        t.instrument = Instrument::Sampler(inst)
                    }
                });
            }
            s.toast(&what);
        }
        Job::Latency(Err(e)) | Job::Recorded(Err(e)) => s.toast_error(&e),
    }
    true
}

/// Back to idle: the Record buttons are buttons again.
fn finish(s: &mut App) {
    let cu = &mut s.ext.capture;
    cu.rx = None;
    cu.busy = None;
    cu.progress = None;
    cu.level = 0.0;
    cu.progress_live.set([0.0, 0.0]);
    cu.level_live.set([0.0, 0.0]);
}

fn spawn(s: &mut App, what: &str, f: impl FnOnce() -> Job + Send + 'static) {
    if s.ext.capture.busy.is_some() {
        return;
    }
    let (tx, rx) = channel();
    let wake = s.live.waker_fn();
    s.ext.capture.rx = Some(rx);
    s.ext.capture.busy = Some(what.to_string());
    std::thread::spawn(move || {
        let _ = tx.send(f());
        wake();
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

fn input_channel(s: &App) -> InputChannel {
    match s.settings.audio_in_channel {
        0 => InputChannel::Mix,
        n => InputChannel::Only(usize::from(n) - 1),
    }
}

fn session(s: &App) -> CaptureSession {
    CaptureSession::new(s.settings.audio_in.as_deref()).with_input_channel(input_channel(s))
}

/// The MIDI port of the groovebox to start in time: the EP-1320 when it is plugged in.
fn sync_port(s: &App) -> Option<String> {
    let outs = &s.ext.devices.midi_out;
    outs.iter()
        .find(|p| aa_midi::guess_device(p).is_some_and(|d| d.port_hint == "EP-1320"))
        .or_else(|| {
            outs.iter()
                .find(|p| Some(*p) != s.settings.midi_out.as_ref())
        })
        .cloned()
}

/// Why recording can't start yet, in the user's words.
fn blocker(s: &App, needs_midi: bool) -> Option<&'static str> {
    if s.path.is_none() {
        Some("Save the project first: recordings are kept in its folder.")
    } else if s.selected_track.is_none() {
        Some("Add a track on the Session screen to record into.")
    } else if needs_midi && s.settings.midi_out.is_none() {
        Some("Connect the synth by USB: no MIDI output was found.")
    } else if matches!(s.ext.devices.audio_in, Some(Ok(ref l)) if l.is_empty()) {
        Some("No audio input was found. Plug in the audio interface.")
    } else {
        None
    }
}

pub fn view(s: &App, accent: Color) -> El {
    s.ext.devices.want_audio_inputs();
    let case = s.tabs.capture.min(CASES.len() - 1);
    let about = [
        "Adaptaudio plays the synth note by note over MIDI, records each note and builds a sampler from them. The track then sounds like the synth without it, and exports to FMOD.",
        "Records a pattern from a groovebox (the EP-1320) for a number of bars. Adaptaudio can start it in time over MIDI.",
        "Records whatever comes into the audio input until you press Stop: a voice, the ocarina, the tongue drum, a field recorder.",
    ][case];
    let right = match case {
        0 => synth_case(s, accent),
        1 => loop_case(s, accent),
        _ => free_case(s, accent),
    };
    let content = col()
        .gap(16.0)
        .child(
            col()
                .gap(8.0)
                .child(caps("What are you recording?"))
                .child(segmented(
                    "capture-case",
                    &CASES,
                    case,
                    accent,
                    |s: &mut App, i| {
                        if s.ext.capture.busy.is_none() {
                            s.tabs.capture = i
                        }
                    },
                ))
                .child(body(about).max_w(760.0)),
        )
        .child(
            row()
                .gap(16.0)
                .items_start()
                .flex_wrap()
                .child(col().w(440.0).gap(16.0).child(connections(s, case, accent)))
                .child(col().w(460.0).gap(16.0).child(right)),
        );
    col()
        .key("capture")
        .w_full()
        .h_full()
        .padding(22.0)
        .gap(16.0)
        .min_h(0.0)
        .child(screen_header(
            "Capture",
            "Record hardware and microphones into the project, so the game can play them.",
            accent,
        ))
        .child(vscroll(content))
}

/// Where the sound comes from and where it goes. The same for every case.
fn connections(s: &App, case: usize, accent: Color) -> El {
    let mut c = card().child(caps("Connections"));

    // Audio input.
    c = match &s.ext.devices.audio_in {
        None => c.child(body(
            "Looking for audio inputs… macOS may ask for microphone access: that is how any app is allowed to record.",
        )),
        Some(Err(e)) => c.child(
            text(format!("Audio inputs can't be listed: {e}"))
                .font(font::BODY)
                .color(theme::danger())
                .wrap(),
        ),
        Some(Ok(list)) => {
            let current = s
                .settings
                .audio_in
                .as_ref()
                .and_then(|a| list.iter().position(|n| n == a));
            c.child(field_row(
                "Audio input",
                dropdown(
                    s,
                    "audio-in",
                    list,
                    current,
                    280.0,
                    |s: &mut App, i, _| {
                        let name = match &s.ext.devices.audio_in {
                            Some(Ok(list)) => list.get(i).cloned(),
                            _ => None,
                        };
                        if name.is_some() {
                            s.settings.audio_in = name;
                            let _ = s.settings.save();
                        }
                    },
                ),
            ))
        }
    };
    c = c.child(field_row(
        "Record",
        segmented(
            "audio-in-ch",
            &["Both inputs", "Input 1", "Input 2"],
            usize::from(s.settings.audio_in_channel.min(2)),
            accent,
            |s: &mut App, i| {
                s.settings.audio_in_channel = i as u8;
                let _ = s.settings.save();
            },
        ),
    ));
    let hint = match s
        .settings
        .audio_in
        .as_deref()
        .and_then(aa_midi::guess_device)
        .map(|d| d.port_hint)
    {
        Some("Scarlett") => {
            "Scarlett Solo: input 1 is the microphone socket, input 2 the instrument or line socket. A synth plugged into input 2 is silent on input 1."
        }
        Some("EP-1320") => {
            "The EP-1320 sends its own sound over USB: both inputs are its left and right."
        }
        _ => "Pick the input your sound arrives on. Not sure? Use “Both inputs”.",
    };
    c = c.child(body(hint));

    // MIDI, for the cases that send it.
    if case == 0 {
        let outs = &s.ext.devices.midi_out;
        let current = s
            .settings
            .midi_out
            .as_ref()
            .and_then(|m| outs.iter().position(|o| o == m));
        c = c.child(field_row(
            "MIDI to synth",
            dropdown(s, "midi-out", outs, current, 280.0, |s: &mut App, i, _| {
                if let Some(name) = s.ext.devices.midi_out.get(i).cloned() {
                    s.settings.midi_out = Some(name);
                    let _ = s.settings.save();
                    s.restart_audio();
                }
            }),
        ));
        c = c.child(status_line(
            s.ext.capture.scheduler.is_some(),
            if outs.is_empty() {
                "No MIDI devices found yet. They appear here by themselves when plugged in."
            } else if s.ext.capture.scheduler.is_some() {
                "MIDI is connected."
            } else {
                "Choose the synth above."
            },
        ));
        if let Some(e) = &s.midi_error {
            c = c.child(
                text(e.clone())
                    .font(font::SMALL)
                    .color(theme::danger())
                    .wrap(),
            );
        }
    }

    // Destination.
    let names: Vec<String> = s.project.tracks.iter().map(|t| t.name.clone()).collect();
    let ti = s.selected_track.and_then(|t| s.project.track_index(t));
    c = c.child(field_row(
        "Record into",
        dropdown(
            s,
            "capture-track",
            &names,
            ti,
            280.0,
            |s: &mut App, i, _| {
                if let Some(id) = s.project.tracks.get(i).map(|t| t.id) {
                    s.select_clip(id, None);
                }
            },
        ),
    ));
    c = c.child(body(
        "The track becomes a sampler that plays the recording. Its old sound comes back with Undo.",
    ));
    if s.path.is_none() {
        c = c.child(
            row()
                .gap(10.0)
                .items_center()
                .child(
                    button("capture-save", "Save project…", accent, true)
                        .on_press_tx(crate::actions::save),
                )
                .child(
                    text("Recordings are kept in the project's folder.")
                        .font(font::SMALL)
                        .color(theme::warn()),
                ),
        );
    }
    c
}

fn status_line(ok: bool, words: &str) -> El {
    row()
        .gap(8.0)
        .items_center()
        .child(
            el().size(8.0, 8.0)
                .radius(4.0)
                .fill(if ok { theme::ok() } else { theme::faint() }),
        )
        .child(
            text(words.to_string())
                .font(font::SMALL)
                .color(theme::muted())
                .wrap(),
        )
}

type Press = fn(&mut App);

/// The Record button of every case: it turns into Stop while its recording runs.
fn record_row(
    s: &App,
    (key, label, stop_label): (&str, &str, &str),
    blocked: Option<&'static str>,
    accent: Color,
    (start, stop): (Press, Press),
) -> El {
    let cu = &s.ext.capture;
    let recording = cu.progress.is_some();
    let mut c = col().gap(10.0);
    if recording {
        c = c
            .child(
                row()
                    .gap(10.0)
                    .items_center()
                    .child(
                        button(format!("{key}-stop"), stop_label, theme::danger(), true)
                            .on_press(stop),
                    )
                    .child(
                        el().size(9.0, 9.0)
                            .radius(4.5)
                            .fill(theme::danger())
                            .live_opacity(&s.live.beat),
                    )
                    .child(
                        text(cu.busy.clone().unwrap_or_default())
                            .font(font::LABEL)
                            .color(theme::text()),
                    ),
            )
            .child(live_bar(420.0, 5.0, accent, &cu.progress_live))
            .child(
                row()
                    .gap(8.0)
                    .items_center()
                    .child(text("Input level").font(font::SMALL).color(theme::faint()))
                    .child(live_bar(330.0, 5.0, theme::ok(), &cu.level_live)),
            );
    } else {
        let busy = cu.busy.is_some();
        c = c.child(
            button(key.to_string(), label, accent, blocked.is_none() && !busy)
                .disabled(blocked.is_some() || busy)
                .on_press(start),
        );
        if let Some(why) = blocked {
            c = c.child(text(why).font(font::SMALL).color(theme::warn()).wrap());
        } else if let Some(b) = &cu.busy {
            c = c.child(text(b.clone()).font(font::SMALL).color(accent));
        }
    }
    c
}

fn cancel(s: &mut App) {
    if let Some(p) = &s.ext.capture.progress {
        p.cancel()
    }
}

fn finish_early(s: &mut App) {
    if let Some(p) = &s.ext.capture.progress {
        p.finish()
    }
}

// ------------------------------------------------------------------------- hardware synth

fn synth_case(s: &App, accent: Color) -> El {
    let cu = &s.ext.capture;
    let plan = cu.plan(channel_of(s));
    let busy = cu.busy.is_some();
    let latency = card()
        .child(caps("1 · Measure the delay (once)"))
        .child(body(
            "Plays one note and listens for it. Knowing the delay, every recorded note starts exactly at its beginning.",
        ))
        .child(
            row()
                .gap(10.0)
                .items_center()
                .child(
                    button("latency", "Measure", accent, cu.latency_ms.is_none())
                        .disabled(busy || blocker(s, true).is_some())
                        .on_press(|s: &mut App| {
                            let (port, ch, session) = (
                                s.settings.midi_out.clone().unwrap_or_default(),
                                channel_of(s),
                                session(s),
                            );
                            spawn(s, "Measuring the delay…", move || {
                                Job::Latency(
                                    MidiOut::connect(&port)
                                        .map_err(|e| e.to_string())
                                        .and_then(|mut out| {
                                            session
                                                .measure_midi_latency(&mut out, ch, 60, 100)
                                                .map_err(|e| e.to_string())
                                        }),
                                )
                            });
                        }),
                )
                .child(
                    text(cu.latency_ms.map_or_else(
                        || "Not measured yet".to_string(),
                        |ms| format!("{ms:.1} ms"),
                    ))
                    .font(font::LABEL)
                    .color(if cu.latency_ms.is_some() {
                        theme::ok()
                    } else {
                        theme::muted()
                    }),
                ),
        );
    let notes = card()
        .child(caps("2 · Which notes to record"))
        .child(slider_row(
            "lo",
            "Lowest",
            f32::from(cu.low - 24) / 60.0,
            aa_core::music::note_name(cu.low),
            200.0,
            accent,
            |s: &mut App, v, _| s.ext.capture.low = 24 + (v * 60.0) as u8,
        ))
        .child(slider_row(
            "hi",
            "Highest",
            f32::from(cu.high - 24) / 72.0,
            aa_core::music::note_name(cu.high),
            200.0,
            accent,
            |s: &mut App, v, _| s.ext.capture.high = 24 + (v * 72.0) as u8,
        ))
        .child(field_row(
            "A note every",
            row()
                .gap(8.0)
                .items_center()
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
        ))
        .child(
            field_row(
                "Loudness layers",
                segmented(
                    "clay",
                    &["1", "2", "3"],
                    cu.layers.clamp(1, 3) - 1,
                    accent,
                    |s: &mut App, i| s.ext.capture.layers = i + 1,
                ),
            )
            .help("Each note is recorded soft and loud, so playing softly sounds soft."),
        )
        .child(
            text(format!(
                "{} notes to record · about {:.0} seconds",
                plan.take_count(),
                plan.total_duration().as_secs_f32()
            ))
            .font(font::LABEL)
            .color(theme::text()),
        );
    let go = card()
        .child(caps("3 · Record"))
        .child(body(
            "Choose the sound on the synth first, and don't touch it while it plays.",
        ))
        .child(record_row(
            s,
            ("capture", "◉ Start recording", "■ Stop"),
            blocker(s, true),
            accent,
            (start_synth, cancel),
        ));
    col().gap(16.0).child(latency).child(notes).child(go)
}

fn start_synth(s: &mut App) {
    let Some(dir) = s.path.clone() else { return };
    let port = s.settings.midi_out.clone().unwrap_or_default();
    let plan = s.ext.capture.plan(channel_of(s));
    let latency = s.ext.capture.latency_ms.unwrap_or(0.0);
    let name = track_name(s);
    let session = session(s);
    s.ext.capture.progress = Some(session.progress());
    let base = file_base(aa_midi::guess_device(&port).map_or("synth", |d| d.friendly_name));
    spawn(s, "Recording the synth…", move || {
        let result = (|| -> Result<(SamplerInstrument, String), String> {
            let mut out = MidiOut::connect(&port).map_err(|e| e.to_string())?;
            let rec = session.run(&plan, &mut out).map_err(|e| e.to_string())?;
            let lat = aa_capture::latency_to_samples(latency, rec.sample_rate);
            let mut notes = aa_capture::slice_recording(&rec.samples, rec.sample_rate, &plan, lat);
            if notes.is_empty() {
                return Err("Nothing was heard. Check the input and the synth's volume.".into());
            }
            aa_capture::normalize_together(&mut notes, -3.0);
            let prefix = aa_capture::unused_prefix(&dir, &base);
            let inst =
                aa_capture::build_instrument(&format!("{name} (captured)"), &notes, &dir, &prefix)
                    .map_err(|e| e.to_string())?;
            let n = inst.zones.len();
            Ok((
                inst,
                format!("Recorded {n} notes into a sampler on “{name}”"),
            ))
        })();
        Job::Recorded(result)
    });
}

// ------------------------------------------------------------------------- groovebox / loop

fn bar_ms(s: &App) -> f64 {
    let p = &s.project;
    60_000.0 / f64::from(p.tempo_bpm.max(20.0)) * f64::from(p.beats_per_bar)
}

fn loop_case(s: &App, accent: Color) -> El {
    let cu = &s.ext.capture;
    let port = sync_port(s);
    let seconds = bar_ms(s) * f64::from(cu.bars) / 1000.0;
    let sync_words = match (&port, cu.sync) {
        (Some(p), true) => format!(
            "Adaptaudio presses Play on “{p}” and keeps it at {:.0} BPM.",
            s.project.tempo_bpm
        ),
        (Some(_), false) => "Press Play on the groovebox yourself right after Record.".to_string(),
        (None, _) => "No groovebox MIDI port found: press Play on it yourself right after Record."
            .to_string(),
    };
    let setup = card()
        .child(caps("1 · How long"))
        .child(field_row(
            "Length",
            row()
                .gap(8.0)
                .items_center()
                .child(segmented(
                    "loop-bars",
                    &["1 bar", "2 bars", "4 bars", "8 bars"],
                    BARS.iter().position(|b| *b == cu.bars).unwrap_or(1),
                    accent,
                    |s: &mut App, i| s.ext.capture.bars = BARS[i],
                ))
                .child(
                    text(format!("{seconds:.1} s at {:.0} BPM", s.project.tempo_bpm))
                        .font(font::LABEL)
                        .color(theme::muted()),
                ),
        ))
        .child(
            row()
                .gap(10.0)
                .items_center()
                .child(crate::widgets::toggle(
                    "loop-sync",
                    "Start it in time (MIDI clock)",
                    cu.sync && port.is_some(),
                    accent,
                    |s: &mut App| s.ext.capture.sync = !s.ext.capture.sync,
                ))
                .help("Sends MIDI Start and a steady clock, so the pattern begins with the recording and stays at the project's tempo."),
        )
        .child(body(sync_words));
    let go = card()
        .child(caps("2 · Record"))
        .child(body(
            "The loop becomes one sample on the track: the note C4 plays it at its recorded speed.",
        ))
        .child(record_row(
            s,
            ("loop-rec", "◉ Record the loop", "■ Stop"),
            blocker(s, false),
            accent,
            (start_loop, cancel),
        ));
    col().gap(16.0).child(setup).child(go)
}

fn start_loop(s: &mut App) {
    let Some(dir) = s.path.clone() else { return };
    let length_ms = (bar_ms(s) * f64::from(s.ext.capture.bars)).round() as u64;
    let bpm = s.project.tempo_bpm;
    let port = sync_port(s).filter(|_| s.ext.capture.sync);
    let name = track_name(s);
    let session = session(s);
    s.ext.capture.progress = Some(session.progress());
    spawn(s, "Recording the loop…", move || {
        let result = (|| -> Result<(SamplerInstrument, String), String> {
            let total = SYNC_PRE_ROLL_MS + length_ms;
            let rec = match &port {
                Some(port) => {
                    let mut out = MidiOut::connect(port).map_err(|e| e.to_string())?;
                    let events = aa_capture::clock_events(bpm, SYNC_PRE_ROLL_MS, length_ms);
                    session.record_synced(&events, total, &mut out)
                }
                None => session.record_audio(total),
            }
            .map_err(|e| e.to_string())?;
            let skip = aa_capture::ms_to_samples(SYNC_PRE_ROLL_MS, rec.sample_rate);
            // With no clock the player starts by hand, so the silence before it is cut.
            let note =
                aa_capture::one_shot(&rec.samples, rec.sample_rate, skip, port.is_none(), -3.0)
                    .ok_or(
                        "Nothing was heard. Check the input and that the groovebox is playing.",
                    )?;
            let inst = one_sample(&format!("{name} (loop)"), note, &dir, "loop")?;
            Ok((
                inst,
                format!("Recorded the loop onto “{name}”. Play C4 to hear it."),
            ))
        })();
        Job::Recorded(result)
    });
}

// ------------------------------------------------------------------------- microphone / line

fn free_case(s: &App, accent: Color) -> El {
    card()
        .child(caps("Record"))
        .child(body(
            "Press Record, make the sound, press Stop. Silence at both ends is cut off and the level is evened out. Two minutes at most.",
        ))
        .child(record_row(s, ("free-rec", "◉ Record", "■ Stop and keep"), blocker(s, false), accent, (start_free, finish_early)))
        .child(body(
            "The recording becomes one sample on the track: the note C4 plays it as recorded, other notes play it higher or lower.",
        ))
}

fn start_free(s: &mut App) {
    let Some(dir) = s.path.clone() else { return };
    let name = track_name(s);
    let session = session(s);
    s.ext.capture.progress = Some(session.progress());
    spawn(s, "Recording… press Stop when done", move || {
        let result = (|| -> Result<(SamplerInstrument, String), String> {
            let rec = session
                .record_audio(FREE_MAX_MS)
                .map_err(|e| e.to_string())?;
            let seconds = rec.samples.len() as f32 / rec.sample_rate.max(1) as f32;
            let note = aa_capture::one_shot(&rec.samples, rec.sample_rate, 0, true, -3.0)
                .ok_or("Nothing was heard. Check the input and its gain knob.")?;
            let inst = one_sample(&format!("{name} (recorded)"), note, &dir, "take")?;
            Ok((
                inst,
                format!("Recorded {seconds:.1} s onto “{name}”. Play C4 to hear it."),
            ))
        })();
        Job::Recorded(result)
    });
}

// ------------------------------------------------------------------------- shared

fn track_name(s: &App) -> String {
    s.selected_track
        .and_then(|t| s.project.track(t))
        .map_or("Capture".into(), |t| t.name.clone())
}

fn file_base(name: &str) -> String {
    name.to_lowercase().replace(' ', "_")
}

/// Writes one recording into the project's samples and returns a sampler that plays it on
/// every key, without looping.
fn one_sample(
    name: &str,
    note: aa_capture::CapturedNote,
    dir: &Path,
    base: &str,
) -> Result<SamplerInstrument, String> {
    let prefix = aa_capture::unused_prefix(dir, base);
    let mut inst =
        aa_capture::build_instrument(name, &[note], dir, &prefix).map_err(|e| e.to_string())?;
    for zone in &mut inst.zones {
        zone.loop_points = None;
    }
    Ok(inst)
}
