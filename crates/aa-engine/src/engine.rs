//! The real-time engine. [`Engine`] lives on the audio thread; [`Controller`] on the UI thread.
//!
//! Rules (see AGENTS.md): after [`Engine::new`], `process` never allocates, locks or does I/O.
//! Commands arrive through a lock-free ring; replaced projects go back through another ring so
//! the UI thread frees them.

use crate::dsp::{Rng, db_to_gain, pan_gains, saturate};
use crate::reverb::Reverb;
use crate::samples::LoadedProject;
use crate::telemetry::{MAX_TRACKS, Telemetry};
use crate::voice::{DECK_ONESHOT, Trigger, Voice, VoiceCtx};
use aa_adaptive::{AdaptiveEvent, AdaptiveState, TrackMods, next_grid_point};
use aa_core::{ClipId, Instrument, Note, Quantize, SectionId, TrackRole};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub const MAX_BLOCK: usize = 512;
pub const VOICES_PER_TRACK: usize = 12;
const MAX_ONESHOTS: usize = 8;
const COMMAND_CAPACITY: usize = 1024;

/// Messages from the UI to the audio thread.
#[derive(Debug)]
pub enum Command {
    /// Replace the project (after every edit). The old one comes back for freeing.
    SetProject(Arc<LoadedProject>),
    Play,
    Stop,
    /// The game sets a parameter (glides at its slew).
    SetParam {
        index: usize,
        value: f32,
    },
    JumpParam {
        index: usize,
        value: f32,
    },
    RampParam {
        index: usize,
        value: f32,
        seconds: f32,
    },
    RequestMode(usize),
    TriggerEvent(usize),
    /// Launch a section by hand (Session grid), on the next bar.
    LaunchSection(SectionId),
    /// Audition one clip on a track in place of its section slot (`None` restores the slot).
    SetClipOverride {
        track: usize,
        clip: Option<ClipId>,
    },
    /// Play one note on a track now (pads, keyboard, pattern editor).
    PreviewNote {
        track: usize,
        pitch: u8,
        velocity: f32,
        seconds: f32,
    },
    /// A key went down on a hardware keyboard: the note sounds until [`Command::NoteOff`].
    NoteOn {
        track: usize,
        pitch: u8,
        velocity: f32,
    },
    /// The key came up.
    NoteOff {
        track: usize,
        pitch: u8,
    },
    SetMetronome(bool),
    /// Back to parameter defaults, no mode, nothing unlocked.
    ResetAdaptive,
}

/// The longest a key held on a hardware keyboard sounds, in case its release is lost.
const HELD_NOTE_SECONDS: f32 = 30.0;

/// What the audio thread tells the UI happened.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Notice {
    ModeSwitched { mode: usize },
    EventFired { event: usize },
    SectionStarted { section: SectionId },
}

/// A note for a hardware synth, with the time it should sound (since [`Controller::epoch`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExternalNote {
    pub track: usize,
    pub channel: u8,
    pub pitch: u8,
    pub velocity: u8,
    pub on: bool,
    pub at: Duration,
}

/// Receives notes for External MIDI tracks. Called on the audio thread: must not block.
pub type ExternalSink = Box<dyn FnMut(ExternalNote) + Send>;

#[derive(Clone, Copy, Debug)]
struct Deck {
    section: Option<SectionId>,
    gain: f32,
    target: f32,
    /// Gain change per sample.
    rate: f32,
    origin: [f64; MAX_TRACKS],
    on: [bool; MAX_TRACKS],
}

impl Deck {
    const EMPTY: Deck = Deck {
        section: None,
        gain: 0.0,
        target: 0.0,
        rate: 1.0,
        origin: [0.0; MAX_TRACKS],
        on: [false; MAX_TRACKS],
    };

    fn audible(&self) -> bool {
        self.section.is_some() && (self.gain > 0.0 || self.target > 0.0)
    }
}

#[derive(Clone, Copy, Debug)]
struct OneShot {
    active: bool,
    track: usize,
    clip: ClipId,
    origin: f64,
}

#[derive(Clone, Copy, Debug)]
struct TrackRt {
    id: u32,
    voices: [Voice; VOICES_PER_TRACK],
    target: TrackMods,
    gain: f32,
    cutoff: f32,
    drive: f32,
    detune: f32,
    reverb: f32,
    wobble: f32,
    wobble_phase: f32,
    flutter_phase: f32,
    last_pitch: Option<f32>,
    hit: f32,
}

impl TrackRt {
    fn empty() -> Self {
        TrackRt {
            id: 0,
            voices: [Voice::default(); VOICES_PER_TRACK],
            target: TrackMods::NEUTRAL,
            gain: 1.0,
            cutoff: 1.0,
            drive: 0.0,
            detune: 0.0,
            reverb: 0.0,
            wobble: 0.0,
            wobble_phase: 0.0,
            flutter_phase: 0.0,
            last_pitch: None,
            hit: 0.0,
        }
    }
}

