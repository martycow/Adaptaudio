//! Application state: the project with undo history, the engine connection, and UI selection.
//!
//! Every edit goes through [`App::edit`] (one undo step) or a gesture ([`App::begin_gesture`] then
//! [`App::edit_quiet`] while dragging), and is sent to the audio thread as a new project snapshot.

use crate::live::LiveState;
use crate::settings::Settings;
use aa_core::{ClipId, Project, TrackId};
use aa_engine::{AudioHost, Command, Controller, Engine, LoadedProject, Notice};
use std::path::PathBuf;
use std::sync::Arc;

const UNDO_LIMIT: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Session,
    Sound,
    Capture,
    Mixer,
    Adaptive,
    Playtest,
    Learn,
    Settings,
}

impl Screen {
    pub const ALL: [Screen; 8] = [
        Screen::Session,
        Screen::Sound,
        Screen::Capture,
        Screen::Mixer,
        Screen::Adaptive,
        Screen::Playtest,
        Screen::Learn,
        Screen::Settings,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Screen::Session => "Session",
            Screen::Sound => "Sound Lab",
            Screen::Capture => "Capture",
            Screen::Mixer => "Mixer",
            Screen::Adaptive => "Adaptive",
            Screen::Playtest => "Playtest",
            Screen::Learn => "Learn",
            Screen::Settings => "Settings",
        }
    }

    /// What the screen is for, in a few words (the sidebar tooltip).
    pub fn about(self) -> &'static str {
        match self {
            Screen::Session => "Write the music: tracks, sections and notes",
            Screen::Sound => "Shape, save and load the sound of a track",
            Screen::Capture => "Record hardware and microphones into the project",
            Screen::Mixer => "Balance the tracks",
            Screen::Adaptive => "Rules that connect the game to the music",
            Screen::Playtest => "Listen to the project the way the game will play it",
            Screen::Learn => "Music words in plain language",
            Screen::Settings => "Devices, assistant, updates",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Screen::Session => "▦",
            Screen::Sound => "∿",
            Screen::Capture => "◉",
            Screen::Mixer => "≡",
            Screen::Adaptive => "⟳",
            Screen::Playtest => "▶",
            Screen::Learn => "?",
            Screen::Settings => "⚙",
        }
    }
}

/// A short message in the corner.
#[derive(Clone, Debug)]
pub struct Toast {
    pub id: u64,
    pub text: String,
    pub error: bool,
    pub born: f64,
}

/// A scenario Playtest is playing.
#[derive(Clone, Debug)]
pub struct ScenarioRun {
    pub index: usize,
    pub started: f64,
    pub next_step: usize,
}

/// A line in Playtest's event log.
#[derive(Clone, Debug)]
pub struct LogLine {
    pub at: f64,
    pub text: String,
    pub color: Option<String>,
}

/// Which tab each screen shows.
#[derive(Clone, Debug, Default)]
pub struct Tabs {
    pub adaptive: usize,
    pub learn: usize,
    pub learn_section: Option<String>,
    pub settings: usize,
    pub capture: usize,
    /// The transition being edited on the Adaptive screen: (from mode, to mode).
    pub transition: Option<(usize, usize)>,
}

pub struct App {
    pub project: Project,
    pub undo: Vec<Project>,
    pub redo: Vec<Project>,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub loaded: Arc<LoadedProject>,
    pub controller: Controller,
    /// `None` when there is no audio device (or headless snapshots).
    pub audio: Option<AudioHost>,
    pub audio_error: Option<String>,
    /// Why the MIDI output could not be opened, if it could not.
    pub midi_error: Option<String>,
    /// Without an audio device the engine still runs here, silently, so the UI stays alive.
    pub offline: Option<Engine>,
    pub offline_clock: f64,
    pub live: LiveState,
    pub settings: Settings,

    pub screen: Screen,
    pub selected_track: Option<TrackId>,
    pub selected_clip: Option<ClipId>,
    pub selected_note: Option<usize>,
    pub selected_mode: usize,
    /// The clip being auditioned in place of its section's clip: (track index, clip).
    pub audition: Option<(usize, ClipId)>,
    /// The project chooser shown at launch and by File → New.
    pub wizard: Option<crate::screens::wizard::Wizard>,
    pub tabs: Tabs,
    /// Play/stop asked by the UI and not yet confirmed by the engine: (wanted, asked at).
    transport_ask: Option<(bool, f64)>,
    pub sim_open: bool,
    /// Key of the dropdown that is open, if any.
    pub open_menu: Option<String>,
    pub assistant_open: bool,
    pub metronome: bool,
    pub now: f64,

