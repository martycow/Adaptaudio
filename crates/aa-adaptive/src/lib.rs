//! The adaptive layer: how the music listens to the game.
//!
//! Deterministic and allocation-free after construction, so the audio thread can own it and
//! tests can drive it step by step. Time is measured in sequencer steps (sixteenth notes) from
//! the transport start; `pos` is fractional.
//!
//! - **Parameters** glide to their targets at their slew rate (FMOD's seek speed).
//! - **Game modes** switch on the next quantize point of their transition rule, never before the
//!   current mode's minimum dwell time (hysteresis).
//! - **Events** fire on their own quantize point and may unlock layers.
//! - **Track mods** fold every response curve of a track, scaled by its Influence.

use aa_core::{CurveTarget, Project, Quantize, SectionId, Track, TransitionSpec};

/// Most parameters a project may define. Arrays of this size live in the audio thread.
pub const MAX_PARAMS: usize = 16;
/// Most events queued at once.
pub const MAX_PENDING_EVENTS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Ramp {
    from: f32,
    to: f32,
    elapsed: f32,
    duration: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ParamState {
    value: f32,
    target: f32,
    /// Units per second the value moves towards `target`.
    rate: f32,
    ramp: Option<Ramp>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PendingMode {
    pub to: usize,
    pub at_step: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PendingEvent {
    event: usize,
    at_step: f64,
}

/// What `advance` reports back to the engine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AdaptiveEvent {
    /// The mode changed at `at_step`: start `section`, crossfading over `crossfade_beats`.
    ModeSwitched {
        from: Option<usize>,
        to: usize,
        section: Option<SectionId>,
        crossfade_beats: f32,
        at_step: f64,
    },
    /// An event fired at `at_step` (its stinger should start there).
    EventFired { event: usize, at_step: f64 },
}

/// Why a mode request didn't schedule a switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeRequest {
    Scheduled,
    AlreadyThere,
    UnknownMode,
}

#[derive(Clone, Debug)]
pub struct AdaptiveState {
    params: [ParamState; MAX_PARAMS],
    param_count: usize,
    mode: Option<usize>,
    pending_mode: Option<PendingMode>,
    pending_stinger: Option<usize>,
    pending_events: [Option<PendingEvent>; MAX_PENDING_EVENTS],
    /// Seconds spent in the current mode.
    time_in_mode: f64,
    /// Minimum dwell of the transition that brought us here.
    dwell: f32,
    /// Bit i set = event i has fired since reset (unlocks layers).
    fired: u64,
}

/// The sound changes the game asks of one track right now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackMods {
    pub volume: f32,
    pub cutoff: f32,
    pub drive: f32,
    pub detune_cents: f32,
    pub density: f32,
    pub reverb: f32,
    pub wobble: f32,
}

impl TrackMods {
    pub const NEUTRAL: TrackMods = TrackMods {
        volume: 1.0,
        cutoff: 1.0,
        drive: 0.0,
        detune_cents: 0.0,
        density: 1.0,
        reverb: 0.0,
        wobble: 0.0,
    };
}

impl Default for TrackMods {
    fn default() -> Self {
        TrackMods::NEUTRAL
    }
}

/// The next point on the quantize grid at or after `pos` (in steps).
pub fn next_grid_point(pos: f64, quantize: Quantize, steps_per_bar: u32) -> f64 {
    match quantize.grid_steps(steps_per_bar) {
        None => pos,
        Some(grid) => {
            let grid = f64::from(grid.max(1));
            let k = (pos / grid - 1e-9).ceil();
            k.max(0.0) * grid
        }
    }
}

impl AdaptiveState {
    pub fn new(project: &Project) -> Self {
        let mut s = AdaptiveState {
            params: [ParamState {
                value: 0.0,
                target: 0.0,
                rate: 1.0,
                ramp: None,
            }; MAX_PARAMS],
            param_count: 0,
            mode: None,
            pending_mode: None,
            pending_stinger: None,
            pending_events: [None; MAX_PENDING_EVENTS],
            time_in_mode: 0.0,
            dwell: 0.0,
            fired: 0,
        };
        s.sync_params(project);
        s
    }