/// UI-side handle: sends commands, frees retired projects, reads telemetry.
pub struct Controller {
    commands: rtrb::Producer<Command>,
    garbage: rtrb::Consumer<Arc<LoadedProject>>,
    notices: rtrb::Consumer<Notice>,
    pub telemetry: Arc<Telemetry>,
    pub epoch: Instant,
}

impl Controller {
    /// Sends a command; false when the ring is full (the audio thread has stalled).
    pub fn send(&mut self, command: Command) -> bool {
        self.commands.push(command).is_ok()
    }

    /// Frees projects the audio thread has replaced. Call every frame.
    pub fn collect_garbage(&mut self) {
        while self.garbage.pop().is_ok() {}
    }

    pub fn drain_notices(&mut self) -> Vec<Notice> {
        let mut out = Vec::new();
        while let Ok(n) = self.notices.pop() {
            out.push(n);
        }
        out
    }
}

pub struct Engine {
    sample_rate: f32,
    loaded: Arc<LoadedProject>,
    retired: Option<Arc<LoadedProject>>,
    adaptive: AdaptiveState,
    playing: bool,
    pos: f64,
    decks: [Deck; 2],
    active: usize,
    pending_launch: Option<(SectionId, f64)>,
    oneshots: [OneShot; MAX_ONESHOTS],
    tracks: Vec<TrackRt>,
    overrides: [Option<ClipId>; MAX_TRACKS],
    reverb: Reverb,
    rng: Rng,
    sidechain_env: f32,
    kick_at: Option<usize>,
    metronome: bool,
    metro_env: f32,
    metro_freq: f32,
    metro_phase: f32,
    track_buf: Vec<f32>,
    deck_gains: [Vec<f32>; 3],
    sc_buf: Vec<f32>,
    mix_l: Vec<f32>,
    mix_r: Vec<f32>,
    send: Vec<f32>,
    events: Vec<AdaptiveEvent>,
    commands: rtrb::Consumer<Command>,
    garbage: rtrb::Producer<Arc<LoadedProject>>,
    notices: rtrb::Producer<Notice>,
    telemetry: Arc<Telemetry>,
    external: Option<ExternalSink>,
    epoch: Instant,
    block_time: Duration,
    age: u64,
    beat_pulse: f32,
    bar_pulse: f32,
    peaks: [f32; 2],
}

impl Engine {
    pub fn new(loaded: Arc<LoadedProject>, sample_rate: f32) -> (Engine, Controller) {
        let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(COMMAND_CAPACITY);
        let (garbage_tx, garbage_rx) = rtrb::RingBuffer::new(64);
        let (notice_tx, notice_rx) = rtrb::RingBuffer::new(256);
        let telemetry = Arc::new(Telemetry::new());
        telemetry.sample_rate.set(sample_rate);
        let epoch = Instant::now();
        let adaptive = AdaptiveState::new(&loaded.project);
        let mut engine = Engine {
            sample_rate,
            loaded,
            retired: None,
            adaptive,
            playing: false,
            pos: 0.0,
            decks: [Deck::EMPTY; 2],
            active: 0,
            pending_launch: None,
            oneshots: [OneShot {
                active: false,
                track: 0,
                clip: 0,
                origin: 0.0,
            }; MAX_ONESHOTS],
            tracks: (0..MAX_TRACKS).map(|_| TrackRt::empty()).collect(),
            overrides: [None; MAX_TRACKS],
            reverb: Reverb::new(sample_rate),
            rng: Rng::new(0x5EED_CEDA),
            sidechain_env: 0.0,
            kick_at: None,
            metronome: false,
            metro_env: 0.0,
            metro_freq: 1000.0,
            metro_phase: 0.0,
            track_buf: vec![0.0; MAX_BLOCK],
            deck_gains: [
                vec![0.0; MAX_BLOCK],
                vec![0.0; MAX_BLOCK],
                vec![1.0; MAX_BLOCK],
            ],
            sc_buf: vec![0.0; MAX_BLOCK],
            mix_l: vec![0.0; MAX_BLOCK],
            mix_r: vec![0.0; MAX_BLOCK],
            send: vec![0.0; MAX_BLOCK],
            events: Vec::with_capacity(16),
            commands: cmd_rx,
            garbage: garbage_tx,
            notices: notice_tx,
            telemetry: Arc::clone(&telemetry),
            external: None,
            epoch,
            block_time: Duration::ZERO,
            age: 0,
            beat_pulse: 0.0,
            bar_pulse: 0.0,
            peaks: [0.0; 2],
        };
        engine.sync_tracks();
        engine
            .reverb
            .set_size(engine.loaded.project.master.reverb_size);
        let controller = Controller {
            commands: cmd_tx,
            garbage: garbage_rx,
            notices: notice_rx,
            telemetry,
            epoch,
        };
        (engine, controller)
    }