    /// Discrete engine state mirrored for the view (rebuilt when it changes).
    pub mode: Option<usize>,
    pub pending_mode: Option<usize>,
    pub playing: bool,
    pub fired: u64,
    pub active_section: Option<u32>,
    pub clip_playing: Vec<u32>,

    pub scenario: Option<ScenarioRun>,
    pub log: Vec<LogLine>,
    pub toasts: Vec<Toast>,
    next_toast: u64,
    pub last_autosave: f64,
    pub glossary_query: rune::TextEdit,

    pub ext: crate::ext::Extras,
}

impl App {
    pub fn new(
        project: Project,
        path: Option<PathBuf>,
        settings: Settings,
        start_audio: bool,
    ) -> Self {
        let loaded = Arc::new(LoadedProject::load(project.clone(), path.as_deref(), None));
        let (sample_rate, device_ok) = if start_audio {
            match AudioHost::probe(settings.audio_device.as_deref()) {
                Ok(sr) => (sr as f32, Ok(())),
                Err(e) => (48_000.0, Err(e.to_string())),
            }
        } else {
            (48_000.0, Err("audio off".to_string()))
        };
        let (engine, controller) = Engine::new(Arc::clone(&loaded), sample_rate);
        let live = LiveState::new(&controller);
        let mut app = App {
            project,
            undo: Vec::new(),
            redo: Vec::new(),
            path,
            dirty: false,
            loaded,
            controller,
            audio: None,
            audio_error: None,
            midi_error: None,
            offline: None,
            offline_clock: 0.0,
            live,
            settings,
            screen: Screen::Session,
            selected_track: None,
            selected_clip: None,
            selected_note: None,
            selected_mode: 0,
            audition: None,
            wizard: None,
            tabs: Tabs::default(),
            transport_ask: None,
            sim_open: false,
            open_menu: None,
            assistant_open: false,
            metronome: false,
            now: 0.0,
            mode: None,
            pending_mode: None,
            playing: false,
            fired: 0,
            active_section: None,
            clip_playing: Vec::new(),
            scenario: None,
            log: Vec::new(),
            toasts: Vec::new(),
            next_toast: 1,
            last_autosave: 0.0,
            glossary_query: rune::TextEdit::new(""),
            ext: crate::ext::Extras::default(),
        };
        match device_ok {
            Ok(()) => app.start_audio(engine),
            Err(e) => {
                if start_audio {
                    app.audio_error = Some(e);
                }
                app.offline = Some(engine);
            }
        }
        app.ext.show_settings(&app.settings);
        app.selected_track = app.project.tracks.first().map(|t| t.id);
        app
    }

    fn start_audio(&mut self, mut engine: Engine) {
        crate::ext::wire_external_midi(&mut engine, self);
        match AudioHost::start(
            self.settings.audio_device.as_deref(),
            self.settings.buffer_frames,
            engine,
        ) {
            Ok(host) => self.audio = Some(host),
            Err(e) => self.audio_error = Some(e.to_string()),
        }
    }

    /// Re-opens the audio device (after a Settings change), keeping the project.
    pub fn restart_audio(&mut self) {
        self.audio = None;
        self.audio_error = None;
        let sr = match AudioHost::probe(self.settings.audio_device.as_deref()) {
            Ok(sr) => sr as f32,
            Err(e) => {
                self.audio_error = Some(e.to_string());
                return;
            }
        };
        let (engine, controller) = Engine::new(Arc::clone(&self.loaded), sr);
        self.live.rebind(&controller);
        self.controller = controller;
        self.start_audio(engine);
        if let Some(m) = self.mode {
            self.send(Command::RequestMode(m));
        }
    }

    /// Advances the silent engine to `now` (only when there is no audio device).
    pub fn run_offline(&mut self, now: f64) {
        let Some(engine) = self.offline.as_mut() else {
            return;
        };
        let dt = (now - self.offline_clock).clamp(0.0, 0.25);
        self.offline_clock = now;
        let frames = (dt * f64::from(engine.sample_rate())) as usize;
        if frames > 0 {
            let mut buf = vec![0.0; frames * 2];
            engine.process(&mut buf, 2);
        }
    }