    /// Keeps parameter values across a project edit, adding new ones at their defaults.
    pub fn sync_params(&mut self, project: &Project) {
        let count = project.game_params.len().min(MAX_PARAMS);
        for (i, def) in project.game_params.iter().take(MAX_PARAMS).enumerate() {
            let rate = 1.0 / def.slew_seconds.max(1e-3);
            if i >= self.param_count {
                self.params[i] = ParamState {
                    value: def.default,
                    target: def.default,
                    rate,
                    ramp: None,
                };
            } else {
                self.params[i].rate = rate;
            }
        }
        self.param_count = count;
    }

    /// Back to the start: defaults, no mode, nothing unlocked.
    pub fn reset(&mut self, project: &Project) {
        *self = AdaptiveState::new(project);
    }

    pub fn param_count(&self) -> usize {
        self.param_count
    }

    pub fn param(&self, index: usize) -> f32 {
        self.params
            .get(index)
            .filter(|_| index < self.param_count)
            .map_or(0.0, |p| p.value)
    }

    pub fn param_target(&self, index: usize) -> f32 {
        self.params
            .get(index)
            .filter(|_| index < self.param_count)
            .map_or(0.0, |p| p.target)
    }

    pub fn param_by_name(&self, project: &Project, name: &str) -> f32 {
        project.param_index(name).map_or(0.0, |i| self.param(i))
    }

    /// The game sets a parameter; it glides there at its slew rate.
    pub fn set_param(&mut self, index: usize, value: f32) {
        if let Some(p) = self
            .params
            .get_mut(index)
            .filter(|_| index < self.param_count)
        {
            p.target = value.clamp(0.0, 1.0);
            p.ramp = None;
        }
    }

    /// Jumps a parameter with no glide.
    pub fn jump_param(&mut self, index: usize, value: f32) {
        if let Some(p) = self
            .params
            .get_mut(index)
            .filter(|_| index < self.param_count)
        {
            let v = value.clamp(0.0, 1.0);
            *p = ParamState {
                value: v,
                target: v,
                rate: p.rate,
                ramp: None,
            };
        }
    }

    /// Moves a parameter to `value` linearly over exactly `seconds` (scenario ramps).
    pub fn ramp_param(&mut self, index: usize, value: f32, seconds: f32) {
        if seconds <= 0.0 {
            return self.jump_param(index, value);
        }
        if let Some(p) = self
            .params
            .get_mut(index)
            .filter(|_| index < self.param_count)
        {
            let to = value.clamp(0.0, 1.0);
            p.target = to;
            p.ramp = Some(Ramp {
                from: p.value,
                to,
                elapsed: 0.0,
                duration: seconds,
            });
        }
    }

    pub fn mode(&self) -> Option<usize> {
        self.mode
    }

    pub fn pending_mode(&self) -> Option<PendingMode> {
        self.pending_mode
    }

    /// The earliest step at which something is scheduled (a mode switch or an event).
    pub fn next_due(&self) -> Option<f64> {
        let mode = self.pending_mode.map(|m| m.at_step);
        let events = self.pending_events.iter().flatten().map(|e| e.at_step);
        mode.into_iter().chain(events).reduce(f64::min)
    }

    /// Bitmask of events fired since reset (bit i = event i).
    pub fn fired_mask(&self) -> u64 {
        self.fired
    }

    pub fn has_fired(&self, event: usize) -> bool {
        event < 64 && self.fired & (1 << event) != 0
    }

    /// True when a track's `locked_until_event` has been satisfied (or it has no lock).
    pub fn is_unlocked(&self, project: &Project, track: &Track) -> bool {
        match &track.adaptive.locked_until_event {
            None => true,
            Some(name) => project.event_index(name).is_some_and(|e| self.has_fired(e)),
        }
    }