    /// Where External MIDI notes go (the MIDI scheduler). Set before the stream starts.
    pub fn set_external_sink(&mut self, sink: ExternalSink) {
        self.external = Some(sink);
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// The instant [`ExternalNote::at`] counts from (same as [`Controller::epoch`]).
    pub fn epoch(&self) -> Instant {
        self.epoch
    }

    pub fn position(&self) -> f64 {
        self.pos
    }

    pub fn adaptive(&self) -> &AdaptiveState {
        &self.adaptive
    }

    fn samples_per_step(&self) -> f64 {
        f64::from(self.sample_rate) * self.loaded.project.seconds_per_step()
    }

    /// Fills `out` (interleaved, `channels` wide) with the next block of music.
    pub fn process(&mut self, out: &mut [f32], channels: usize) {
        let started = Instant::now();
        self.block_time = started.saturating_duration_since(self.epoch);
        self.drain_commands();
        if let Some(old) = self.retired.take()
            && let Err(rtrb::PushError::Full(old)) = self.garbage.push(old)
        {
            self.retired = Some(old);
        }
        let channels = channels.max(1);
        let frames = out.len() / channels;
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(MAX_BLOCK);
            self.render(n);
            for j in 0..n {
                let frame = &mut out[(done + j) * channels..(done + j + 1) * channels];
                if channels == 1 {
                    frame[0] = 0.5 * (self.mix_l[j] + self.mix_r[j]);
                } else {
                    frame[0] = self.mix_l[j];
                    frame[1] = self.mix_r[j];
                    for extra in frame.iter_mut().skip(2) {
                        *extra = 0.0;
                    }
                }
            }
            done += n;
        }
        self.publish(started.elapsed().as_secs_f32() / (frames.max(1) as f32 / self.sample_rate));
    }

    /// Renders `seconds` of interleaved stereo offline (tests, export).
    pub fn render_offline(&mut self, seconds: f32) -> Vec<f32> {
        let frames = (seconds * self.sample_rate).round() as usize;
        let mut out = vec![0.0; frames * 2];
        self.process(&mut out, 2);
        out
    }

    /// Applies a command directly (offline rendering without a controller).
    pub fn apply(&mut self, command: Command) {
        self.handle(command);
    }

    fn drain_commands(&mut self) {
        while let Ok(c) = self.commands.pop() {
            self.handle(c);
        }
    }

    fn handle(&mut self, command: Command) {
        let loaded = Arc::clone(&self.loaded);
        let project = &loaded.project;
        match command {
            Command::SetProject(next) => {
                let previous = std::mem::replace(&mut self.loaded, next);
                self.adaptive.sync_params(&self.loaded.project);
                self.sync_tracks();
                self.reverb.set_size(self.loaded.project.master.reverb_size);
                // The `loaded` clone above also holds the old project until this returns.
                drop(loaded);
                self.retire(previous);
            }
            Command::Play => self.play(),
            Command::Stop => self.stop(),
            Command::SetParam { index, value } => self.adaptive.set_param(index, value),
            Command::JumpParam { index, value } => self.adaptive.jump_param(index, value),
            Command::RampParam {
                index,
                value,
                seconds,
            } => self.adaptive.ramp_param(index, value, seconds),
            Command::RequestMode(m) => {
                self.adaptive.request_mode(project, m, self.pos);
            }
            Command::TriggerEvent(e) => {
                self.adaptive.trigger_event(project, e, self.pos);
            }
            Command::LaunchSection(id) => {
                if self.playing {
                    self.pending_launch = Some((
                        id,
                        next_grid_point(self.pos, Quantize::Bar, project.steps_per_bar()),
                    ));
                } else {
                    self.switch_to(Some(id), 0.0, 0.0);
                }
            }
            Command::SetClipOverride { track, clip } => {
                if let Some(slot) = self.overrides.get_mut(track) {
                    *slot = clip;
                }
            }
            Command::PreviewNote {
                track,
                pitch,
                velocity,
                seconds,
            } => {
                let note = Note::new(0, pitch, velocity, 1.0);
                let gate = (seconds * self.sample_rate) as usize;
                self.fire(track, &note, 0, DECK_ONESHOT, Some(gate), true);
            }
            Command::NoteOn {
                track,
                pitch,
                velocity,
            } => {
                let note = Note::new(0, pitch, velocity, 1.0);
                let gate = (HELD_NOTE_SECONDS * self.sample_rate) as usize;
                self.fire(track, &note, 0, DECK_ONESHOT, Some(gate), true);
            }
            Command::NoteOff { track, pitch } => {
                if let Some(rt) = self.tracks.get_mut(track) {
                    for v in &mut rt.voices {
                        if v.active && v.deck == DECK_ONESHOT && v.pitch == pitch {
                            v.release();
                        }
                    }
                }
            }
            Command::SetMetronome(on) => self.metronome = on,
            Command::ResetAdaptive => self.adaptive.reset(project),
        }
    }

