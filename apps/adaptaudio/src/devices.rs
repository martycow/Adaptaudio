//! Watches which MIDI and audio devices are plugged in, on a worker thread, and plays the
//! selected track from a hardware keyboard.
//!
//! Listing audio inputs can block while macOS asks for microphone access, so it never runs on
//! the UI thread, and only starts once a screen asks for it ([`Devices::want_audio_inputs`]).

use crate::state::App;
use aa_core::Instrument;
use aa_engine::Command;
use aa_midi::{MidiIn, MidiMessage};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

const SCAN_EVERY: Duration = Duration::from_secs(2);

pub enum Event {
    MidiOut(Vec<String>),
    MidiIn(Vec<String>),
    AudioIn(Result<Vec<String>, String>),
    AudioOut(Vec<String>),
    Key(MidiMessage),
}

pub struct Devices {
    pub midi_out: Vec<String>,
    pub midi_in: Vec<String>,
    /// `None` until the first scan (which waits for a screen to ask).
    pub audio_in: Option<Result<Vec<String>, String>>,
    /// Audio outputs, the system default first.
    pub audio_out: Vec<String>,
    /// True once the watcher has reported at least once.
    pub scanned: bool,
    want_audio: Arc<AtomicBool>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    running: bool,
    keyboard: Option<MidiIn>,
    /// Keys held on the hardware keyboard: (pitch, track index they sound on).
    held: Vec<(u8, usize)>,
    /// When a key last arrived, for the "keyboard is alive" light.
    pub last_key: Option<f64>,
}

impl Default for Devices {
    fn default() -> Self {
        let (tx, rx) = channel();
        Devices {
            midi_out: Vec::new(),
            midi_in: Vec::new(),
            audio_in: None,
            audio_out: Vec::new(),
            scanned: false,
            want_audio: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
            running: false,
            keyboard: None,
            held: Vec::new(),
            last_key: None,
        }
    }
}

impl Devices {
    /// Starts the watcher thread (once). `wake` nudges the UI when something changed.
    pub fn start(&mut self, wake: impl Fn() + Send + 'static) {
        if self.running {
            return;
        }
        self.running = true;
        let tx = self.tx.clone();
        let want_audio = Arc::clone(&self.want_audio);
        let spawned = std::thread::Builder::new()
            .name("adaptaudio-devices".into())
            .spawn(move || {
                let mut last_out: Option<Vec<String>> = None;
                let mut last_in: Option<Vec<String>> = None;
                let mut last_audio: Option<Result<Vec<String>, String>> = None;
                let mut last_speakers: Option<Vec<String>> = None;
                loop {
                    let mut changed = false;
                    let outs = aa_midi::list_outputs().unwrap_or_default();
                    if last_out.as_ref() != Some(&outs) {
                        last_out = Some(outs.clone());
                        changed |= tx.send(Event::MidiOut(outs)).is_ok();
                    }
                    let ins = aa_midi::list_inputs().unwrap_or_default();
                    if last_in.as_ref() != Some(&ins) {
                        last_in = Some(ins.clone());
                        if tx.send(Event::MidiIn(ins)).is_err() {
                            return;
                        }
                        changed = true;
                    }
                    let speakers = aa_engine::host::output_devices();
                    if last_speakers.as_ref() != Some(&speakers) {
                        last_speakers = Some(speakers.clone());
                        changed |= tx.send(Event::AudioOut(speakers)).is_ok();
                    }
                    if want_audio.load(Ordering::Relaxed) {
                        let audio = aa_capture::list_input_devices().map_err(|e| e.to_string());
                        if last_audio.as_ref() != Some(&audio) {
                            last_audio = Some(audio.clone());
                            changed |= tx.send(Event::AudioIn(audio)).is_ok();
                        }
                    }
                    if changed {
                        wake();
                    }
                    std::thread::sleep(SCAN_EVERY);
                }
            });
        if spawned.is_err() {
            self.running = false;
        }
    }

    /// A screen that shows audio inputs calls this; the first scan may make macOS ask for
    /// microphone access.
    pub fn want_audio_inputs(&self) {
        self.want_audio.store(true, Ordering::Relaxed);
    }

    pub fn keyboard_port(&self) -> Option<&str> {
        self.keyboard.as_ref().map(MidiIn::port_name)
    }
}