    /// The game asks for a mode. It lands on the transition's quantize point, after the dwell.
    pub fn request_mode(&mut self, project: &Project, to: usize, pos: f64) -> ModeRequest {
        let Some(target) = project.game_modes.get(to) else {
            return ModeRequest::UnknownMode;
        };
        if self.mode == Some(to) {
            self.pending_mode = None;
            self.pending_stinger = None;
            return ModeRequest::AlreadyThere;
        }
        // The very first mode starts at once; later ones follow their transition rule.
        let spec: Option<&TransitionSpec> = self
            .mode
            .and_then(|m| project.game_modes.get(m))
            .map(|from| project.transition_between(&from.name, &target.name));
        let quantize = spec.map_or(Quantize::Immediate, |s| s.quantize);
        // Hysteresis: wait out the dwell of the transition that brought us into this mode.
        let wait_steps =
            (f64::from(self.dwell) - self.time_in_mode).max(0.0) / project.seconds_per_step();
        let earliest = pos + wait_steps;
        let at_step = next_grid_point(earliest, quantize, project.steps_per_bar());
        self.pending_mode = Some(PendingMode { to, at_step });
        self.pending_stinger = spec
            .and_then(|s| s.stinger_event.as_deref())
            .and_then(|e| project.event_index(e));
        ModeRequest::Scheduled
    }

    /// The game fires an event; it lands on the event's own quantize point.
    pub fn trigger_event(&mut self, project: &Project, event: usize, pos: f64) -> Option<f64> {
        let def = project.events.get(event)?;
        let at_step = next_grid_point(pos, def.quantize, project.steps_per_bar());
        let slot = self.pending_events.iter_mut().find(|e| e.is_none())?;
        *slot = Some(PendingEvent { event, at_step });
        Some(at_step)
    }

    /// Moves time forward by `dt` seconds to position `pos`, pushing what happened into `out`.
    /// `out` should have capacity for a few events; nothing is pushed past its capacity, so the
    /// audio thread never allocates.
    pub fn advance(&mut self, project: &Project, dt: f32, pos: f64, out: &mut Vec<AdaptiveEvent>) {
        for p in self.params.iter_mut().take(self.param_count) {
            if let Some(r) = &mut p.ramp {
                r.elapsed += dt;
                let t = (r.elapsed / r.duration).clamp(0.0, 1.0);
                p.value = r.from + (r.to - r.from) * t;
                if t >= 1.0 {
                    p.ramp = None;
                }
            } else {
                let step = p.rate * dt;
                let delta = p.target - p.value;
                p.value = if delta.abs() <= step {
                    p.target
                } else {
                    p.value + step * delta.signum()
                };
            }
        }
        self.time_in_mode += f64::from(dt);

        let push = |out: &mut Vec<AdaptiveEvent>, e: AdaptiveEvent| {
            if out.len() < out.capacity() {
                out.push(e);
            }
        };

        if let Some(pending) = self.pending_mode.filter(|p| p.at_step <= pos) {
            let from = self.mode;
            let spec = from
                .and_then(|m| project.game_modes.get(m))
                .zip(project.game_modes.get(pending.to))
                .map(|(f, t)| project.transition_between(&f.name, &t.name));
            let crossfade_beats = spec.map_or(0.0, |s| s.crossfade_beats);
            self.dwell = spec.map_or(0.0, |s| s.min_dwell_seconds);
            self.mode = Some(pending.to);
            self.pending_mode = None;
            self.time_in_mode = 0.0;
            let section = project.game_modes.get(pending.to).and_then(|m| m.section);
            if let Some(event) = self.pending_stinger.take() {
                self.fire(event);
                push(
                    out,
                    AdaptiveEvent::EventFired {
                        event,
                        at_step: pending.at_step,
                    },
                );
            }
            push(
                out,
                AdaptiveEvent::ModeSwitched {
                    from,
                    to: pending.to,
                    section,
                    crossfade_beats,
                    at_step: pending.at_step,
                },
            );
        }

        for slot in &mut self.pending_events {
            if let Some(e) = *slot
                && e.at_step <= pos
            {
                *slot = None;
                if e.event < 64 {
                    self.fired |= 1 << e.event;
                }
                push(
                    out,
                    AdaptiveEvent::EventFired {
                        event: e.event,
                        at_step: e.at_step,
                    },
                );
            }
        }
    }

    fn fire(&mut self, event: usize) {
        if event < 64 {
            self.fired |= 1 << event;
        }
    }