    fn retire(&mut self, old: Arc<LoadedProject>) {
        // A previous retiree that couldn't be queued yet waits its turn; never free here.
        if let Some(waiting) = self.retired.take()
            && let Err(rtrb::PushError::Full(w)) = self.garbage.push(waiting)
        {
            self.retired = Some(w);
            if let Err(rtrb::PushError::Full(o)) = self.garbage.push(old) {
                // Both slots are busy: keep the newest; the UI drains every frame.
                std::mem::forget(o);
            }
            return;
        }
        if let Err(rtrb::PushError::Full(old)) = self.garbage.push(old) {
            self.retired = Some(old);
        }
    }

    /// Matches runtime slots to the project's tracks; a slot whose track changed goes quiet.
    fn sync_tracks(&mut self) {
        let project = &self.loaded.project;
        for (i, rt) in self.tracks.iter_mut().enumerate() {
            let id = project.tracks.get(i).map_or(0, |t| t.id);
            if rt.id != id {
                *rt = TrackRt {
                    id,
                    ..TrackRt::empty()
                };
                if let Some(o) = self.overrides.get_mut(i) {
                    *o = None;
                }
            }
        }
    }

    fn play(&mut self) {
        let project = &self.loaded.project;
        self.pos = 0.0;
        self.playing = true;
        self.pending_launch = None;
        for o in &mut self.oneshots {
            o.active = false;
        }
        let section = self
            .adaptive
            .mode()
            .and_then(|m| project.game_modes.get(m))
            .and_then(|m| m.section)
            .or(self.decks[self.active].section)
            .or_else(|| project.sections.first().map(|s| s.id));
        self.decks = [Deck::EMPTY; 2];
        self.switch_to(section, 0.0, 0.0);
    }

    fn stop(&mut self) {
        self.playing = false;
        for rt in &mut self.tracks {
            for v in &mut rt.voices {
                v.release();
            }
        }
    }

    /// Starts `section` on the idle deck at `at` and fades the active one out.
    fn switch_to(&mut self, section: Option<SectionId>, crossfade_beats: f32, at: f64) {
        let project = &self.loaded.project;
        let old = self.active;
        let new = 1 - old;
        let fade_samples = f64::from(crossfade_beats)
            * f64::from(aa_core::STEPS_PER_BEAT)
            * self.samples_per_step();
        let rate = if fade_samples >= 1.0 {
            (1.0 / fade_samples) as f32
        } else {
            1.0
        };
        let mut next = Deck {
            section,
            gain: if fade_samples >= 1.0 { 0.0 } else { 1.0 },
            target: 1.0,
            rate,
            origin: [at; MAX_TRACKS],
            on: [true; MAX_TRACKS],
        };
        let old_section = self.decks[old].section.and_then(|id| project.section(id));
        let new_section = section.and_then(|id| project.section(id));
        let old_audible = self.decks[old].audible();
        for (i, track) in project.tracks.iter().enumerate().take(MAX_TRACKS) {
            let before = old_section.and_then(|s| s.clip_for(track.id));
            let after = new_section.and_then(|s| s.clip_for(track.id));
            // Same clip in both sections: keep it playing, don't restart it.
            if old_audible && before.is_some() && before == after && self.decks[old].on[i] {
                next.origin[i] = self.decks[old].origin[i];
                next.gain = next.gain.max(0.0);
                self.decks[old].on[i] = false;
                for v in &mut self.tracks[i].voices {
                    if v.active && usize::from(v.deck) == old {
                        v.deck = new as u8;
                    }
                }
            }
        }
        self.decks[old].target = 0.0;
        self.decks[old].rate = rate;
        if fade_samples < 1.0 {
            self.decks[old].gain = 0.0;
            self.release_deck(old);
        }
        // Tracks carried over keep full level even while the deck fades in.
        self.decks[new] = next;
        self.active = new;
        if let Some(id) = section {
            let _ = self.notices.push(Notice::SectionStarted { section: id });
        }
    }

    fn release_deck(&mut self, deck: usize) {
        for rt in &mut self.tracks {
            for v in &mut rt.voices {
                if v.active && usize::from(v.deck) == deck {
                    v.release();
                }
            }
        }
    }

    fn render(&mut self, n: usize) {
        for buf in [&mut self.mix_l, &mut self.mix_r, &mut self.send] {
            buf[..n].fill(0.0);
        }
        let mut offset = 0;
        while offset < n {
            let remaining = n - offset;
            let mut chunk = remaining;
            if self.playing {
                let sps = self.samples_per_step();
                let end = self.pos + remaining as f64 / sps;
                let due = [
                    self.adaptive.next_due(),
                    self.pending_launch.map(|(_, at)| at),
                ]
                .into_iter()
                .flatten()
                .reduce(f64::min);
                if let Some(d) = due.filter(|d| *d > self.pos && *d < end) {
                    chunk = (((d - self.pos) * sps).ceil() as usize).clamp(1, remaining);
                }
            }
            self.render_chunk(offset, chunk);
            offset += chunk;
        }
        self.master(n);
    }

