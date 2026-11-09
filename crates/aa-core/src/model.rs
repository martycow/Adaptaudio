//! The project model. Everything the user makes lives in a [`Project`].

use crate::music::{PitchClass, ScaleKind};
use serde::{Deserialize, Serialize};

pub type TrackId = u32;
pub type ClipId = u32;
pub type SectionId = u32;
pub type PuzzleId = u32;

/// Current on-disk format. Bump it and add a migration in `io` when the shape changes.
pub const FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format_version: u32,
    pub name: String,
    pub tempo_bpm: f32,
    pub beats_per_bar: u8,
    pub key: PitchClass,
    pub scale: ScaleKind,
    /// Notes typed in the editors snap to the scale.
    pub scale_lock: bool,
    pub tracks: Vec<Track>,
    pub sections: Vec<Section>,
    pub game_params: Vec<ParamDef>,
    pub game_modes: Vec<GameModeDef>,
    pub transitions: Vec<TransitionRule>,
    pub default_transition: TransitionSpec,
    pub events: Vec<EventDef>,
    pub scenarios: Vec<Scenario>,
    pub puzzles: Vec<Puzzle>,
    pub master: MasterSettings,
    #[serde(default)]
    next_id: u32,
}

impl Project {
    /// An empty project with the standard game parameters and modes.
    pub fn new(name: impl Into<String>) -> Self {
        Project {
            format_version: FORMAT_VERSION,
            name: name.into(),
            tempo_bpm: 120.0,
            beats_per_bar: 4,
            key: PitchClass(2),
            scale: ScaleKind::NaturalMinor,
            scale_lock: true,
            tracks: Vec::new(),
            sections: Vec::new(),
            game_params: ParamDef::standard(),
            game_modes: Vec::new(),
            transitions: Vec::new(),
            default_transition: TransitionSpec::default(),
            events: Vec::new(),
            scenarios: Vec::new(),
            puzzles: Vec::new(),
            master: MasterSettings::default(),
            next_id: 1,
        }
    }