    /// Folds a track's response curves at the current parameter values, scaled by Influence.
    pub fn track_mods(&self, project: &Project, track: &Track) -> TrackMods {
        track_mods_at(
            project,
            track,
            |name| self.param_by_name(project, name),
            self.is_unlocked(project, track),
        )
    }
}

/// [`AdaptiveState::track_mods`] for any parameter source (the Simulator's preview, export).
pub fn track_mods_at(
    project: &Project,
    track: &Track,
    param: impl Fn(&str) -> f32,
    unlocked: bool,
) -> TrackMods {
    let _ = project;
    let influence = track.adaptive.influence.clamp(0.0, 1.0);
    let mut m = TrackMods::NEUTRAL;
    for curve in &track.adaptive.curves {
        let raw = curve.eval(param(&curve.param));
        let neutral = curve.target.neutral();
        let v = neutral + influence * (raw - neutral);
        match curve.target {
            CurveTarget::Volume => m.volume *= v.max(0.0),
            CurveTarget::FilterCutoff => m.cutoff *= v.clamp(0.0, 1.0),
            CurveTarget::Density => m.density *= v.clamp(0.0, 1.0),
            CurveTarget::Drive => m.drive += v.max(0.0),
            CurveTarget::Detune => m.detune_cents += v.max(0.0),
            CurveTarget::ReverbSend => m.reverb += v.max(0.0),
            CurveTarget::Wobble => m.wobble += v.max(0.0),
        }
    }
    if !unlocked {
        m.volume = 0.0;
    }
    m.drive = m.drive.min(1.0);
    m.reverb = m.reverb.min(1.0);
    m.wobble = m.wobble.min(1.0);
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use aa_core::demo::cedar_station;

    fn steps(project: &Project, seconds: f64) -> f64 {
        seconds / project.seconds_per_step()
    }

    #[test]
    fn grid_points_round_up_and_keep_exact_hits() {
        assert_eq!(next_grid_point(0.0, Quantize::Bar, 16), 0.0);
        assert_eq!(next_grid_point(0.5, Quantize::Bar, 16), 16.0);
        assert_eq!(next_grid_point(16.0, Quantize::Bar, 16), 16.0);
        assert_eq!(next_grid_point(17.0, Quantize::Beat, 16), 20.0);
        assert_eq!(next_grid_point(33.0, Quantize::Phrase(2), 16), 64.0);
        assert_eq!(next_grid_point(7.3, Quantize::Immediate, 16), 7.3);
    }

    #[test]
    fn parameters_glide_at_their_slew_rate() {
        let p = cedar_station();
        let mut a = AdaptiveState::new(&p);
        let i = p.param_index("Intensity").unwrap();
        a.jump_param(i, 0.0);
        a.set_param(i, 1.0);
        let mut out = Vec::with_capacity(4);
        // Intensity slews over 2 s for the whole range.
        for _ in 0..100 {
            a.advance(&p, 0.01, 0.0, &mut out);
        }
        assert!((a.param(i) - 0.5).abs() < 1e-3, "{}", a.param(i));
        for _ in 0..200 {
            a.advance(&p, 0.01, 0.0, &mut out);
        }
        assert_eq!(a.param(i), 1.0);
    }

    #[test]
    fn ramps_take_exactly_their_time() {
        let p = cedar_station();
        let mut a = AdaptiveState::new(&p);
        let i = p.param_index("Tension").unwrap();
        a.jump_param(i, 0.0);
        a.ramp_param(i, 0.8, 4.0);
        let mut out = Vec::with_capacity(4);
        for _ in 0..200 {
            a.advance(&p, 0.01, 0.0, &mut out);
        }
        assert!((a.param(i) - 0.4).abs() < 1e-3);
        for _ in 0..300 {
            a.advance(&p, 0.01, 0.0, &mut out);
        }
        assert!((a.param(i) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn the_first_mode_starts_at_once_and_later_ones_wait_for_their_grid() {
        let p = cedar_station();
        let mut a = AdaptiveState::new(&p);
        let mut out = Vec::with_capacity(4);
        let stealth = p.mode_index("Stealth").unwrap();
        let alert = p.mode_index("Alert").unwrap();
        let chase = p.mode_index("Chase").unwrap();

        assert_eq!(a.request_mode(&p, stealth, 3.0), ModeRequest::Scheduled);
        a.advance(&p, 0.0, 3.0, &mut out);
        assert_eq!(a.mode(), Some(stealth));
        out.clear();

        // Let the dwell pass, then Stealth → Alert quantizes to the beat and fires Detected.
        a.advance(&p, 2.0, 20.0, &mut out);
        a.request_mode(&p, alert, 21.0);
        assert_eq!(a.pending_mode().map(|m| m.at_step), Some(24.0));
        out.clear();
        a.advance(&p, 0.01, 23.9, &mut out);
        assert_eq!(a.mode(), Some(stealth));
        a.advance(&p, 0.01, 24.0, &mut out);
        assert_eq!(a.mode(), Some(alert));
        let detected = p.event_index("Detected").unwrap();
        assert!(out.contains(&AdaptiveEvent::EventFired {
            event: detected,
            at_step: 24.0
        }));
        assert!(
            out.iter()
                .any(|e| matches!(e, AdaptiveEvent::ModeSwitched { to, .. } if *to == alert))
        );

        // Alert → Chase waits for the bar, but first the 1 s dwell after entering Alert.
        out.clear();
        a.request_mode(&p, chase, 25.0);
        let dwell_steps = steps(&p, 1.0);
        let expected = next_grid_point(25.0 + dwell_steps, Quantize::Bar, 16);
        assert_eq!(a.pending_mode().map(|m| m.at_step), Some(expected));
    }

    #[test]
    fn asking_for_the_current_mode_cancels_a_pending_switch() {
        let p = cedar_station();
        let mut a = AdaptiveState::new(&p);
        let mut out = Vec::with_capacity(4);
        let explore = p.mode_index("Explore").unwrap();
        let stealth = p.mode_index("Stealth").unwrap();
        a.request_mode(&p, explore, 0.0);
        a.advance(&p, 5.0, 0.0, &mut out);
        a.request_mode(&p, stealth, 1.0);
        assert!(a.pending_mode().is_some());
        assert_eq!(a.request_mode(&p, explore, 2.0), ModeRequest::AlreadyThere);
        assert!(a.pending_mode().is_none());
    }

    #[test]
    fn events_unlock_layers() {
        let p = cedar_station();
        let mut a = AdaptiveState::new(&p);
        let bells = p.tracks.iter().find(|t| t.name == "Bells").unwrap();
        let progress = p.param_index("PuzzleProgress").unwrap();
        a.jump_param(progress, 1.0);
        assert_eq!(a.track_mods(&p, bells).volume, 0.0);
        let solved = p.event_index("PuzzleSolved").unwrap();
        let at = a.trigger_event(&p, solved, 5.0);
        assert_eq!(at, Some(16.0));
        let mut out = Vec::with_capacity(4);
        a.advance(&p, 0.1, 16.0, &mut out);
        assert!(out.contains(&AdaptiveEvent::EventFired {
            event: solved,
            at_step: 16.0
        }));
        assert!((a.track_mods(&p, bells).volume - 1.0).abs() < 1e-6);
    }

    #[test]
    fn influence_scales_curves_towards_neutral() {
        let p = cedar_station();
        let mut kick = p.tracks.iter().find(|t| t.name == "Kick").unwrap().clone();
        let quiet = |t: &Track| track_mods_at(&p, t, |_| 0.0, true).volume;
        assert_eq!(quiet(&kick), 0.0);
        kick.adaptive.influence = 0.5;
        assert!((quiet(&kick) - 0.5).abs() < 1e-6);
        kick.adaptive.influence = 0.0;
        assert_eq!(quiet(&kick), 1.0);
    }

    #[test]
    fn advance_never_grows_the_output_buffer() {
        let p = cedar_station();
        let mut a = AdaptiveState::new(&p);
        let mut out = Vec::with_capacity(1);
        let cap = out.capacity();
        a.request_mode(&p, 0, 0.0);
        for e in 0..p.events.len() {
            a.trigger_event(&p, e, 0.0);
        }
        a.advance(&p, 0.1, 100.0, &mut out);
        assert_eq!(out.capacity(), cap);
    }
}