    fn render_chunk(&mut self, offset: usize, n: usize) {
        let loaded = Arc::clone(&self.loaded);
        let project = &loaded.project;
        let sr = self.sample_rate;
        let sps = self.samples_per_step();
        let dt = n as f32 / sr;
        let pos0 = self.pos;
        let pos1 = if self.playing {
            pos0 + n as f64 / sps
        } else {
            pos0
        };

        // Adaptive: what's due at the start of this chunk.
        self.events.clear();
        let due_pos = if self.playing { pos0 } else { f64::INFINITY };
        self.adaptive
            .advance(project, dt, due_pos, &mut self.events);
        for k in 0..self.events.len() {
            match self.events[k] {
                AdaptiveEvent::ModeSwitched {
                    to,
                    section,
                    crossfade_beats,
                    ..
                } => {
                    let at = if self.playing { pos0 } else { 0.0 };
                    self.switch_to(section, crossfade_beats, at);
                    let _ = self.notices.push(Notice::ModeSwitched { mode: to });
                }
                AdaptiveEvent::EventFired { event, .. } => {
                    if self.playing
                        && let Some(st) = project.events.get(event).and_then(|e| e.stinger)
                        && let Some(track) = project.track_index(st.track)
                        && let Some(slot) = self.oneshots.iter_mut().find(|o| !o.active)
                    {
                        *slot = OneShot {
                            active: true,
                            track,
                            clip: st.clip,
                            origin: pos0,
                        };
                    }
                    let _ = self.notices.push(Notice::EventFired { event });
                }
            }
        }
        if let Some((id, at)) = self.pending_launch.filter(|(_, at)| *at <= pos0) {
            let _ = at;
            self.pending_launch = None;
            self.switch_to(Some(id), 1.0, pos0);
        }

        // Track mods at the current parameter values.
        for (i, track) in project.tracks.iter().enumerate().take(MAX_TRACKS) {
            self.tracks[i].target = self.adaptive.track_mods(project, track);
        }

        // Deck gain ramps for this chunk.
        for d in 0..2 {
            let deck = &mut self.decks[d];
            let gains = &mut self.deck_gains[d];
            for g in gains.iter_mut().take(n) {
                if deck.gain < deck.target {
                    deck.gain = (deck.gain + deck.rate).min(deck.target);
                } else if deck.gain > deck.target {
                    deck.gain = (deck.gain - deck.rate).max(deck.target);
                }
                *g = deck.gain;
            }
            if deck.section.is_some() && deck.gain <= 0.0 && deck.target <= 0.0 {
                deck.section = None;
                self.release_deck_later(d);
            }
        }

        self.kick_at = None;
        if self.playing {
            self.sequence(pos0, pos1, sps);
            self.metronome_tick(pos0, pos1, sps, n);
        }

        // Sidechain envelope.
        let release = (-1.0 / (project.master.sidechain_release.max(0.01) * sr)).exp();
        for j in 0..n {
            if self.kick_at == Some(j) {
                self.sidechain_env = 1.0;
            }
            self.sc_buf[j] = self.sidechain_env;
            self.sidechain_env *= release;
        }

        self.render_tracks(offset, n);

        // Beat and bar pulses.
        if self.playing {
            let spb = f64::from(aa_core::STEPS_PER_BEAT);
            if (pos1 / spb).floor() > (pos0 / spb).floor() || (pos0 == 0.0 && pos1 > 0.0) {
                self.beat_pulse = 1.0;
            }
            let bar = f64::from(project.steps_per_bar());
            if (pos1 / bar).floor() > (pos0 / bar).floor() || (pos0 == 0.0 && pos1 > 0.0) {
                self.bar_pulse = 1.0;
            }
        }
        let decay = (-dt / 0.25).exp();
        self.beat_pulse *= decay;
        self.bar_pulse *= decay;
        self.pos = pos1;
    }

    fn release_deck_later(&mut self, deck: usize) {
        self.release_deck(deck);
    }

    fn sequence(&mut self, pos0: f64, pos1: f64, sps: f64) {
        let loaded = Arc::clone(&self.loaded);
        let project = &loaded.project;
        for (i, track) in project.tracks.iter().enumerate().take(MAX_TRACKS) {
            // External synths are triggered early by their measured latency so they land on time.
            let lead = match &track.instrument {
                Instrument::External(e) => {
                    f64::from(e.latency_ms) / 1000.0 / project.seconds_per_step()
                }
                _ => 0.0,
            };
            for d in 0..2 {
                let deck = self.decks[d];
                if !deck.audible() || !deck.on[i] {
                    continue;
                }
                let Some(section) = deck.section.and_then(|id| project.section(id)) else {
                    continue;
                };
                let clip_id = if d == self.active {
                    self.overrides[i].or_else(|| section.clip_for(track.id))
                } else {
                    section.clip_for(track.id)
                };
                let Some(clip) = clip_id.and_then(|c| track.clip(c)) else {
                    continue;
                };
                self.scan(
                    i,
                    clip,
                    deck.origin[i],
                    true,
                    pos0 + lead,
                    pos1 + lead,
                    d as u8,
                    sps,
                );
            }
        }
        for k in 0..MAX_ONESHOTS {
            let o = self.oneshots[k];
            if !o.active {
                continue;
            }
            let Some(track) = project.tracks.get(o.track) else {
                self.oneshots[k].active = false;
                continue;
            };
            let Some(clip) = track.clip(o.clip) else {
                self.oneshots[k].active = false;
                continue;
            };
            self.scan(
                o.track,
                clip,
                o.origin,
                false,
                pos0,
                pos1,
                DECK_ONESHOT,
                sps,
            );
            if pos1 >= o.origin + f64::from(clip.length_steps) {
                self.oneshots[k].active = false;
            }
        }
    }