    /// A fresh id, unique across tracks, clips, sections and puzzles of this project.
    pub fn alloc_id(&mut self) -> u32 {
        let floor = self.max_used_id() + 1;
        self.next_id = self.next_id.max(floor);
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn max_used_id(&self) -> u32 {
        let tracks = self.tracks.iter().map(|t| t.id);
        let clips = self
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.id));
        let sections = self.sections.iter().map(|s| s.id);
        let puzzles = self.puzzles.iter().map(|p| p.id);
        tracks
            .chain(clips)
            .chain(sections)
            .chain(puzzles)
            .max()
            .unwrap_or(0)
    }

    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    pub fn track_index(&self, id: TrackId) -> Option<usize> {
        self.tracks.iter().position(|t| t.id == id)
    }

    pub fn section(&self, id: SectionId) -> Option<&Section> {
        self.sections.iter().find(|s| s.id == id)
    }

    pub fn section_mut(&mut self, id: SectionId) -> Option<&mut Section> {
        self.sections.iter_mut().find(|s| s.id == id)
    }

    pub fn section_index(&self, id: SectionId) -> Option<usize> {
        self.sections.iter().position(|s| s.id == id)
    }

    pub fn clip(&self, track: TrackId, clip: ClipId) -> Option<&Clip> {
        self.track(track)?.clips.iter().find(|c| c.id == clip)
    }

    pub fn clip_mut(&mut self, track: TrackId, clip: ClipId) -> Option<&mut Clip> {
        self.track_mut(track)?
            .clips
            .iter_mut()
            .find(|c| c.id == clip)
    }

    pub fn param_index(&self, name: &str) -> Option<usize> {
        self.game_params.iter().position(|p| p.name == name)
    }

    pub fn mode_index(&self, name: &str) -> Option<usize> {
        self.game_modes.iter().position(|m| m.name == name)
    }

    pub fn event_index(&self, name: &str) -> Option<usize> {
        self.events.iter().position(|e| e.name == name)
    }

    /// The transition to use between two game modes: the most specific rule wins.
    pub fn transition_between(&self, from: &str, to: &str) -> &TransitionSpec {
        let exact = self
            .transitions
            .iter()
            .find(|r| r.from.as_deref() == Some(from) && r.to.as_deref() == Some(to));
        let into = || {
            self.transitions
                .iter()
                .find(|r| r.from.is_none() && r.to.as_deref() == Some(to))
        };
        let out_of = || {
            self.transitions
                .iter()
                .find(|r| r.from.as_deref() == Some(from) && r.to.is_none())
        };
        exact
            .or_else(into)
            .or_else(out_of)
            .map(|r| &r.spec)
            .unwrap_or(&self.default_transition)
    }

    /// Length of one step in seconds at the project tempo.
    pub fn seconds_per_step(&self) -> f64 {
        60.0 / f64::from(self.tempo_bpm.max(1.0)) / f64::from(crate::STEPS_PER_BEAT)
    }

    pub fn steps_per_bar(&self) -> u32 {
        u32::from(self.beats_per_bar) * u32::from(crate::STEPS_PER_BEAT)
    }

    /// Adds a track with an empty clip in every section, returning its id.
    pub fn add_track(&mut self, name: impl Into<String>, role: TrackRole) -> TrackId {
        let id = self.alloc_id();
        let mut track = Track::new(id, name, role);
        track.instrument = Instrument::Synth(SynthPatch::preset(role));
        self.tracks.push(track);
        id
    }

    /// Adds a clip to a track and returns its id.
    pub fn add_clip(&mut self, track: TrackId, clip: Clip) -> Option<ClipId> {
        let id = self.alloc_id();
        let t = self.track_mut(track)?;
        t.clips.push(Clip { id, ..clip });
        Some(id)
    }

    pub fn add_section(&mut self, name: impl Into<String>, length_bars: u16) -> SectionId {
        let id = self.alloc_id();
        self.sections.push(Section {
            id,
            name: name.into(),
            length_bars,
            slots: Vec::new(),
        });
        id
    }

    /// Puts a clip into a section's slot for its track (or clears it with `None`).
    pub fn set_slot(&mut self, section: SectionId, track: TrackId, clip: Option<ClipId>) {
        let Some(s) = self.section_mut(section) else {
            return;
        };
        s.slots.retain(|slot| slot.track != track);
        if let Some(clip) = clip {
            s.slots.push(SectionSlot { track, clip });
        }
    }

    /// Removes a track and every reference to it.
    pub fn remove_track(&mut self, id: TrackId) {
        self.tracks.retain(|t| t.id != id);
        for s in &mut self.sections {
            s.slots.retain(|slot| slot.track != id);
        }
        for e in &mut self.events {
            if e.stinger.as_ref().is_some_and(|st| st.track == id) {
                e.stinger = None;
            }
            if e.unlock_track == Some(id) {
                e.unlock_track = None;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TrackRole {
    Kick,
    Snare,
    Hats,
    Perc,
    Bass,
    Pad,
    Lead,
    Arp,
    Ambience,
    Fx,
    Custom,
}

impl TrackRole {
    pub const ALL: [TrackRole; 11] = [
        TrackRole::Kick,
        TrackRole::Snare,
        TrackRole::Hats,
        TrackRole::Perc,
        TrackRole::Bass,
        TrackRole::Pad,
        TrackRole::Lead,
        TrackRole::Arp,
        TrackRole::Ambience,
        TrackRole::Fx,
        TrackRole::Custom,
    ];

    pub fn name(self) -> &'static str {
        match self {
            TrackRole::Kick => "Kick",
            TrackRole::Snare => "Snare",
            TrackRole::Hats => "Hats",
            TrackRole::Perc => "Perc",
            TrackRole::Bass => "Bass",
            TrackRole::Pad => "Pad",
            TrackRole::Lead => "Lead",
            TrackRole::Arp => "Arp",
            TrackRole::Ambience => "Ambience",
            TrackRole::Fx => "FX",
            TrackRole::Custom => "Custom",
        }
    }

    pub fn is_drum(self) -> bool {
        matches!(
            self,
            TrackRole::Kick | TrackRole::Snare | TrackRole::Hats | TrackRole::Perc
        )
    }

    /// The MIDI note new clips on this track default to.
    pub fn default_note(self) -> u8 {
        match self {
            TrackRole::Kick => 36,
            TrackRole::Snare => 38,
            TrackRole::Hats => 42,
            TrackRole::Perc => 39,
            TrackRole::Bass => 38,
            TrackRole::Pad => 62,
            TrackRole::Lead | TrackRole::Arp => 74,
            TrackRole::Ambience | TrackRole::Fx | TrackRole::Custom => 60,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    pub role: TrackRole,
    pub instrument: Instrument,
    pub clips: Vec<Clip>,
    pub mixer: MixerSettings,
    pub adaptive: TrackAdaptive,
}

impl Track {
    pub fn new(id: TrackId, name: impl Into<String>, role: TrackRole) -> Self {
        Track {
            id,
            name: name.into(),
            role,
            instrument: Instrument::Synth(SynthPatch::default()),
            clips: Vec::new(),
            mixer: MixerSettings::default(),
            adaptive: TrackAdaptive::default(),
        }
    }

    pub fn clip(&self, id: ClipId) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id == id)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MixerSettings {
    pub volume_db: f32,
    /// -1 = left, 1 = right.
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    /// 0..1 to the shared reverb.
    pub reverb_send: f32,
    /// 0..1 saturation.
    pub drive: f32,
    /// 0..1: how much this track ducks under the kick (sidechain).
    pub sidechain: f32,
}

impl Default for MixerSettings {
    fn default() -> Self {
        MixerSettings {
            volume_db: -6.0,
            pan: 0.0,
            mute: false,
            solo: false,
            reverb_send: 0.15,
            drive: 0.0,
            sidechain: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MasterSettings {
    pub volume_db: f32,
    /// Reverb room size 0..1.
    pub reverb_size: f32,
    /// Seconds the sidechain needs to recover after a kick.
    pub sidechain_release: f32,
    /// Soft limiter on the master bus.
    pub limiter: bool,
}

impl Default for MasterSettings {
    fn default() -> Self {
        MasterSettings {
            volume_db: -3.0,
            reverb_size: 0.6,
            sidechain_release: 0.18,
            limiter: true,
        }
    }
}

/// How a track follows the game.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackAdaptive {
    /// 0 = ignores the game, 1 = fully driven by its response curves.
    pub influence: f32,
    pub curves: Vec<ResponseCurve>,
    /// Layers that stay silent until a game event unlocks them (a solved puzzle).
    pub locked_until_event: Option<String>,
}

impl Default for TrackAdaptive {
    fn default() -> Self {
        TrackAdaptive {
            influence: 1.0,
            curves: Vec::new(),
            locked_until_event: None,
        }
    }
}

/// Maps one game parameter onto one sound property of the track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResponseCurve {
    pub param: String,
    pub target: CurveTarget,
    /// Piecewise-linear points (input 0..1, output), sorted by input.
    pub points: Vec<[f32; 2]>,
}

impl ResponseCurve {
    pub fn new(param: impl Into<String>, target: CurveTarget, points: &[[f32; 2]]) -> Self {
        ResponseCurve {
            param: param.into(),
            target,
            points: points.to_vec(),
        }
    }

    /// Evaluates the curve; outside the points it holds the end values.
    pub fn eval(&self, x: f32) -> f32 {
        let pts = &self.points;
        let Some(first) = pts.first() else {
            return self.target.neutral();
        };
        if x <= first[0] {
            return first[1];
        }
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            if x <= b[0] {
                let span = (b[0] - a[0]).max(1e-6);
                return a[1] + (b[1] - a[1]) * (x - a[0]) / span;
            }
        }
        pts.last().map_or(self.target.neutral(), |p| p[1])
    }
}

/// What a response curve drives. Output units are in each variant's doc.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CurveTarget {
    /// Gain multiplier 0..1.
    Volume,
    /// Filter cutoff multiplier, 0..1 maps to 1/16×..1× of the patch cutoff.
    FilterCutoff,
    /// Extra drive 0..1.
    Drive,
    /// Detune in cents, 0..100.
    Detune,
    /// Note probability multiplier 0..1 (density).
    Density,
    /// Extra reverb send 0..1.
    ReverbSend,
    /// Tape wobble depth 0..1 (pitch wow and flutter).
    Wobble,
}

impl CurveTarget {
    pub const ALL: [CurveTarget; 7] = [
        CurveTarget::Volume,
        CurveTarget::FilterCutoff,
        CurveTarget::Drive,
        CurveTarget::Detune,
        CurveTarget::Density,
        CurveTarget::ReverbSend,
        CurveTarget::Wobble,
    ];

    /// The value meaning "no change", which Influence = 0 settles on.
    pub fn neutral(self) -> f32 {
        match self {
            CurveTarget::Volume | CurveTarget::FilterCutoff | CurveTarget::Density => 1.0,
            CurveTarget::Drive
            | CurveTarget::Detune
            | CurveTarget::ReverbSend
            | CurveTarget::Wobble => 0.0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            CurveTarget::Volume => "Volume",
            CurveTarget::FilterCutoff => "Cutoff",
            CurveTarget::Drive => "Drive",
            CurveTarget::Detune => "Detune",
            CurveTarget::Density => "Density",
            CurveTarget::ReverbSend => "Reverb",
            CurveTarget::Wobble => "Wobble",
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Instruments

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Instrument {
    Synth(SynthPatch),
    Sampler(SamplerInstrument),
    /// A hardware synth played over MIDI (Live mode). Silent inside the engine.
    External(ExternalInstrument),
}

impl Instrument {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Instrument::Synth(_) => "Synth",
            Instrument::Sampler(_) => "Sampler",
            Instrument::External(_) => "External MIDI",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExternalInstrument {
    pub port_name: String,
    /// 1..=16.
    pub channel: u8,
    /// Round-trip latency compensation, measured on the Capture screen.
    pub latency_ms: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Waveform {
    Sine,
    Triangle,
    Saw,
    Square,
    Noise,
}

impl Waveform {
    pub const ALL: [Waveform; 5] = [
        Waveform::Sine,
        Waveform::Triangle,
        Waveform::Saw,
        Waveform::Square,
        Waveform::Noise,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Waveform::Sine => "Sine",
            Waveform::Triangle => "Triangle",
            Waveform::Saw => "Saw",
            Waveform::Square => "Square",
            Waveform::Noise => "Noise",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Oscillator {
    pub wave: Waveform,
    pub level: f32,
    pub octave: i8,
    pub detune_cents: f32,
    /// 0.05..0.95, for Square.
    pub pulse_width: f32,
}

impl Oscillator {
    pub fn new(wave: Waveform, level: f32) -> Self {
        Oscillator {
            wave,
            level,
            octave: 0,
            detune_cents: 0.0,
            pulse_width: 0.5,
        }
    }
}

/// Attack, decay and release in seconds; sustain level 0..1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Adsr {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Adsr {
    pub const fn new(attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        Adsr {
            attack,
            decay,
            sustain,
            release,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FilterMode {
    LowPass,
    HighPass,
    BandPass,
}

impl FilterMode {
    pub const ALL: [FilterMode; 3] = [
        FilterMode::LowPass,
        FilterMode::HighPass,
        FilterMode::BandPass,
    ];

    pub fn name(self) -> &'static str {
        match self {
            FilterMode::LowPass => "LP",
            FilterMode::HighPass => "HP",
            FilterMode::BandPass => "BP",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FilterSettings {
    pub mode: FilterMode,
    pub cutoff_hz: f32,
    /// 0..1.
    pub resonance: f32,
    /// Octaves the filter envelope opens the cutoff by.
    pub env_octaves: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LfoTarget {
    Pitch,
    Cutoff,
    Volume,
    PulseWidth,
}

impl LfoTarget {
    pub const ALL: [LfoTarget; 4] = [
        LfoTarget::Pitch,
        LfoTarget::Cutoff,
        LfoTarget::Volume,
        LfoTarget::PulseWidth,
    ];

    pub fn name(self) -> &'static str {
        match self {
            LfoTarget::Pitch => "Pitch",
            LfoTarget::Cutoff => "Cutoff",
            LfoTarget::Volume => "Volume",
            LfoTarget::PulseWidth => "PW",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lfo {
    pub rate_hz: f32,
    /// 0..1.
    pub depth: f32,
    pub target: LfoTarget,
}

/// A falling pitch at note start: a sine with a pitch envelope is a kick drum.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PitchEnv {
    pub semitones: f32,
    pub decay: f32,
}

/// The built-in two-oscillator subtractive synth.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SynthPatch {
    pub name: String,
    pub osc: [Oscillator; 2],
    pub noise: f32,
    pub amp_env: Adsr,
    pub filter: FilterSettings,
    pub filter_env: Adsr,
    pub lfo: Lfo,
    pub pitch_env: PitchEnv,
    /// Seconds to slide between notes (0 = off).
    pub glide: f32,
    pub gain_db: f32,
}

impl Default for SynthPatch {
    fn default() -> Self {
        SynthPatch {
            name: "Init".into(),
            osc: [
                Oscillator::new(Waveform::Saw, 0.8),
                Oscillator::new(Waveform::Square, 0.0),
            ],
            noise: 0.0,
            amp_env: Adsr::new(0.005, 0.2, 0.7, 0.2),
            filter: FilterSettings {
                mode: FilterMode::LowPass,
                cutoff_hz: 4000.0,
                resonance: 0.2,
                env_octaves: 1.0,
            },
            filter_env: Adsr::new(0.005, 0.3, 0.3, 0.3),
            lfo: Lfo {
                rate_hz: 5.0,
                depth: 0.0,
                target: LfoTarget::Pitch,
            },
            pitch_env: PitchEnv {
                semitones: 0.0,
                decay: 0.05,
            },
            glide: 0.0,
            gain_db: 0.0,
        }
    }
}

impl SynthPatch {
    /// A sensible starting sound for each track role.
    pub fn preset(role: TrackRole) -> Self {
        let d = SynthPatch::default();
        match role {
            TrackRole::Kick => SynthPatch {
                name: "Kick: sine drop".into(),
                osc: [
                    Oscillator::new(Waveform::Sine, 1.0),
                    Oscillator::new(Waveform::Triangle, 0.15),
                ],
                noise: 0.05,
                amp_env: Adsr::new(0.001, 0.32, 0.0, 0.08),
                filter: FilterSettings {
                    mode: FilterMode::LowPass,
                    cutoff_hz: 900.0,
                    resonance: 0.0,
                    env_octaves: 2.5,
                },
                filter_env: Adsr::new(0.001, 0.05, 0.0, 0.05),
                pitch_env: PitchEnv {
                    semitones: 36.0,
                    decay: 0.045,
                },
                gain_db: 2.0,
                ..d
            },
            TrackRole::Snare => SynthPatch {
                name: "Snare: noise + tone".into(),
                osc: [
                    Oscillator::new(Waveform::Triangle, 0.5),
                    Oscillator::new(Waveform::Sine, 0.0),
                ],
                noise: 0.8,
                amp_env: Adsr::new(0.001, 0.16, 0.0, 0.1),
                filter: FilterSettings {
                    mode: FilterMode::BandPass,
                    cutoff_hz: 2200.0,
                    resonance: 0.2,
                    env_octaves: 1.0,
                },
                filter_env: Adsr::new(0.001, 0.08, 0.0, 0.05),
                pitch_env: PitchEnv {
                    semitones: 12.0,
                    decay: 0.03,
                },
                ..d
            },
            TrackRole::Hats => SynthPatch {
                name: "Hats: bright noise".into(),
                osc: [
                    Oscillator::new(Waveform::Square, 0.0),
                    Oscillator::new(Waveform::Square, 0.0),
                ],
                noise: 0.9,
                amp_env: Adsr::new(0.001, 0.05, 0.0, 0.04),
                filter: FilterSettings {
                    mode: FilterMode::HighPass,
                    cutoff_hz: 7000.0,
                    resonance: 0.1,
                    env_octaves: 0.0,
                },
                gain_db: -4.0,
                ..d
            },
            TrackRole::Perc => SynthPatch {
                name: "Perc: metal click".into(),
                osc: [
                    Oscillator::new(Waveform::Square, 0.6),
                    Oscillator {
                        octave: 1,
                        detune_cents: 37.0,
                        ..Oscillator::new(Waveform::Square, 0.5)
                    },
                ],
                noise: 0.2,
                amp_env: Adsr::new(0.001, 0.09, 0.0, 0.06),
                filter: FilterSettings {
                    mode: FilterMode::BandPass,
                    cutoff_hz: 3200.0,
                    resonance: 0.5,
                    env_octaves: 0.5,
                },
                ..d
            },
            TrackRole::Bass => SynthPatch {
                name: "Bass: square drive".into(),
                osc: [
                    Oscillator::new(Waveform::Saw, 0.7),
                    Oscillator {
                        octave: -1,
                        ..Oscillator::new(Waveform::Square, 0.5)
                    },
                ],
                amp_env: Adsr::new(0.003, 0.25, 0.6, 0.08),
                filter: FilterSettings {
                    mode: FilterMode::LowPass,
                    cutoff_hz: 700.0,
                    resonance: 0.35,
                    env_octaves: 2.0,
                },
                filter_env: Adsr::new(0.001, 0.18, 0.2, 0.1),
                ..d
            },
            TrackRole::Pad => SynthPatch {
                name: "Pad: mossy saws".into(),
                osc: [
                    Oscillator::new(Waveform::Saw, 0.5),
                    Oscillator {
                        detune_cents: 12.0,
                        ..Oscillator::new(Waveform::Saw, 0.5)
                    },
                ],
                amp_env: Adsr::new(0.9, 0.8, 0.8, 1.6),
                filter: FilterSettings {
                    mode: FilterMode::LowPass,
                    cutoff_hz: 1400.0,
                    resonance: 0.15,
                    env_octaves: 0.5,
                },
                filter_env: Adsr::new(1.2, 1.0, 0.5, 1.5),
                lfo: Lfo {
                    rate_hz: 0.25,
                    depth: 0.25,
                    target: LfoTarget::Cutoff,
                },
                gain_db: -3.0,
                ..d
            },
            TrackRole::Lead => SynthPatch {
                name: "Lead: pulse chip".into(),
                osc: [
                    Oscillator {
                        pulse_width: 0.25,
                        ..Oscillator::new(Waveform::Square, 0.7)
                    },
                    Oscillator {
                        detune_cents: 6.0,
                        ..Oscillator::new(Waveform::Saw, 0.3)
                    },
                ],
                amp_env: Adsr::new(0.005, 0.15, 0.7, 0.18),
                filter: FilterSettings {
                    mode: FilterMode::LowPass,
                    cutoff_hz: 5000.0,
                    resonance: 0.25,
                    env_octaves: 1.0,
                },
                lfo: Lfo {
                    rate_hz: 5.5,
                    depth: 0.12,
                    target: LfoTarget::Pitch,
                },
                glide: 0.03,
                ..d
            },
            TrackRole::Arp => SynthPatch {
                name: "Arp: square pluck".into(),
                osc: [
                    Oscillator {
                        pulse_width: 0.5,
                        ..Oscillator::new(Waveform::Square, 0.7)
                    },
                    Oscillator {
                        octave: 1,
                        ..Oscillator::new(Waveform::Square, 0.2)
                    },
                ],
                amp_env: Adsr::new(0.001, 0.12, 0.0, 0.08),
                filter: FilterSettings {
                    mode: FilterMode::LowPass,
                    cutoff_hz: 3000.0,
                    resonance: 0.3,
                    env_octaves: 1.5,
                },
                filter_env: Adsr::new(0.001, 0.1, 0.0, 0.08),
                gain_db: -4.0,
                ..d
            },
            TrackRole::Ambience => SynthPatch {
                name: "Ambience: rain hiss".into(),
                osc: [
                    Oscillator::new(Waveform::Sine, 0.0),
                    Oscillator::new(Waveform::Sine, 0.0),
                ],
                noise: 0.6,
                amp_env: Adsr::new(2.0, 1.0, 1.0, 3.0),
                filter: FilterSettings {
                    mode: FilterMode::BandPass,
                    cutoff_hz: 2500.0,
                    resonance: 0.1,
                    env_octaves: 0.0,
                },
                lfo: Lfo {
                    rate_hz: 0.13,
                    depth: 0.4,
                    target: LfoTarget::Cutoff,
                },
                gain_db: -10.0,
                ..d
            },
            TrackRole::Fx => SynthPatch {
                name: "FX: riser".into(),
                osc: [
                    Oscillator::new(Waveform::Saw, 0.5),
                    Oscillator::new(Waveform::Noise, 0.0),
                ],
                noise: 0.4,
                amp_env: Adsr::new(1.5, 0.1, 1.0, 0.4),
                filter: FilterSettings {
                    mode: FilterMode::BandPass,
                    cutoff_hz: 600.0,
                    resonance: 0.6,
                    env_octaves: 3.0,
                },
                filter_env: Adsr::new(1.8, 0.1, 1.0, 0.3),
                pitch_env: PitchEnv {
                    semitones: -12.0,
                    decay: 1.5,
                },
                gain_db: -6.0,
                ..d
            },
            TrackRole::Custom => d,
        }
    }
}

/// A multisample instrument: each zone plays a WAV over a key range.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SamplerInstrument {
    pub name: String,
    pub zones: Vec<SampleZone>,
    pub amp_env: Adsr,
    pub gain_db: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SampleZone {
    /// Path relative to the project folder, e.g. `samples/pad_C3_v100.wav`.
    pub path: String,
    pub root_note: u8,
    pub low_note: u8,
    pub high_note: u8,
    /// Velocity range 0..1 this zone answers.
    pub low_velocity: f32,
    pub high_velocity: f32,
    pub loop_points: Option<[u32; 2]>,
}

// ---------------------------------------------------------------------------------------------
// Clips and sections

/// A clip: a looping pattern of notes on one track ("sequence" in AGENTS.md).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: ClipId,
    pub name: String,
    /// Length in steps (16 per 4/4 bar).
    pub length_steps: u16,
    pub notes: Vec<Note>,
    /// Swing 0..0.5: how late every second step lands, as a fraction of a step.
    pub swing: f32,
}

impl Clip {
    pub fn new(name: impl Into<String>, length_steps: u16) -> Self {
        Clip {
            id: 0,
            name: name.into(),
            length_steps: length_steps.max(1),
            notes: Vec::new(),
            swing: 0.0,
        }
    }

    /// Adds a note, keeping notes sorted by step.
    pub fn with_note(mut self, step: u16, pitch: u8, velocity: f32, length_steps: f32) -> Self {
        self.notes
            .push(Note::new(step, pitch, velocity, length_steps));
        self.notes.sort_by_key(|n| n.step);
        self
    }

    pub fn notes_at(&self, step: u16) -> impl Iterator<Item = &Note> {
        self.notes.iter().filter(move |n| n.step == step)
    }

    /// Toggles a note at a step: removes it if present, adds it otherwise.
    pub fn toggle(&mut self, step: u16, pitch: u8, velocity: f32) {
        if let Some(i) = self
            .notes
            .iter()
            .position(|n| n.step == step && n.pitch == pitch)
        {
            self.notes.remove(i);
        } else {
            self.notes.push(Note::new(step, pitch, velocity, 1.0));
            self.notes.sort_by_key(|n| n.step);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub step: u16,
    pub pitch: u8,
    /// 0..1.
    pub velocity: f32,
    pub length_steps: f32,
    /// 0..1: chance the note plays each loop.
    pub probability: f32,
    /// Per-note sound overrides ("parameter locks").
    pub locks: Vec<ParamLock>,
}

impl Note {
    pub fn new(step: u16, pitch: u8, velocity: f32, length_steps: f32) -> Self {
        Note {
            step,
            pitch,
            velocity,
            length_steps,
            probability: 1.0,
            locks: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParamLock {
    pub target: LockTarget,
    pub value: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LockTarget {
    /// Hz.
    Cutoff,
    /// 0..1.
    Resonance,
    /// Seconds.
    Decay,
    /// Semitones.
    Transpose,
    /// Osc 1 pulse width 0..1.
    PulseWidth,
}

impl LockTarget {
    pub const ALL: [LockTarget; 5] = [
        LockTarget::Cutoff,
        LockTarget::Resonance,
        LockTarget::Decay,
        LockTarget::Transpose,
        LockTarget::PulseWidth,
    ];

    pub fn name(self) -> &'static str {
        match self {
            LockTarget::Cutoff => "Cutoff",
            LockTarget::Resonance => "Resonance",
            LockTarget::Decay => "Decay",
            LockTarget::Transpose => "Transpose",
            LockTarget::PulseWidth => "Pulse width",
        }
    }
}

/// A set of clips that play together (a "scene" in Ableton terms).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub id: SectionId,
    pub name: String,
    pub length_bars: u16,
    pub slots: Vec<SectionSlot>,
}

impl Section {
    pub fn clip_for(&self, track: TrackId) -> Option<ClipId> {
        self.slots.iter().find(|s| s.track == track).map(|s| s.clip)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionSlot {
    pub track: TrackId,
    pub clip: ClipId,
}

// ---------------------------------------------------------------------------------------------
// The game side: parameters, modes, transitions, events, scenarios

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParamDef {
    pub name: String,
    pub default: f32,
    /// Seconds to travel the whole 0..1 range (FMOD's seek speed).
    pub slew_seconds: f32,
    pub about: String,
}

impl ParamDef {
    pub fn new(name: &str, default: f32, slew_seconds: f32, about: &str) -> Self {
        ParamDef {
            name: name.into(),
            default,
            slew_seconds,
            about: about.into(),
        }
    }

    pub fn standard() -> Vec<ParamDef> {
        vec![
            ParamDef::new("Intensity", 0.1, 2.0, "How much action and energy"),
            ParamDef::new("Tension", 0.1, 3.0, "How much suspense and unknown"),
            ParamDef::new(
                "Paranoia",
                0.0,
                4.0,
                "How much the opossum distrusts reality",
            ),
            ParamDef::new(
                "PuzzleProgress",
                0.0,
                1.0,
                "How solved the current puzzle is",
            ),
        ]
    }
}

/// A game mode ("режим"): Explore, Stealth, Chase… Each plays a section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameModeDef {
    pub name: String,
    pub section: Option<SectionId>,
    /// Accent colour as `#rrggbb`.
    pub color: String,
    /// Parameter values the Simulator jumps to when the mode is picked.
    pub param_presets: Vec<(String, f32)>,
    pub about: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransitionRule {
    /// `None` = from any mode.
    pub from: Option<String>,
    /// `None` = to any mode.
    pub to: Option<String>,
    pub spec: TransitionSpec,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransitionSpec {
    pub quantize: Quantize,
    /// Crossfade length in beats (0 = cut).
    pub crossfade_beats: f32,
    /// A one-shot played at the switch point (a drum fill, a stinger).
    pub stinger_event: Option<String>,
    /// The mode can't change again for this many seconds (hysteresis).
    pub min_dwell_seconds: f32,
}

impl Default for TransitionSpec {
    fn default() -> Self {
        TransitionSpec {
            quantize: Quantize::Bar,
            crossfade_beats: 1.0,
            stinger_event: None,
            min_dwell_seconds: 1.0,
        }
    }
}

/// Where a change may land.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Quantize {
    Immediate,
    Beat,
    Bar,
    /// Every N bars.
    Phrase(u16),
}

impl Quantize {
    pub fn name(self) -> String {
        match self {
            Quantize::Immediate => "Now".into(),
            Quantize::Beat => "Beat".into(),
            Quantize::Bar => "Bar".into(),
            Quantize::Phrase(n) => format!("{n} bars"),
        }
    }

    /// Length of the grid in steps (`None` for immediate).
    pub fn grid_steps(self, steps_per_bar: u32) -> Option<u32> {
        match self {
            Quantize::Immediate => None,
            Quantize::Beat => Some(u32::from(crate::STEPS_PER_BEAT)),
            Quantize::Bar => Some(steps_per_bar),
            Quantize::Phrase(n) => Some(steps_per_bar * u32::from(n.max(1))),
        }
    }
}

/// A one-shot signal from the game: Detected, PuzzleSolved…
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventDef {
    pub name: String,
    pub stinger: Option<Stinger>,
    pub quantize: Quantize,
    /// Unlocks every track whose `locked_until_event` names this event.
    pub unlocks: bool,
    pub unlock_track: Option<TrackId>,
    pub about: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stinger {
    pub track: TrackId,
    pub clip: ClipId,
}

/// A scripted run of the game, played by the Simulator.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    pub steps: Vec<ScenarioStep>,
}

impl Scenario {
    pub fn duration(&self) -> f32 {
        self.steps
            .iter()
            .map(|s| s.at_seconds + s.action.ramp_seconds())
            .fold(0.0, f32::max)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScenarioStep {
    pub at_seconds: f32,
    pub action: ScenarioAction,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ScenarioAction {
    SetMode(String),
    /// Ramp a parameter to `value` over `over_seconds`.
    SetParam {
        name: String,
        value: f32,
        over_seconds: f32,
    },
    Trigger(String),
}

impl ScenarioAction {
    pub fn ramp_seconds(&self) -> f32 {
        match self {
            ScenarioAction::SetParam { over_seconds, .. } => *over_seconds,
            _ => 0.0,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Puzzles

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Puzzle {
    pub id: PuzzleId,
    pub name: String,
    pub kind: PuzzleKind,
    /// How far ahead or behind a press may land and still count, in milliseconds.
    pub window_ms: f32,
    /// How much Paranoia may shift the beat the player hears, in milliseconds at Paranoia = 1.
    pub paranoia_shift_ms: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PuzzleKind {
    /// Press on these beats (in beats from the puzzle start).
    Timing { beats: Vec<f32>, lanes: Vec<u8> },
    /// Fill a lanes × steps grid so it matches `target` (lane, step) cells.
    Pattern {
        lanes: u8,
        steps: u16,
        target: Vec<[u16; 2]>,
        pitches: Vec<u8>,
    },
    /// Repeat the phrase: the game plays it, then the player answers one bar later.
    Echo {
        phrase: Vec<EchoNote>,
        answer_offset_beats: f32,
    },
}

impl PuzzleKind {
    pub fn name(&self) -> &'static str {
        match self {
            PuzzleKind::Timing { .. } => "Timing",
            PuzzleKind::Pattern { .. } => "Pattern",
            PuzzleKind::Echo { .. } => "Echo",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EchoNote {
    pub beat: f32,
    /// Which pad or key the player presses (0-based).
    pub lane: u8,
    pub pitch: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_never_repeat_even_after_loading() {
        let mut p = Project::new("t");
        let a = p.add_track("A", TrackRole::Kick);
        let mut loaded = p.clone();
        loaded.next_id = 0; // a file written without the counter
        let b = loaded.add_track("B", TrackRole::Bass);
        assert_ne!(a, b);
    }

    #[test]
    fn curves_interpolate_and_hold_their_ends() {
        let c = ResponseCurve::new(
            "Intensity",
            CurveTarget::Volume,
            &[[0.0, 0.0], [0.4, 0.0], [0.6, 1.0]],
        );
        assert_eq!(c.eval(-1.0), 0.0);
        assert_eq!(c.eval(0.2), 0.0);
        assert!((c.eval(0.5) - 0.5).abs() < 1e-6);
        assert_eq!(c.eval(0.9), 1.0);
    }

    #[test]
    fn the_most_specific_transition_wins() {
        let mut p = Project::new("t");
        let fast = TransitionSpec {
            quantize: Quantize::Beat,
            ..TransitionSpec::default()
        };
        let any_to_chase = TransitionSpec {
            quantize: Quantize::Phrase(4),
            ..TransitionSpec::default()
        };
        p.transitions.push(TransitionRule {
            from: None,
            to: Some("Chase".into()),
            spec: any_to_chase.clone(),
        });
        p.transitions.push(TransitionRule {
            from: Some("Alert".into()),
            to: Some("Chase".into()),
            spec: fast.clone(),
        });
        assert_eq!(p.transition_between("Alert", "Chase"), &fast);
        assert_eq!(p.transition_between("Stealth", "Chase"), &any_to_chase);
        assert_eq!(
            p.transition_between("Stealth", "Relief"),
            &p.default_transition
        );
    }

    #[test]
    fn removing_a_track_clears_its_references() {
        let mut p = Project::new("t");
        let t = p.add_track("Kick", TrackRole::Kick);
        let c = p.add_clip(t, Clip::new("beat", 16)).unwrap_or_default();
        let s = p.add_section("Chase", 4);
        p.set_slot(s, t, Some(c));
        p.events.push(EventDef {
            name: "Hit".into(),
            stinger: Some(Stinger { track: t, clip: c }),
            quantize: Quantize::Beat,
            unlocks: false,
            unlock_track: Some(t),
            about: String::new(),
        });
        p.remove_track(t);
        assert!(p.sections[0].slots.is_empty());
        assert!(p.events[0].stinger.is_none());
        assert!(p.events[0].unlock_track.is_none());
    }

    #[test]
    fn quantize_grids() {
        assert_eq!(Quantize::Immediate.grid_steps(16), None);
        assert_eq!(Quantize::Beat.grid_steps(16), Some(4));
        assert_eq!(Quantize::Bar.grid_steps(16), Some(16));
        assert_eq!(Quantize::Phrase(4).grid_steps(16), Some(64));
    }
}