    pub fn send(&mut self, command: Command) {
        if !self.controller.send(command) {
            self.toast_error("The audio engine is not responding");
        }
    }

    // ------------------------------------------------------------------ edits and undo

    /// One undoable edit.
    pub fn edit(&mut self, f: impl FnOnce(&mut Project)) {
        self.begin_gesture();
        self.edit_quiet(f);
    }

    /// Saves an undo point (the start of a drag).
    pub fn begin_gesture(&mut self) {
        self.undo.push(self.project.clone());
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// An edit without a new undo point (inside a drag).
    pub fn edit_quiet(&mut self, f: impl FnOnce(&mut Project)) {
        f(&mut self.project);
        self.dirty = true;
        self.push_project();
    }

    pub fn push_project(&mut self) {
        let next = Arc::new(LoadedProject::load(
            self.project.clone(),
            self.path.as_deref(),
            Some(&self.loaded),
        ));
        if !next.missing.is_empty() {
            let missing = next.missing.join(", ");
            self.toast_error(&format!("Missing samples: {missing}"));
        }
        self.loaded = Arc::clone(&next);
        self.send(Command::SetProject(next));
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.project, prev));
            self.dirty = true;
            self.push_project();
            self.fix_selection();
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.project, next));
            self.dirty = true;
            self.push_project();
            self.fix_selection();
        }
    }

    pub fn fix_selection(&mut self) {
        let track_ok = self
            .selected_track
            .and_then(|t| self.project.track(t))
            .is_some();
        if !track_ok {
            self.selected_track = self.project.tracks.first().map(|t| t.id);
        }
        let clip_ok = self
            .selected_track
            .zip(self.selected_clip)
            .and_then(|(t, c)| self.project.clip(t, c))
            .is_some();
        if !clip_ok {
            self.selected_clip = None;
            self.selected_note = None;
        }
    }

    /// Replaces the whole project (open, new).
    pub fn replace_project(&mut self, project: Project, path: Option<PathBuf>) {
        self.project = project;
        self.path = path;
        self.undo.clear();
        self.redo.clear();
        self.dirty = false;
        self.loaded = Arc::new(LoadedProject::load(
            self.project.clone(),
            self.path.as_deref(),
            None,
        ));
        let loaded = Arc::clone(&self.loaded);
        self.send(Command::Stop);
        self.send(Command::SetProject(loaded));
        self.send(Command::ResetAdaptive);
        self.selected_track = None;
        self.selected_clip = None;
        self.audition = None;
        self.fix_selection();
        self.scenario = None;
        self.playing = false;
        self.transport_ask = Some((false, self.now));
        self.log.clear();
    }

    // ------------------------------------------------------------------ selection helpers

    pub fn select_clip(&mut self, track: TrackId, clip: Option<ClipId>) {
        self.selected_track = Some(track);
        self.selected_clip = clip;
        self.selected_note = None;
    }

    // ------------------------------------------------------------------ transport and game

    pub fn toggle_play(&mut self) {
        self.set_playing(!self.playing);
    }

    /// The one place the transport starts and stops, so every Play button, key and menu item
    /// shows the same state.
    pub fn set_playing(&mut self, on: bool) {
        if on {
            self.send(Command::Play);
        } else {
            self.send(Command::Stop);
            self.scenario = None;
            self.live.scenario.set([0.0, 0.0]);
        }
        self.playing = on;
        self.transport_ask = Some((on, self.now));
    }

    /// Plays `clip` in place of the section's clip on its track, or stops doing so.
    pub fn set_audition(&mut self, audition: Option<(usize, ClipId)>) {
        if let Some((track, _)) = self.audition.filter(|a| Some(*a) != audition) {
            self.send(Command::SetClipOverride { track, clip: None });
        }
        if let Some((track, clip)) = audition {
            self.send(Command::SetClipOverride {
                track,
                clip: Some(clip),
            });
        }
        self.audition = audition;
    }

    pub fn request_mode(&mut self, mode: usize) {
        if let Some(m) = self.project.game_modes.get(mode) {
            let presets = m.param_presets.clone();
            let name = m.name.clone();
            let color = m.color.clone();
            for (param, value) in presets {
                if let Some(i) = self.project.param_index(&param) {
                    self.send(Command::SetParam { index: i, value });
                }
            }
            self.send(Command::RequestMode(mode));
            self.pending_mode = Some(mode);
            self.selected_mode = mode;
            self.log_line(format!("→ {name} requested"), Some(color));
        }
    }

    pub fn trigger_event(&mut self, event: usize) {
        if let Some(e) = self.project.events.get(event) {
            let name = e.name.clone();
            self.send(Command::TriggerEvent(event));
            self.log_line(format!("⚡ {name}"), None);
        }
    }

    pub fn set_param(&mut self, index: usize, value: f32) {
        self.send(Command::SetParam { index, value });
        self.live.set_param_target(index, value);
    }

    pub fn log_line(&mut self, text: String, color: Option<String>) {
        self.log.push(LogLine {
            at: self.now,
            text,
            color,
        });
        if self.log.len() > 60 {
            self.log.remove(0);
        }
    }

    pub fn toast(&mut self, text: &str) {
        let id = self.next_toast;
        self.next_toast += 1;
        self.toasts.push(Toast {
            id,
            text: text.to_string(),
            error: false,
            born: self.now,
        });
    }

    pub fn toast_error(&mut self, text: &str) {
        let id = self.next_toast;
        self.next_toast += 1;
        self.toasts.push(Toast {
            id,
            text: text.to_string(),
            error: true,
            born: self.now,
        });
    }

    /// The accent of the playing mode.
    pub fn accent(&self) -> rune::Color {
        self.mode
            .or(self.pending_mode)
            .and_then(|m| self.project.game_modes.get(m))
            .map_or_else(crate::theme::default_accent, |m| {
                crate::theme::mode_color(&m.color)
            })
    }

    /// Mirrors discrete engine state; true when the view should rebuild.
    pub fn sync_from_engine(&mut self) -> bool {
        self.controller.collect_garbage();
        let t = Arc::clone(&self.controller.telemetry);
        let mut changed = false;
        let mode = t.mode();
        if mode != self.mode {
            self.mode = mode;
            changed = true;
        }
        let pending = t.pending_mode();
        if pending != self.pending_mode
            && (pending.is_some() || self.mode == self.pending_mode || mode.is_some())
        {
            self.pending_mode = pending;
            changed = true;
        }
        // The engine answers a moment after the UI asks: until it does (or half a second
        // passes), what the UI asked for stands, so buttons never flicker back.
        let playing = t.is_playing();
        if let Some((wanted, at)) = self.transport_ask
            && (playing == wanted || self.now - at > 0.5)
        {
            self.transport_ask = None;
        }
        if self.transport_ask.is_none()
            && playing != self.playing
            && (self.audio.is_some() || self.offline.is_some())
        {
            self.playing = playing;
            if !playing {
                self.scenario = None;
            }
            changed = true;
        }
        let fired = t.fired.load(std::sync::atomic::Ordering::Relaxed);
        if fired != self.fired {
            self.fired = fired;
            changed = true;
        }
        let active = t.active_deck.load(std::sync::atomic::Ordering::Relaxed) as usize;
        let section = t.deck_section[active.min(1)].load(std::sync::atomic::Ordering::Relaxed);
        let section = (section != 0).then_some(section);
        if section != self.active_section {
            self.active_section = section;
            changed = true;
        }
        let clips: Vec<u32> = (0..self.project.tracks.len().min(aa_engine::MAX_TRACKS))
            .map(|i| t.tracks[i].clip.load(std::sync::atomic::Ordering::Relaxed))
            .collect();
        if clips != self.clip_playing {
            self.clip_playing = clips;
            changed = true;
        }
        for n in self.controller.drain_notices() {
            changed = true;
            match n {
                Notice::ModeSwitched { mode } => {
                    if let Some(m) = self.project.game_modes.get(mode) {
                        let (name, color) = (m.name.clone(), m.color.clone());
                        self.log_line(format!("● {name}"), Some(color));
                    }
                }
                Notice::EventFired { event } => {
                    if let Some(e) = self.project.events.get(event) {
                        let name = e.name.clone();
                        self.log_line(format!("  fired {name}"), None);
                    }
                }
                Notice::SectionStarted { .. } => {}
            }
        }
        changed
    }
}