    /// Fires the clip's notes whose time falls in `[from, to)`.
    #[allow(clippy::too_many_arguments)]
    fn scan(
        &mut self,
        track: usize,
        clip: &aa_core::Clip,
        origin: f64,
        looping: bool,
        from: f64,
        to: f64,
        deck: u8,
        sps: f64,
    ) {
        let len = i64::from(clip.length_steps.max(1));
        let rel0 = from - origin;
        let rel1 = to - origin;
        if rel1 <= 0.0 {
            return;
        }
        let j0 = (rel0 - 1.0).floor().max(0.0) as i64;
        let j1 = rel1.ceil() as i64;
        for j in j0..=j1 {
            if !looping && j >= len {
                break;
            }
            let step = if looping { j.rem_euclid(len) } else { j } as u16;
            let swing = if step % 2 == 1 {
                f64::from(clip.swing.clamp(0.0, 0.5))
            } else {
                0.0
            };
            let t = origin + j as f64 + swing;
            if t < from || t >= to {
                continue;
            }
            let delay = ((t - from) * sps) as usize;
            for note in clip.notes.iter().filter(|n| n.step == step) {
                self.fire(track, note, delay, deck, None, false);
            }
        }
    }

    fn fire(
        &mut self,
        index: usize,
        note: &Note,
        delay: usize,
        deck: u8,
        gate_override: Option<usize>,
        always: bool,
    ) {
        let loaded = Arc::clone(&self.loaded);
        let project = &loaded.project;
        let Some(track) = project.tracks.get(index) else {
            return;
        };
        if index >= MAX_TRACKS {
            return;
        }
        let sps = self.samples_per_step();
        let rt = &mut self.tracks[index];
        let chance = note.probability.clamp(0.0, 1.0) * rt.target.density.clamp(0.0, 1.0);
        if !always && chance < 1.0 && self.rng.unit() >= chance {
            return;
        }
        let gate = gate_override
            .unwrap_or_else(|| (f64::from(note.length_steps.max(0.05)) * sps) as usize);
        if track.role == TrackRole::Kick && !track.mixer.mute {
            self.kick_at = Some(self.kick_at.map_or(delay, |k| k.min(delay)));
        }
        rt.hit = 1.0;
        match &track.instrument {
            Instrument::External(ext) => {
                if let Some(sink) = self.external.as_mut() {
                    let at = self.block_time
                        + Duration::from_secs_f64(delay as f64 / f64::from(self.sample_rate));
                    let off =
                        at + Duration::from_secs_f64(gate as f64 / f64::from(self.sample_rate));
                    let velocity = (note.velocity.clamp(0.0, 1.0) * 127.0).round().max(1.0) as u8;
                    let channel = ext.channel.clamp(1, 16);
                    sink(ExternalNote {
                        track: index,
                        channel,
                        pitch: note.pitch,
                        velocity,
                        on: true,
                        at,
                    });
                    sink(ExternalNote {
                        track: index,
                        channel,
                        pitch: note.pitch,
                        velocity: 0,
                        on: false,
                        at: off,
                    });
                }
            }
            Instrument::Synth(patch) => {
                let glide_from = if patch.glide > 0.0 {
                    rt.last_pitch
                } else {
                    None
                };
                let slot = pick_voice(&rt.voices, note.pitch, track.role.is_drum());
                self.age += 1;
                let trigger = Trigger {
                    pitch: note.pitch,
                    velocity: note.velocity,
                    gate_samples: gate,
                    delay_samples: delay,
                    deck,
                    glide_from,
                    glide_seconds: patch.glide,
                };
                rt.voices[slot].start(trigger, Some(note), self.age);
                rt.last_pitch = Some(f32::from(note.pitch));
            }
            Instrument::Sampler(inst) => {
                let zone = pick_zone(inst, note.pitch, note.velocity);
                if zone.is_none() {
                    return;
                }
                let slot = pick_voice(&rt.voices, note.pitch, track.role.is_drum());
                self.age += 1;
                let trigger = Trigger {
                    pitch: note.pitch,
                    velocity: note.velocity,
                    gate_samples: gate,
                    delay_samples: delay,
                    deck,
                    glide_from: None,
                    glide_seconds: 0.0,
                };
                rt.voices[slot].start(trigger, Some(note), self.age);
                rt.voices[slot].set_zone(zone);
            }
        }
    }

