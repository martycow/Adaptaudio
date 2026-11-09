//! `manifest.json`: everything the FMOD Studio import script needs, built purely from a project.
//!
//! The mapping follows docs/ADAPTIVE_AUDIO.md, "Как это ложится на FMOD". Anything that has no
//! FMOD counterpart is listed in [`Manifest::warnings`] (prototype only) or, when the game code has
//! to do it, in [`Manifest::game_side`].

use crate::convert::{
    FmodQuantization, FmodTarget, OPEN_CUTOFF_HZ, SILENCE_DB, bake_curve, bars_to_frames,
    beats_to_ms, map_quantize, phrase_segments, sanitize_name, seconds_per_bar, seconds_to_frames,
    seek_speed, unique_keys,
};
use crate::export::{BitDepth, ExportOptions};
use aa_core::{ClipId, Instrument, Project, Quantize, SectionId, TrackId, TransitionSpec};
use serde::{Deserialize, Serialize};

pub const MANIFEST_VERSION: u32 = 1;
/// The labeled parameter that carries the game mode.
pub const MODE_PARAMETER: &str = "Mode";
/// Unlock parameters are named `Unlocked_<Event>`.
pub const UNLOCK_PREFIX: &str = "Unlocked_";
/// Folder (relative to the scene folder) the WAV files live in.
pub const STEMS_DIR: &str = "stems";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub manifest_version: u32,
    pub generator: String,
    pub project_name: String,
    /// Sanitized scene name: the export folder and the event name.
    pub scene: String,
    /// FMOD event path without the `event:/` prefix, e.g. `music/Cedar_Station`.
    pub event_path: String,
    pub tempo_bpm: f32,
    pub beats_per_bar: u8,
    pub key: String,
    pub scale: String,
    pub sample_rate: u32,
    pub bit_depth: BitDepth,
    pub channels: u16,
    pub timing: Timing,
    pub parameters: Vec<Parameter>,
    /// Sections laid out one after another on the event timeline.
    pub regions: Vec<Region>,
    pub destination_markers: Vec<DestinationMarker>,
    pub tracks: Vec<TrackSpec>,
    pub transitions: Vec<TransitionRegion>,
    pub events: Vec<EventSpec>,
    /// Every WAV in `stems/`. Everything else refers to these by `file`.
    pub stems: Vec<StemFile>,
    /// Tracks whose density curve needs extra stems rendered by hand (prototype only).
    pub density_variants: Vec<DensityVariant>,
    /// Things the Unity/C# side has to do, because FMOD doesn't.
    pub game_side: Vec<String>,
    /// Every feature that doesn't map to FMOD (prototype only) or maps only approximately.
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Timing {
    pub seconds_per_bar: f64,
    /// Exact frames per bar; may be fractional. Stem lengths round the whole span once.
    pub frames_per_bar: f64,
    /// Put this tempo marker at the start of the timeline so FMOD sends `TIMELINE_BEAT` callbacks.
    pub tempo_marker: TempoMarker,
    pub rounding: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TempoMarker {
    pub bar: u32,
    pub seconds: f64,
    pub tempo_bpm: f32,
    pub beats_per_bar: u8,
    /// Adaptaudio's grid is quarter-note beats.
    pub beat_unit: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterKind {
    Continuous,
    Labeled,
    /// Integer steps (the 0/1 unlock switches).
    Discrete,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub kind: ParameterKind,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    /// Units per second; 0 = no seeking (jumps at once).
    pub seek_speed: f32,
    /// Labels for a labeled parameter, in value order.
    pub labels: Vec<String>,
    pub about: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub name: String,
    pub key: String,
    pub start_bar: u32,
    pub length_bars: u32,
    pub start_seconds: f64,
    pub length_seconds: f64,
    /// Always true: every section is a loop region.
    #[serde(rename = "loop")]
    pub looping: bool,
    /// Game modes that play this section.
    pub modes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DestinationMarker {
    /// Named after the game mode it leads to.
    pub name: String,
    pub region: String,
    pub bar: u32,
    pub seconds: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackSpec {
    pub name: String,
    pub key: String,
    pub role: String,
    pub instrument_kind: String,
    /// Track fader. Stems are rendered pre-fader, so this is applied once, in FMOD.
    pub volume_db: f32,
    /// −1 = left, 1 = right (FMOD's panner uses −100..100).
    pub pan: f32,
    /// The static reverb send, already baked into the stems (for reference).
    pub reverb_send_baked: f32,
    pub instruments: Vec<InstrumentSpec>,
    pub automation: Vec<Automation>,
    /// The `Unlocked_<Event>` parameter that opens this layer, if it is locked.
    pub unlocked_by: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InstrumentSpec {
    pub region: String,
    pub stem: String,
    pub clip: String,
    pub start_bar: u32,
    pub length_bars: u32,
    pub start_seconds: f64,
    pub length_seconds: f64,
    #[serde(rename = "loop")]
    pub looping: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Automation {
    pub parameter: String,
    /// Human-readable origin, e.g. `Intensity → Volume`.
    pub source: String,
    pub target: FmodTarget,
    pub unit: String,
    /// FMOD effect to add to the track for this automation (`None` = the track itself).
    pub effect: Option<String>,
    /// The Influence that was baked into `points`.
    pub influence: f32,
    /// `[parameter value, FMOD value]`, sorted by parameter value; linear between points.
    pub points: Vec<[f32; 2]>,
    /// FMOD can't do this; the importer skips it.
    pub prototype_only: bool,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    pub parameter: String,
    pub label: String,
    /// The label's index: the value FMOD compares against.
    pub value: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub start_bar: u32,
    pub length_bars: u32,
    pub start_seconds: f64,
    pub length_seconds: f64,
}

/// One FMOD transition region: leaving `from_mode`'s section for `to_mode`'s marker.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransitionRegion {
    /// Which Adaptaudio rule produced it: `Stealth -> Alert`, `* -> PlayDead`, `default`…
    pub rule: String,
    pub from_mode: String,
    pub to_mode: String,
    pub source_region: String,
    pub destination_marker: String,
    pub condition: Condition,
    pub quantization: FmodQuantization,
    /// Where to place transition regions inside the source region (more than one for phrases).
    pub segments: Vec<Segment>,
    pub crossfade_beats: f32,
    pub crossfade_ms: f32,
    pub stinger_event: Option<String>,
    pub stinger_stem: Option<String>,
    /// Game-side hysteresis; FMOD has no equivalent.
    pub min_dwell_seconds: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventSpec {
    pub name: String,
    pub key: String,
    /// One-shot FMOD event for the stinger, e.g. `music/Cedar_Station_stingers/Detected`.
    pub event_path: Option<String>,
    pub stinger_stem: Option<String>,
    pub length_seconds: Option<f64>,
    /// Where the game should start the one-shot (on the next beat or bar callback).
    pub quantization: FmodQuantization,
    pub unlock_parameter: Option<String>,
    pub unlocks_tracks: Vec<String>,
    pub about: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StemKind {
    /// A section loop of one track.
    Loop,
    /// A one-shot for an event.
    Stinger,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StemFile {
    /// Relative to the scene folder, e.g. `stems/Explore__Pad.wav`.
    pub file: String,
    pub kind: StemKind,
    pub track: String,
    pub region: Option<String>,
    pub event: Option<String>,
    pub length_bars: Option<u32>,
    /// Frames (samples per channel).
    pub length_frames: u64,
    pub length_seconds: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DensityVariant {
    pub track: String,
    pub parameter: String,
    /// The stems that need a sparse and a dense take.
    pub stems: Vec<String>,
    /// Baked density curve `[parameter value, note probability]`.
    pub points: Vec<[f32; 2]>,
    pub suggestion: String,
}

/// What the exporter has to render, in manifest order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RenderJob {
    Loop {
        file: String,
        section: SectionId,
        track: TrackId,
        bars: u32,
    },
    Stinger {
        file: String,
        track: TrackId,
        clip: ClipId,
        frames: usize,
    },
}

pub(crate) struct Plan {
    pub manifest: Manifest,
    pub jobs: Vec<RenderJob>,
}

/// Builds the manifest for a project without rendering or writing anything.
pub fn build_manifest(project: &Project, options: &ExportOptions) -> Manifest {
    plan(project, options).manifest
}

/// The scene name used for the folder and the event: the option, else the project name.
pub fn scene_key(project: &Project, options: &ExportOptions) -> String {
    let name = if options.scene_name.trim().is_empty() {
        &project.name
    } else {
        &options.scene_name
    };
    sanitize_name(name)
}

pub(crate) fn plan(project: &Project, options: &ExportOptions) -> Plan {
    let mut b = Builder::new(project, options);
    b.regions();
    b.parameters();
    let unlocks = b.unlocks();
    b.tracks(&unlocks);
    b.events(&unlocks);
    b.transitions();
    b.project_warnings();
    b.finish()
}

struct Unlock {
    event: usize,
    parameter: String,
    tracks: Vec<usize>,
}

struct Builder<'a> {
    p: &'a Project,
    o: &'a ExportOptions,
    m: Manifest,
    jobs: Vec<RenderJob>,
    section_keys: Vec<String>,
    track_keys: Vec<String>,
    event_keys: Vec<String>,
}

impl<'a> Builder<'a> {
    fn new(p: &'a Project, o: &'a ExportOptions) -> Self {
        let scene = scene_key(p, o);
        let spb = seconds_per_bar(p.tempo_bpm, p.beats_per_bar);
        let m = Manifest {
            manifest_version: MANIFEST_VERSION,
            generator: format!("Adaptaudio aa-export {}", env!("CARGO_PKG_VERSION")),
            project_name: p.name.clone(),
            event_path: format!("music/{scene}"),
            scene,
            tempo_bpm: p.tempo_bpm,
            beats_per_bar: p.beats_per_bar,
            key: p.key.name().into(),
            scale: p.scale.name().into(),
            sample_rate: o.sample_rate,
            bit_depth: o.bit_depth,
            channels: 2,
            timing: Timing {
                seconds_per_bar: spb,
                frames_per_bar: spb * f64::from(o.sample_rate),
                tempo_marker: TempoMarker {
                    bar: 0,
                    seconds: 0.0,
                    tempo_bpm: p.tempo_bpm,
                    beats_per_bar: p.beats_per_bar,
                    beat_unit: 4,
                },
                rounding: "Loop stem length = round(length_bars × frames_per_bar), the whole span rounded \
                           once to the nearest frame (error < half a frame, no drift). Stinger length = \
                           round((clip seconds + tail) × sample_rate)."
                    .into(),
            },
            parameters: Vec::new(),
            regions: Vec::new(),
            destination_markers: Vec::new(),
            tracks: Vec::new(),
            transitions: Vec::new(),
            events: Vec::new(),
            stems: Vec::new(),
            density_variants: Vec::new(),
            game_side: Vec::new(),
            warnings: Vec::new(),
        };
        Builder {
            p,
            o,
            m,
            jobs: Vec::new(),
            section_keys: unique_keys(p.sections.iter().map(|s| s.name.as_str()), &["stinger"]),
            track_keys: unique_keys(p.tracks.iter().map(|t| t.name.as_str()), &[]),
            event_keys: unique_keys(p.events.iter().map(|e| e.name.as_str()), &[]),
        }
    }

    fn spb(&self) -> f64 {
        self.m.timing.seconds_per_bar
    }

    fn frames(&self, bars: u32) -> usize {
        bars_to_frames(
            bars,
            self.p.tempo_bpm,
            self.p.beats_per_bar,
            self.o.sample_rate,
        )
    }

    fn warn(&mut self, text: String) {
        self.m.warnings.push(text);
    }

    fn regions(&mut self) {
        let mut bar = 0u32;
        for (i, s) in self.p.sections.iter().enumerate() {
            if s.length_bars == 0 {
                self.m.warnings.push(format!(
                    "Section '{}' is 0 bars long; exported as 1 bar.",
                    s.name
                ));
            }
            let length_bars = u32::from(s.length_bars.max(1));
            let modes = self
                .p
                .game_modes
                .iter()
                .filter(|m| m.section == Some(s.id))
                .map(|m| m.name.clone())
                .collect();
            self.m.regions.push(Region {
                name: s.name.clone(),
                key: self.section_keys[i].clone(),
                start_bar: bar,
                length_bars,
                start_seconds: f64::from(bar) * self.spb(),
                length_seconds: f64::from(length_bars) * self.spb(),
                looping: true,
                modes,
            });
            bar += length_bars;
        }
    }

    fn parameters(&mut self) {
        for d in &self.p.game_params {
            self.m.parameters.push(Parameter {
                name: d.name.clone(),
                kind: ParameterKind::Continuous,
                min: 0.0,
                max: 1.0,
                default: d.default.clamp(0.0, 1.0),
                seek_speed: seek_speed(d.slew_seconds, 1.0),
                labels: Vec::new(),
                about: format!(
                    "{} (slew {} s → seek speed 1/slew units per second)",
                    d.about, d.slew_seconds
                ),
            });
        }
        if self.p.game_modes.is_empty() {
            self.warn(
                "The project has no game modes: no Mode parameter and no transitions are exported."
                    .into(),
            );
            return;
        }
        if self.p.param_index(MODE_PARAMETER).is_some() {
            self.warn(format!("A game parameter is called '{MODE_PARAMETER}', which clashes with the mode parameter."));
        }
        let labels: Vec<String> = self.p.game_modes.iter().map(|m| m.name.clone()).collect();
        self.m.parameters.push(Parameter {
            name: MODE_PARAMETER.into(),
            kind: ParameterKind::Labeled,
            min: 0.0,
            max: (labels.len() - 1) as f32,
            default: 0.0,
            seek_speed: 0.0,
            labels,
            about: "Game mode. The game sets it; transition regions jump to the mode's destination marker.".into(),
        });
    }

    fn unlocks(&mut self) -> Vec<Unlock> {
        let mut out = Vec::new();
        for (ei, e) in self.p.events.iter().enumerate() {
            let mut tracks: Vec<usize> = Vec::new();
            if e.unlocks {
                tracks.extend(
                    self.p
                        .tracks
                        .iter()
                        .enumerate()
                        .filter(|(_, t)| {
                            t.adaptive.locked_until_event.as_deref() == Some(e.name.as_str())
                        })
                        .map(|(i, _)| i),
                );
            }
            if let Some(i) = e.unlock_track.and_then(|id| self.p.track_index(id))
                && !tracks.contains(&i)
            {
                tracks.push(i);
            }
            if tracks.is_empty() {
                continue;
            }
            let parameter = format!("{UNLOCK_PREFIX}{}", self.event_keys[ei]);
            self.m.parameters.push(Parameter {
                name: parameter.clone(),
                kind: ParameterKind::Discrete,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                // Fade the layer in over one bar instead of clicking in.
                seek_speed: (1.0 / self.spb()) as f32,
                labels: Vec::new(),
                about: format!("0 = locked, 1 = unlocked by event '{}'.", e.name),
            });
            self.m.game_side.push(format!(
                "Event '{}': set parameter {parameter} = 1 on the music event (back to 0 when the scene resets).",
                e.name
            ));
            out.push(Unlock {
                event: ei,
                parameter,
                tracks,
            });
        }
        for t in &self.p.tracks {
            let Some(name) = &t.adaptive.locked_until_event else {
                continue;
            };
            match self.p.events.iter().find(|e| &e.name == name) {
                None => self.m.warnings.push(format!(
                    "Track '{}' is locked until event '{name}', which doesn't exist: it is exported unlocked.",
                    t.name
                )),
                Some(e) if !e.unlocks && e.unlock_track != Some(t.id) => self.m.warnings.push(format!(
                    "Track '{}' is locked until event '{name}', but that event doesn't unlock layers: exported unlocked.",
                    t.name
                )),
                Some(_) => {}
            }
        }
        out
    }

    fn tracks(&mut self, unlocks: &[Unlock]) {
        let stinger_tracks: Vec<TrackId> = self
            .p
            .events
            .iter()
            .filter_map(|e| e.stinger.map(|s| s.track))
            .collect();
        for (ti, t) in self.p.tracks.iter().enumerate() {
            let mut instruments = Vec::new();
            let mut probability_clips: Vec<String> = Vec::new();
            for (si, s) in self.p.sections.iter().enumerate() {
                let Some(clip_id) = s.clip_for(t.id) else {
                    continue;
                };
                let Some(clip) = t.clip(clip_id) else {
                    self.m.warnings.push(format!(
                        "Section '{}' points at a missing clip on '{}': skipped.",
                        s.name, t.name
                    ));
                    continue;
                };
                let region = &self.m.regions[si];
                let (start_bar, length_bars, region_name) =
                    (region.start_bar, region.length_bars, region.name.clone());
                let file = format!(
                    "{STEMS_DIR}/{}__{}.wav",
                    self.section_keys[si], self.track_keys[ti]
                );
                let frames = self.frames(length_bars);
                self.m.stems.push(StemFile {
                    file: file.clone(),
                    kind: StemKind::Loop,
                    track: t.name.clone(),
                    region: Some(region_name.clone()),
                    event: None,
                    length_bars: Some(length_bars),
                    length_frames: frames as u64,
                    length_seconds: frames as f64 / f64::from(self.o.sample_rate),
                });
                self.jobs.push(RenderJob::Loop {
                    file: file.clone(),
                    section: s.id,
                    track: t.id,
                    bars: length_bars,
                });
                instruments.push(InstrumentSpec {
                    region: region_name,
                    stem: file,
                    clip: clip.name.clone(),
                    start_bar,
                    length_bars,
                    start_seconds: f64::from(start_bar) * self.spb(),
                    length_seconds: f64::from(length_bars) * self.spb(),
                    looping: true,
                });
                if clip.notes.iter().any(|n| n.probability < 1.0)
                    && !probability_clips.contains(&clip.name)
                {
                    probability_clips.push(clip.name.clone());
                }
            }
            let used_by_stingers = stinger_tracks.contains(&t.id);
            if matches!(t.instrument, Instrument::External(_))
                && (!instruments.is_empty() || used_by_stingers)
            {
                self.warn(format!(
                    "Track '{}' plays an External MIDI instrument, which can't be rendered offline (prototype only): \
                     capture it on the Capture screen first, or its stems are silent.",
                    t.name
                ));
            }
            if instruments.is_empty() {
                if !used_by_stingers {
                    self.warn(format!(
                        "Track '{}' has no clip in any section and no stinger: not exported.",
                        t.name
                    ));
                }
                continue;
            }
            for clip in probability_clips {
                self.warn(format!(
                    "Track '{}', clip '{clip}': note probability (density) doesn't export (prototype only); \
                     the stem is one fixed take. Use an FMOD multi instrument with a play chance for randomness.",
                    t.name
                ));
            }
            if t.mixer.mute {
                self.warn(format!(
                    "Track '{}' is muted in Adaptaudio; it is exported anyway (mute it in FMOD).",
                    t.name
                ));
            }
            if t.mixer.sidechain > 0.0 {
                self.warn(format!(
                    "Track '{}': sidechain ducking ({:.2}) isn't in the stems; add an FMOD Sidechain compressor keyed \
                     from the kick.",
                    t.name, t.mixer.sidechain
                ));
            }
            let automation = self.automation(ti, &instruments);
            let unlock = unlocks.iter().find(|u| u.tracks.contains(&ti));
            let mut automation = automation;
            if let Some(u) = unlock {
                automation.push(Automation {
                    parameter: u.parameter.clone(),
                    source: format!("Unlocked by event '{}'", self.p.events[u.event].name),
                    target: FmodTarget::TrackVolume,
                    unit: FmodTarget::TrackVolume.unit().into(),
                    effect: None,
                    influence: 1.0,
                    points: vec![[0.0, SILENCE_DB], [1.0, 0.0]],
                    prototype_only: false,
                    note: "Locked layer: silent until the game sets the unlock parameter to 1."
                        .into(),
                });
            }
            self.m.tracks.push(TrackSpec {
                name: t.name.clone(),
                key: self.track_keys[ti].clone(),
                role: t.role.name().into(),
                instrument_kind: t.instrument.kind_name().into(),
                volume_db: t.mixer.volume_db,
                pan: t.mixer.pan.clamp(-1.0, 1.0),
                reverb_send_baked: t.mixer.reverb_send,
                instruments,
                automation,
                unlocked_by: unlock.map(|u| u.parameter.clone()),
            });
        }
    }

    fn automation(&mut self, ti: usize, instruments: &[InstrumentSpec]) -> Vec<Automation> {
        let t = &self.p.tracks[ti];
        let influence = t.adaptive.influence.clamp(0.0, 1.0);
        let (base_hz, base_note) = match &t.instrument {
            Instrument::Synth(patch) => (patch.filter.cutoff_hz, "the patch cutoff"),
            _ => (OPEN_CUTOFF_HZ, "fully open"),
        };
        let mut out = Vec::new();
        for c in &t.adaptive.curves {
            if self.p.param_index(&c.param).is_none() {
                self.m.warnings.push(format!(
                    "Track '{}': curve on unknown parameter '{}' skipped.",
                    t.name, c.param
                ));
                continue;
            }
            if influence <= 0.0 {
                continue;
            }
            let target = FmodTarget::for_curve(c.target);
            let baked = bake_curve(c, influence);
            let points: Vec<[f32; 2]> = baked
                .iter()
                .map(|&[x, v]| [x, target.value(v, base_hz)])
                .collect();
            let source = format!("{} → {}", c.param, c.target.name());
            let note = match target {
                FmodTarget::TrackVolume => "20·log10(gain), floor −80 dB, on top of the track fader.".into(),
                FmodTarget::LowpassCutoff => format!(
                    "Lowpass effect: 1 = {base_hz:.0} Hz ({base_note}), 0 = four octaves lower (logarithmic)."
                ),
                FmodTarget::DistortionLevel => "Extra drive on top of the static drive baked into the stem.".into(),
                FmodTarget::ReverbSendLevel => "Extra send to the reverb return, on top of the baked static send.".into(),
                FmodTarget::ChorusDepth => "Approximation: Adaptaudio detunes oscillators; Chorus depth % = cents.".into(),
                FmodTarget::FlangerMix => "Approximation of tape wow/flutter: slow Flanger (~0.3 Hz), mix % = depth × 100.".into(),
                FmodTarget::Density => "Prototype only: FMOD can't thin notes. See density_variants.".into(),
            };
            if target == FmodTarget::Density {
                self.m.warnings.push(format!(
                    "Track '{}': {source} doesn't map to FMOD, which can't thin notes (prototype only). Render \
                     density variants of {} stem(s) and crossfade them by {} (see density_variants).",
                    t.name,
                    instruments.len(),
                    c.param
                ));
                self.m.density_variants.push(DensityVariant {
                    track: t.name.clone(),
                    parameter: c.param.clone(),
                    stems: instruments.iter().map(|i| i.stem.clone()).collect(),
                    points: baked.clone(),
                    suggestion: format!(
                        "Render a sparse and a dense take of each stem (e.g. '{}_sparse.wav') and automate their \
                         volumes in opposite directions by {}.",
                        instruments.first().map_or("stem", |i| i.stem.trim_end_matches(".wav")),
                        c.param
                    ),
                });
            } else if target.is_approximate() {
                self.m.warnings.push(format!(
                    "Track '{}': {source} maps only approximately to an FMOD {} effect; tune it by ear.",
                    t.name,
                    target.effect().unwrap_or("track")
                ));
            }
            out.push(Automation {
                parameter: c.param.clone(),
                source,
                target,
                unit: target.unit().into(),
                effect: target.effect().map(Into::into),
                influence,
                points,
                prototype_only: target == FmodTarget::Density,
                note,
            });
        }
        out
    }

    fn events(&mut self, unlocks: &[Unlock]) {
        let tail = f64::from(self.o.stinger_tail_seconds.max(0.0));
        for (ei, e) in self.p.events.iter().enumerate() {
            let key = self.event_keys[ei].clone();
            let mut stem = None;
            let mut length_seconds = None;
            if let Some(st) = e.stinger {
                match self.p.clip(st.track, st.clip) {
                    None => self.m.warnings.push(format!(
                        "Event '{}': its stinger clip is missing; skipped.",
                        e.name
                    )),
                    Some(_) if !self.o.include_stingers => {}
                    Some(clip) => {
                        let seconds =
                            f64::from(clip.length_steps) * self.p.seconds_per_step() + tail;
                        let frames = seconds_to_frames(seconds, self.o.sample_rate);
                        let file = format!("{STEMS_DIR}/stinger__{key}.wav");
                        let track = self
                            .p
                            .track(st.track)
                            .map(|t| t.name.clone())
                            .unwrap_or_default();
                        self.m.stems.push(StemFile {
                            file: file.clone(),
                            kind: StemKind::Stinger,
                            track,
                            region: None,
                            event: Some(e.name.clone()),
                            length_bars: None,
                            length_frames: frames as u64,
                            length_seconds: frames as f64 / f64::from(self.o.sample_rate),
                        });
                        self.jobs.push(RenderJob::Stinger {
                            file: file.clone(),
                            track: st.track,
                            clip: st.clip,
                            frames,
                        });
                        stem = Some(file);
                        length_seconds = Some(frames as f64 / f64::from(self.o.sample_rate));
                    }
                }
            }
            let quantization = map_quantize(e.quantize);
            let event_path = stem
                .as_ref()
                .map(|_| format!("{}_stingers/{key}", self.m.event_path));
            if let Some(path) = &event_path {
                self.m.game_side.push(format!(
                    "Event '{}': start one-shot event '{path}' {}.",
                    e.name,
                    match e.quantize {
                        Quantize::Immediate => "at once".to_string(),
                        Quantize::Beat =>
                            "on the next TIMELINE_BEAT callback of the music event".to_string(),
                        Quantize::Bar =>
                            "on the next bar (TIMELINE_BEAT with beat == 1)".to_string(),
                        Quantize::Phrase(n) =>
                            format!("on the next {n}-bar boundary (count TIMELINE_BEAT bars)"),
                    }
                ));
            }
            let unlock = unlocks.iter().find(|u| u.event == ei);
            self.m.events.push(EventSpec {
                name: e.name.clone(),
                key,
                event_path,
                stinger_stem: stem,
                length_seconds,
                quantization,
                unlock_parameter: unlock.map(|u| u.parameter.clone()),
                unlocks_tracks: unlock.map_or_else(Vec::new, |u| {
                    u.tracks
                        .iter()
                        .map(|&i| self.p.tracks[i].name.clone())
                        .collect()
                }),
                about: e.about.clone(),
            });
        }
        if !self.o.include_stingers && self.p.events.iter().any(|e| e.stinger.is_some()) {
            self.warn("Stingers were left out of this export (include_stingers = false).".into());
        }
    }

    fn transitions(&mut self) {
        let modes: Vec<(usize, &aa_core::GameModeDef, usize)> = self
            .p
            .game_modes
            .iter()
            .enumerate()
            .filter_map(|(i, m)| Some((i, m, m.section.and_then(|s| self.p.section_index(s))?)))
            .collect();
        for m in &self.p.game_modes {
            if m.section.and_then(|s| self.p.section_index(s)).is_none() {
                self.m.warnings.push(format!(
                    "Game mode '{}' has no section: no marker, no transitions into it.",
                    m.name
                ));
            }
        }
        for &(_, m, si) in &modes {
            let r = &self.m.regions[si];
            self.m.destination_markers.push(DestinationMarker {
                name: m.name.clone(),
                region: r.name.clone(),
                bar: r.start_bar,
                seconds: r.start_seconds,
            });
        }
        for &(_, from, from_si) in &modes {
            for &(to_i, to, to_si) in &modes {
                if from.name == to.name || from_si == to_si {
                    continue;
                }
                let (rule, spec) = self.rule_between(&from.name, &to.name);
                let q = map_quantize(spec.quantize);
                let source = &self.m.regions[from_si];
                let spb = self.spb();
                let segments = phrase_segments(source.start_bar, source.length_bars, q.phrase_bars)
                    .into_iter()
                    .map(|(start_bar, length_bars)| Segment {
                        start_bar,
                        length_bars,
                        start_seconds: f64::from(start_bar) * spb,
                        length_seconds: f64::from(length_bars) * spb,
                    })
                    .collect();
                let stinger_stem = spec.stinger_event.as_ref().and_then(|ev| {
                    self.m
                        .events
                        .iter()
                        .find(|e| &e.name == ev)
                        .and_then(|e| e.stinger_stem.clone())
                });
                let transition = TransitionRegion {
                    rule,
                    from_mode: from.name.clone(),
                    to_mode: to.name.clone(),
                    source_region: source.name.clone(),
                    destination_marker: to.name.clone(),
                    condition: Condition {
                        parameter: MODE_PARAMETER.into(),
                        label: to.name.clone(),
                        value: to_i as f32,
                    },
                    quantization: q,
                    segments,
                    crossfade_beats: spec.crossfade_beats,
                    crossfade_ms: beats_to_ms(spec.crossfade_beats, self.p.tempo_bpm),
                    stinger_event: spec.stinger_event.clone(),
                    stinger_stem,
                    min_dwell_seconds: spec.min_dwell_seconds,
                };
                self.m.transitions.push(transition);
            }
        }
        let rules = self
            .p
            .transitions
            .iter()
            .map(|r| (rule_label(r.from.as_deref(), r.to.as_deref()), &r.spec));
        let all: Vec<(String, &TransitionSpec)> = rules
            .chain([("default".to_string(), &self.p.default_transition)])
            .collect();
        for (label, spec) in all {
            if let Some(ev) = &spec.stinger_event
                && self.p.event_index(ev).is_none()
            {
                self.m.warnings.push(format!(
                    "Transition {label}: stinger event '{ev}' doesn't exist."
                ));
            }
            if spec.min_dwell_seconds > 0.0 {
                self.m.game_side.push(format!(
                    "Hysteresis for {label}: after the switch keep Mode unchanged for at least {} s (game-side; FMOD \
                     has no minimum dwell).",
                    spec.min_dwell_seconds
                ));
            }
        }
        if let Some(first) = self.m.regions.first() {
            self.m.game_side.push(format!(
                "Set Mode before start(). The event starts in region '{}'; to start straight in another mode, also \
                 call setTimelinePosition(region start_seconds × 1000).",
                first.name
            ));
        }
    }

    /// The same lookup as `Project::transition_between`, plus which rule won.
    fn rule_between(&self, from: &str, to: &str) -> (String, &'a TransitionSpec) {
        let rules = &self.p.transitions;
        let exact = rules
            .iter()
            .find(|r| r.from.as_deref() == Some(from) && r.to.as_deref() == Some(to));
        let into = || {
            rules
                .iter()
                .find(|r| r.from.is_none() && r.to.as_deref() == Some(to))
        };
        let out_of = || {
            rules
                .iter()
                .find(|r| r.from.as_deref() == Some(from) && r.to.is_none())
        };
        match exact.or_else(into).or_else(out_of) {
            Some(r) => (rule_label(r.from.as_deref(), r.to.as_deref()), &r.spec),
            None => ("default".into(), &self.p.default_transition),
        }
    }

    fn project_warnings(&mut self) {
        if !self.p.scenarios.is_empty() {
            let names: Vec<&str> = self.p.scenarios.iter().map(|s| s.name.as_str()).collect();
            self.warn(format!(
                "Scenarios ({}) are a Simulator tool and don't export (prototype only).",
                names.join(", ")
            ));
        }
        let master = &self.p.master;
        self.warn(format!(
            "Master bus (volume {:+.1} dB, limiter {}, reverb size {:.2}) isn't in the stems: set volume and limiter \
             on FMOD's music bus; the shared reverb is baked per stem.",
            master.volume_db,
            if master.limiter { "on" } else { "off" },
            master.reverb_size
        ));
        if !self.p.puzzles.is_empty() {
            self.m.game_side.push(
                "Puzzles: the C# module reads puzzles.json; Paranoia's beat shift moves the input windows in game \
                 code. Check it against puzzle_vectors.json in C# tests."
                    .into(),
            );
        }
    }

    fn finish(self) -> Plan {
        Plan {
            manifest: self.m,
            jobs: self.jobs,
        }
    }
}

fn rule_label(from: Option<&str>, to: Option<&str>) -> String {
    format!("{} -> {}", from.unwrap_or("*"), to.unwrap_or("*"))
}

/// Checks a manifest for internal consistency. Returns one line per problem; empty = valid.
pub fn validate_manifest(m: &Manifest) -> Vec<String> {
    let mut problems = Vec::new();
    if m.manifest_version != MANIFEST_VERSION {
        problems.push(format!(
            "manifest_version is {}, expected {MANIFEST_VERSION}",
            m.manifest_version
        ));
    }
    if !(m.tempo_bpm.is_finite() && m.tempo_bpm > 0.0) {
        problems.push(format!("tempo_bpm {} is not positive", m.tempo_bpm));
    }
    if m.beats_per_bar == 0 {
        problems.push("beats_per_bar is 0".into());
    }
    if m.sample_rate == 0 {
        problems.push("sample_rate is 0".into());
    }
    if m.scene.is_empty() || sanitize_name(&m.scene) != m.scene {
        problems.push(format!("scene name '{}' is not sanitized", m.scene));
    }

    duplicates(
        "parameter",
        m.parameters.iter().map(|p| p.name.as_str()),
        false,
        &mut problems,
    );
    duplicates(
        "track",
        m.tracks.iter().map(|t| t.name.as_str()),
        false,
        &mut problems,
    );
    duplicates(
        "track key",
        m.tracks.iter().map(|t| t.key.as_str()),
        true,
        &mut problems,
    );
    duplicates(
        "region",
        m.regions.iter().map(|r| r.name.as_str()),
        false,
        &mut problems,
    );
    duplicates(
        "region key",
        m.regions.iter().map(|r| r.key.as_str()),
        true,
        &mut problems,
    );
    duplicates(
        "event",
        m.events.iter().map(|e| e.name.as_str()),
        false,
        &mut problems,
    );
    duplicates(
        "stem file",
        m.stems.iter().map(|s| s.file.as_str()),
        true,
        &mut problems,
    );
    duplicates(
        "marker",
        m.destination_markers.iter().map(|d| d.name.as_str()),
        false,
        &mut problems,
    );

    let param = |name: &str| m.parameters.iter().find(|p| p.name == name);
    let region = |name: &str| m.regions.iter().find(|r| r.name == name);
    let stem = |file: &str| m.stems.iter().find(|s| s.file == file);

    for p in &m.parameters {
        if !(p.min < p.max || (p.kind == ParameterKind::Labeled && p.labels.len() == 1)) {
            problems.push(format!(
                "parameter {}: min {} is not below max {}",
                p.name, p.min, p.max
            ));
        }
        if !(p.min..=p.max).contains(&p.default) {
            problems.push(format!(
                "parameter {}: default {} outside {}..{}",
                p.name, p.default, p.min, p.max
            ));
        }
        if !(p.seek_speed.is_finite() && p.seek_speed >= 0.0) {
            problems.push(format!(
                "parameter {}: bad seek speed {}",
                p.name, p.seek_speed
            ));
        }
        if p.kind == ParameterKind::Labeled
            && (p.labels.is_empty() || p.max != (p.labels.len() - 1) as f32)
        {
            problems.push(format!(
                "parameter {}: labels don't match its range",
                p.name
            ));
        }
    }

    let mut next_bar = 0;
    for r in &m.regions {
        if r.start_bar != next_bar {
            problems.push(format!(
                "region {} starts at bar {}, expected {next_bar}",
                r.name, r.start_bar
            ));
        }
        if r.length_bars == 0 {
            problems.push(format!("region {} is empty", r.name));
        }
        next_bar = r.start_bar + r.length_bars;
    }

    for s in &m.stems {
        let name = s
            .file
            .strip_prefix(&format!("{STEMS_DIR}/"))
            .and_then(|f| f.strip_suffix(".wav"));
        if !name.is_some_and(|n| {
            !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        }) {
            problems.push(format!("stem file name '{}' is not FMOD-friendly", s.file));
        }
        if s.length_frames == 0 {
            problems.push(format!("stem {} is empty", s.file));
        }
        if let (StemKind::Loop, Some(bars)) = (s.kind, s.length_bars) {
            let expected = bars_to_frames(bars, m.tempo_bpm, m.beats_per_bar, m.sample_rate) as u64;
            if s.length_frames != expected {
                problems.push(format!(
                    "stem {}: {} frames, expected {expected}",
                    s.file, s.length_frames
                ));
            }
        }
        let used = m
            .tracks
            .iter()
            .flat_map(|t| &t.instruments)
            .any(|i| i.stem == s.file)
            || m.events
                .iter()
                .any(|e| e.stinger_stem.as_deref() == Some(s.file.as_str()));
        if !used {
            problems.push(format!("stem {} is not used by any track or event", s.file));
        }
    }

    for t in &m.tracks {
        for i in &t.instruments {
            match stem(&i.stem) {
                None => problems.push(format!(
                    "track {}: stem {} is not in the stems list",
                    t.name, i.stem
                )),
                Some(s)
                    if s.kind != StemKind::Loop
                        || s.region.as_deref() != Some(i.region.as_str())
                        || s.track != t.name =>
                {
                    problems.push(format!(
                        "track {}: stem {} belongs to another track or region",
                        t.name, i.stem
                    ));
                }
                Some(_) => {}
            }
            match region(&i.region) {
                None => problems.push(format!("track {}: unknown region {}", t.name, i.region)),
                Some(r) if r.start_bar != i.start_bar || r.length_bars != i.length_bars => {
                    problems.push(format!(
                        "track {}: instrument in {} doesn't fill its region",
                        t.name, i.region
                    ));
                }
                Some(_) => {}
            }
        }
        for a in &t.automation {
            match param(&a.parameter) {
                None => problems.push(format!(
                    "track {}: automation on unknown parameter {}",
                    t.name, a.parameter
                )),
                Some(p) => {
                    if a.points.is_empty() {
                        problems.push(format!(
                            "track {}: automation {} has no points",
                            t.name, a.source
                        ));
                    }
                    if a.points.windows(2).any(|w| w[0][0] > w[1][0]) {
                        problems.push(format!(
                            "track {}: automation {} points are not sorted",
                            t.name, a.source
                        ));
                    }
                    if a.points
                        .iter()
                        .any(|pt| !pt[1].is_finite() || pt[0] < p.min || pt[0] > p.max)
                    {
                        problems.push(format!(
                            "track {}: automation {} has points out of range",
                            t.name, a.source
                        ));
                    }
                }
            }
        }
        if let Some(u) = &t.unlocked_by
            && param(u).is_none()
        {
            problems.push(format!("track {}: unlock parameter {u} is missing", t.name));
        }
    }

    for d in &m.destination_markers {
        match region(&d.region) {
            Some(r) if r.start_bar == d.bar => {}
            _ => problems.push(format!(
                "marker {} is not at the start of region {}",
                d.name, d.region
            )),
        }
    }

    let mode = param(MODE_PARAMETER);
    if !m.transitions.is_empty() && mode.is_none_or(|p| p.kind != ParameterKind::Labeled) {
        problems.push(format!(
            "transitions need a labeled {MODE_PARAMETER} parameter"
        ));
    }
    for tr in &m.transitions {
        let label_index = mode.and_then(|p| p.labels.iter().position(|l| l == &tr.condition.label));
        if tr.condition.parameter != MODE_PARAMETER
            || label_index.map(|i| i as f32) != Some(tr.condition.value)
        {
            problems.push(format!(
                "transition {} -> {}: bad condition",
                tr.from_mode, tr.to_mode
            ));
        }
        if !m
            .destination_markers
            .iter()
            .any(|d| d.name == tr.destination_marker)
        {
            problems.push(format!(
                "transition {} -> {}: unknown marker {}",
                tr.from_mode, tr.to_mode, tr.destination_marker
            ));
        }
        match region(&tr.source_region) {
            None => problems.push(format!(
                "transition {} -> {}: unknown region {}",
                tr.from_mode, tr.to_mode, tr.source_region
            )),
            Some(r) => {
                let end = r.start_bar + r.length_bars;
                if tr.segments.is_empty()
                    || tr
                        .segments
                        .iter()
                        .any(|s| s.start_bar < r.start_bar || s.start_bar + s.length_bars > end)
                {
                    problems.push(format!(
                        "transition {} -> {}: segments outside region {}",
                        tr.from_mode, tr.to_mode, r.name
                    ));
                }
            }
        }
        if let Some(file) = &tr.stinger_stem
            && stem(file).is_none_or(|s| s.kind != StemKind::Stinger)
        {
            problems.push(format!(
                "transition {} -> {}: stinger {file} is not in the stems list",
                tr.from_mode, tr.to_mode
            ));
        }
    }

    for e in &m.events {
        if let Some(file) = &e.stinger_stem
            && stem(file).is_none_or(|s| s.kind != StemKind::Stinger)
        {
            problems.push(format!(
                "event {}: stinger {file} is not in the stems list",
                e.name
            ));
        }
        if e.stinger_stem.is_some() != e.event_path.is_some() {
            problems.push(format!(
                "event {}: stinger file and event path must come together",
                e.name
            ));
        }
        if let Some(u) = &e.unlock_parameter
            && param(u).is_none()
        {
            problems.push(format!("event {}: unlock parameter {u} is missing", e.name));
        }
    }
    problems
}

fn duplicates<'a>(
    what: &str,
    names: impl Iterator<Item = &'a str>,
    ignore_case: bool,
    problems: &mut Vec<String>,
) {
    let mut seen: Vec<String> = Vec::new();
    for name in names {
        let k = if ignore_case {
            name.to_ascii_lowercase()
        } else {
            name.to_string()
        };
        if seen.contains(&k) {
            problems.push(format!("duplicate {what} name '{name}'"));
        } else {
            seen.push(k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aa_core::demo::cedar_station;

    fn demo() -> Manifest {
        build_manifest(&cedar_station(), &ExportOptions::default())
    }

    #[test]
    fn the_demo_manifest_is_valid() {
        let m = demo();
        assert_eq!(validate_manifest(&m), Vec::<String>::new());
    }

    #[test]
    fn sections_are_laid_out_back_to_back() {
        let m = demo();
        assert_eq!(m.regions.len(), 8);
        assert_eq!(m.regions[0].start_bar, 0);
        assert_eq!(m.regions[1].start_bar, 4);
        assert_eq!(m.regions[7].start_bar, 28);
        assert!((m.regions[1].start_seconds - 8.0).abs() < 1e-9);
        assert!(m.regions.iter().all(|r| r.looping));
    }

    #[test]
    fn parameters_include_mode_labels_and_unlocks() {
        let m = demo();
        let intensity = m.parameters.iter().find(|p| p.name == "Intensity");
        assert!(intensity.is_some_and(
            |p| (p.seek_speed - 0.5).abs() < 1e-6 && p.kind == ParameterKind::Continuous
        ));
        let mode = m.parameters.iter().find(|p| p.name == MODE_PARAMETER);
        assert!(
            mode.is_some_and(|p| p.labels.len() == 8 && p.labels[0] == "Explore" && p.max == 7.0)
        );
        let unlock = m
            .parameters
            .iter()
            .find(|p| p.name == "Unlocked_PuzzleSolved");
        assert!(unlock.is_some_and(|p| p.kind == ParameterKind::Discrete && p.default == 0.0));
        let bells = m.tracks.iter().find(|t| t.name == "Bells");
        assert_eq!(
            bells.and_then(|t| t.unlocked_by.as_deref()),
            Some("Unlocked_PuzzleSolved")
        );
    }

    #[test]
    fn influence_is_baked_into_rain_volume() {
        let m = demo();
        let rain = m.tracks.iter().find(|t| t.name == "Rain");
        let auto = rain.and_then(|t| t.automation.first());
        // curve 1.0 → 0.3 at x = 0.8, influence 0.6: 1 + 0.6·(0.3 − 1) = 0.58 → −4.73 dB.
        let pt = auto.and_then(|a| a.points.iter().find(|p| (p[0] - 0.8).abs() < 1e-6).copied());
        assert!(
            pt.is_some_and(|p| (p[1] - 20.0 * 0.58f32.log10()).abs() < 1e-3),
            "{pt:?}"
        );
        assert_eq!(auto.map(|a| a.target), Some(FmodTarget::TrackVolume));
    }

    #[test]
    fn transitions_cover_every_mode_pair_with_their_rules() {
        let m = demo();
        assert_eq!(m.transitions.len(), 8 * 7);
        let find = |from: &str, to: &str| {
            m.transitions
                .iter()
                .find(|t| t.from_mode == from && t.to_mode == to)
        };
        let alert = find("Stealth", "Alert");
        assert!(alert.is_some_and(|t| t.quantization.interval
            == crate::convert::QuantizeInterval::Beat
            && t.stinger_event.as_deref() == Some("Detected")
            && t.stinger_stem.as_deref() == Some("stems/stinger__Detected.wav")
            && t.crossfade_ms == 0.0));
        let relief = find("Chase", "Relief");
        assert!(
            relief.is_some_and(|t| t.quantization.phrase_bars == Some(2) && t.segments.len() == 2)
        );
        assert!(find("Explore", "PlayDead").is_some_and(|t| t.rule == "* -> PlayDead"));
        assert!(
            find("Explore", "Chase")
                .is_some_and(|t| t.rule == "default" && (t.crossfade_ms - 500.0).abs() < 1e-3)
        );
        assert_eq!(m.destination_markers.len(), 8);
        assert!(m.game_side.iter().any(|g| g.contains("Hysteresis")));
    }

    #[test]
    fn density_becomes_a_prototype_only_variant_list() {
        let m = demo();
        assert_eq!(m.density_variants.len(), 1);
        assert_eq!(m.density_variants[0].track, "Hats");
        assert_eq!(m.density_variants[0].stems.len(), 5);
        let hats = m.tracks.iter().find(|t| t.name == "Hats");
        assert!(hats.is_some_and(|t| t.automation.iter().any(|a| a.prototype_only)));
    }

    #[test]
    fn validation_catches_broken_references() {
        let mut m = demo();
        m.tracks[0].instruments[0].stem = "stems/nope.wav".into();
        m.tracks[1].name = m.tracks[2].name.clone();
        m.transitions[0].destination_marker = "Nowhere".into();
        let problems = validate_manifest(&m);
        assert!(
            problems.iter().any(|p| p.contains("not in the stems list")),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.contains("duplicate track")),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.contains("unknown marker")),
            "{problems:?}"
        );
    }
}