/// The MIDI port to use when the user has not chosen one: the first synth the studio knows.
fn first_synth(ports: &[String]) -> Option<String> {
    aa_midi::KNOWN_DEVICES
        .iter()
        .filter(|d| !d.is_audio_interface)
        .find_map(|d| {
            ports
                .iter()
                .find(|p| p.to_lowercase().contains(&d.port_hint.to_lowercase()))
        })
        .cloned()
}

/// Opens the hardware keyboard the settings ask for (or the first known synth).
fn connect_keyboard(s: &mut App) {
    let wanted = s
        .settings
        .midi_in
        .clone()
        .filter(|w| s.ext.devices.midi_in.contains(w))
        .or_else(|| first_synth(&s.ext.devices.midi_in));
    if wanted.as_deref() == s.ext.devices.keyboard_port() {
        return;
    }
    s.ext.devices.keyboard = None;
    let Some(port) = wanted else {
        return;
    };
    let tx = s.ext.devices.tx.clone();
    let wake = s.live.waker_fn();
    let connected = MidiIn::connect(&port, move |msg| {
        if msg.is_note() && tx.send(Event::Key(msg)).is_ok() {
            wake();
        }
    });
    match connected {
        Ok(input) => s.ext.devices.keyboard = Some(input),
        Err(e) => s.toast_error(&format!("MIDI keyboard “{port}”: {e}")),
    }
}

/// Re-opens the keyboard after the user picked another one.
pub fn reconnect_keyboard(s: &mut App) {
    s.ext.devices.keyboard = None;
    connect_keyboard(s);
}

/// A key on the hardware keyboard plays the selected track. External tracks are skipped: the
/// synth already plays itself, and echoing the note back would double it.
fn key(s: &mut App, msg: MidiMessage) {
    s.ext.devices.last_key = Some(s.now);
    match msg {
        MidiMessage::NoteOn { note, velocity, .. } if velocity > 0 => {
            let Some(track) = s.selected_track.and_then(|t| s.project.track_index(t)) else {
                return;
            };
            if matches!(s.project.tracks[track].instrument, Instrument::External(_)) {
                return;
            }
            s.ext.devices.held.push((note, track));
            s.send(Command::NoteOn {
                track,
                pitch: note,
                velocity: f32::from(velocity) / 127.0,
            });
        }
        MidiMessage::NoteOn { note, .. } | MidiMessage::NoteOff { note, .. } => {
            let held = std::mem::take(&mut s.ext.devices.held);
            let (up, still): (Vec<_>, Vec<_>) = held.into_iter().partition(|(p, _)| *p == note);
            s.ext.devices.held = still;
            for (pitch, track) in up {
                s.send(Command::NoteOff { track, pitch });
            }
        }
        _ => {}
    }
}

/// Takes in what the watcher found. True when the view should rebuild.
pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    let mut ports_changed = false;
    while let Ok(event) = s.ext.devices.rx.try_recv() {
        match event {
            Event::Key(msg) => {
                key(s, msg);
                continue;
            }
            Event::MidiOut(list) => {
                s.ext.devices.midi_out = list;
                ports_changed = true;
            }
            Event::MidiIn(list) => {
                s.ext.devices.midi_in = list;
                ports_changed = true;
            }
            Event::AudioIn(list) => s.ext.devices.audio_in = Some(list),
            Event::AudioOut(list) => s.ext.devices.audio_out = list,
        }
        s.ext.devices.scanned = true;
        changed = true;
    }
    if ports_changed {
        // A synth that was just plugged in is used without a trip to Settings.
        let outs = s.ext.devices.midi_out.clone();
        if s.settings.midi_out.is_none()
            && let Some(port) = first_synth(&outs)
        {
            s.settings.midi_out = Some(port);
            let _ = s.settings.save();
        }
        let wanted_here = s
            .settings
            .midi_out
            .as_ref()
            .is_some_and(|w| outs.contains(w));
        if wanted_here && s.ext.capture.scheduler.is_none() && s.audio.is_some() {
            s.restart_audio();
            if s.ext.capture.scheduler.is_some() {
                s.toast("MIDI output connected");
            }
        } else if !wanted_here && s.ext.capture.scheduler.take().is_some() {
            s.toast_error("The MIDI output was unplugged");
        }
        connect_keyboard(s);
    }
    changed
}