    fn metronome_tick(&mut self, pos0: f64, pos1: f64, sps: f64, n: usize) {
        if !self.metronome {
            return;
        }
        let spb = f64::from(aa_core::STEPS_PER_BEAT);
        let beat = (pos0 / spb).ceil() * spb;
        if beat < pos1 {
            let delay = ((beat - pos0) * sps) as usize;
            let bar = f64::from(self.loaded.project.steps_per_bar());
            self.metro_freq = if (beat / bar).fract().abs() < 1e-9 {
                1500.0
            } else {
                1000.0
            };
            // The click starts `delay` samples in; `metro_env` is read in `master`.
            self.metro_env = 1.0;
            self.metro_phase = -(delay.min(n) as f32);
        }
    }

    fn render_tracks(&mut self, offset: usize, n: usize) {
        let loaded = Arc::clone(&self.loaded);
        let project = &loaded.project;
        let sr = self.sample_rate;
        let dt = n as f32 / sr;
        let k = 1.0 - (-dt / 0.04).exp();
        let any_solo = project.tracks.iter().any(|t| t.mixer.solo);
        for (i, track) in project.tracks.iter().enumerate().take(MAX_TRACKS) {
            let rt = &mut self.tracks[i];
            let target = rt.target;
            rt.gain += (target.volume - rt.gain) * k;
            rt.cutoff += (target.cutoff - rt.cutoff) * k;
            rt.drive += (target.drive - rt.drive) * k;
            rt.detune += (target.detune_cents - rt.detune) * k;
            rt.reverb += (target.reverb - rt.reverb) * k;
            rt.wobble += (target.wobble - rt.wobble) * k;
            rt.wobble_phase = (rt.wobble_phase + 0.55 * dt).fract();
            rt.flutter_phase = (rt.flutter_phase + 6.3 * dt).fract();
            let wobble_cents = rt.wobble
                * (28.0 * (rt.wobble_phase * std::f32::consts::TAU).sin()
                    + 7.0 * (rt.flutter_phase * std::f32::consts::TAU).sin());
            let ctx = VoiceCtx {
                sample_rate: sr,
                cutoff_mul: rt.cutoff,
                detune_cents: rt.detune,
                wobble_cents,
            };
            rt.hit *= (-dt / 0.15).exp();

            let buf = &mut self.track_buf[..n];
            buf.fill(0.0);
            let mut any = false;
            for v in rt.voices.iter_mut() {
                if !v.active {
                    continue;
                }
                any = true;
                let gains = &self.deck_gains[usize::from(v.deck.min(DECK_ONESHOT))][..n];
                match &track.instrument {
                    Instrument::Synth(patch) => {
                        v.render_synth(patch, &ctx, &mut self.rng, buf, gains)
                    }
                    Instrument::Sampler(inst) => {
                        let zone = v.zone();
                        let data = zone
                            .and_then(|z| {
                                loaded
                                    .zone_samples
                                    .get(i)
                                    .and_then(|zs| zs.get(z).copied().flatten())
                            })
                            .and_then(|s| loaded.samples.get(s));
                        match (zone.and_then(|z| inst.zones.get(z)), data) {
                            (Some(zd), Some(data)) => v.render_sampler(
                                inst,
                                data,
                                zd.root_note,
                                &ctx,
                                buf,
                                gains,
                                zd.loop_points,
                            ),
                            _ => v.kill(),
                        }
                    }
                    Instrument::External(_) => v.kill(),
                }
            }
            let tt = &self.telemetry.tracks[i];
            if !any {
                tt.level.set(tt.level.get() * 0.8);
                continue;
            }
            let patch_gain = match &track.instrument {
                Instrument::Synth(p) => db_to_gain(p.gain_db),
                Instrument::Sampler(s) => db_to_gain(s.gain_db),
                Instrument::External(_) => 0.0,
            };
            let audible = !track.mixer.mute && (!any_solo || track.mixer.solo);
            let fader = if audible {
                db_to_gain(track.mixer.volume_db) * patch_gain * rt.gain
            } else {
                0.0
            };
            let drive = (track.mixer.drive + rt.drive).min(1.0);
            let (pl, pr) = pan_gains(track.mixer.pan);
            let send = (track.mixer.reverb_send + rt.reverb).min(1.0);
            let sc = track.mixer.sidechain.clamp(0.0, 1.0);
            let mut peak: f32 = 0.0;
            for (j, &x) in buf.iter().enumerate() {
                let duck = 1.0 - 0.85 * sc * self.sc_buf[j];
                let y = saturate(x, drive) * fader * duck;
                peak = peak.max(y.abs());
                self.mix_l[offset + j] += y * pl;
                self.mix_r[offset + j] += y * pr;
                self.send[offset + j] += y * send;
            }
            tt.level.set(peak.max(tt.level.get() * 0.8));
        }
    }

    fn master(&mut self, n: usize) {
        let project = &self.loaded.project;
        let gain = db_to_gain(project.master.volume_db);
        let limiter = project.master.limiter;
        let sr = self.sample_rate;
        let mut peaks = [0.0f32; 2];
        for j in 0..n {
            let (wl, wr) = self.reverb.process(self.send[j]);
            let mut l = (self.mix_l[j] + wl) * gain;
            let mut r = (self.mix_r[j] + wr) * gain;
            if self.metro_env > 1e-3 {
                if self.metro_phase >= 0.0 {
                    let click = (self.metro_phase / sr * self.metro_freq * std::f32::consts::TAU)
                        .sin()
                        * self.metro_env
                        * 0.3;
                    l += click;
                    r += click;
                    self.metro_env *= (-1.0 / (0.03 * sr)).exp();
                }
                self.metro_phase += 1.0;
            }
            if limiter {
                l = l.tanh();
                r = r.tanh();
            }
            peaks[0] = peaks[0].max(l.abs());
            peaks[1] = peaks[1].max(r.abs());
            self.mix_l[j] = l;
            self.mix_r[j] = r;
        }
        self.peaks = [self.peaks[0].max(peaks[0]), self.peaks[1].max(peaks[1])];
    }

    fn publish(&mut self, load: f32) {
        let t = &self.telemetry;
        let project = &self.loaded.project;
        t.playing.store(self.playing, Ordering::Relaxed);
        t.bpm.set(project.tempo_bpm);
        t.position.set(self.pos);
        t.beat_pulse.set(self.beat_pulse);
        t.bar_pulse.set(self.bar_pulse);
        t.mode.store(
            self.adaptive.mode().map_or(-1, |m| m as i32),
            Ordering::Relaxed,
        );
        match self.adaptive.pending_mode() {
            Some(p) => {
                t.pending_mode.store(p.to as i32, Ordering::Relaxed);
                t.pending_at.set(p.at_step);
            }
            None => t.pending_mode.store(-1, Ordering::Relaxed),
        }
        for d in 0..2 {
            t.deck_section[d].store(self.decks[d].section.unwrap_or(0), Ordering::Relaxed);
            t.deck_gain[d].set(self.decks[d].gain);
        }
        t.active_deck.store(self.active as u32, Ordering::Relaxed);
        for i in 0..self
            .adaptive
            .param_count()
            .min(crate::telemetry::MAX_PARAMS)
        {
            t.params[i].set(self.adaptive.param(i));
            t.param_targets[i].set(self.adaptive.param_target(i));
        }
        t.fired.store(self.adaptive.fired_mask(), Ordering::Relaxed);
        let deck = self.decks[self.active];
        let section = deck.section.and_then(|id| project.section(id));
        for (i, track) in project.tracks.iter().enumerate().take(MAX_TRACKS) {
            let tt = &t.tracks[i];
            let rt = &self.tracks[i];
            tt.hit.set(rt.hit);
            tt.gain_mod.set(rt.gain);
            tt.cutoff_mod.set(rt.cutoff);
            let clip = self.overrides[i].or_else(|| section.and_then(|s| s.clip_for(track.id)));
            match clip.and_then(|c| track.clip(c)) {
                Some(c) if self.playing => {
                    tt.clip.store(c.id, Ordering::Relaxed);
                    let rel = (self.pos - deck.origin[i]).max(0.0);
                    tt.clip_step
                        .set((rel % f64::from(c.length_steps.max(1))) as f32);
                }
                Some(c) => {
                    tt.clip.store(c.id, Ordering::Relaxed);
                    tt.clip_step.set(-1.0);
                }
                None => {
                    tt.clip.store(0, Ordering::Relaxed);
                    tt.clip_step.set(-1.0);
                }
            }
        }
        t.master_peak[0].set(self.peaks[0]);
        t.master_peak[1].set(self.peaks[1]);
        self.peaks = [self.peaks[0] * 0.7, self.peaks[1] * 0.7];
        t.sidechain.set(self.sidechain_env);
        t.load.set(load);
        t.blocks.fetch_add(1, Ordering::Relaxed);
    }
}

/// Free voice, else the same pitch for drums (choke), else the oldest.
fn pick_voice(voices: &[Voice; VOICES_PER_TRACK], pitch: u8, drum: bool) -> usize {
    if drum && let Some(i) = voices.iter().position(|v| v.active && v.pitch == pitch) {
        return i;
    }
    if let Some(i) = voices.iter().position(|v| !v.active) {
        return i;
    }
    voices
        .iter()
        .enumerate()
        .min_by_key(|(_, v)| v.age)
        .map_or(0, |(i, _)| i)
}

/// The zone covering the note and velocity, else the zone with the nearest root.
fn pick_zone(inst: &aa_core::SamplerInstrument, pitch: u8, velocity: f32) -> Option<usize> {
    inst.zones
        .iter()
        .position(|z| {
            (z.low_note..=z.high_note).contains(&pitch)
                && velocity >= z.low_velocity
                && velocity <= z.high_velocity
        })
        .or_else(|| {
            inst.zones
                .iter()
                .enumerate()
                .min_by_key(|(_, z)| (i32::from(z.root_note) - i32::from(pitch)).abs())
                .map(|(i, _)| i)
        })
}
