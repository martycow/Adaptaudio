//! Offline rendering: stems for the FMOD export, scenarios for tests.

use crate::engine::{Command, Engine};
use crate::samples::LoadedProject;
use aa_core::{Project, Scenario, ScenarioAction, SectionId, TrackId};
use std::sync::Arc;

/// A copy of `base` sharing its sample data but with a different model.
pub fn with_project(base: &LoadedProject, project: Project) -> LoadedProject {
    LoadedProject {
        project,
        ..base.share()
    }
}

impl LoadedProject {
    /// Same samples, same model (cheap: sample data is shared).
    pub fn share(&self) -> LoadedProject {
        self.clone_shallow()
    }
}

/// Renders one track of one section, `bars` long, with the game layer neutral.
///
/// The loop plays twice and the second pass is kept, so reverb and release tails from the end
/// already ring into the start: the stem loops without a seam.
pub fn render_stem(
    base: &LoadedProject,
    section: SectionId,
    track: TrackId,
    bars: u32,
    sample_rate: f32,
) -> Vec<f32> {
    let mut project = base.project.clone();
    for t in &mut project.tracks {
        t.mixer.mute = t.id != track;
        t.mixer.solo = false;
        t.adaptive.influence = 0.0;
        t.adaptive.locked_until_event = None;
    }
    project.master.limiter = false;
    let steps = f64::from(bars) * f64::from(project.steps_per_bar());
    let seconds = steps * project.seconds_per_step();
    let frames = (seconds * f64::from(sample_rate)).round() as usize;
    let loaded = Arc::new(with_project(base, project));
    let (mut engine, _controller) = Engine::new(loaded, sample_rate);
    engine.apply(Command::LaunchSection(section));
    engine.apply(Command::Play);
    // `Play` restarts on the active deck, which `LaunchSection` set while stopped.
    let mut out = vec![0.0; frames * 2];
    engine.process(&mut out, 2);
    engine.process(&mut out, 2);
    out
}

/// Renders `bars` bars of one track in one section as a continuous take (no pass is dropped).
///
/// `prefader`: leave out the track's fader and pan (FMOD applies those), keeping the instrument,
/// its drive and its reverb share. Used by the FMOD export, which asks for several passes and
/// keeps the last itself.
pub fn render_bars(
    base: &LoadedProject,
    section: SectionId,
    track: TrackId,
    bars: u32,
    sample_rate: f32,
    prefader: bool,
) -> Vec<f32> {
    let mut project = base.project.clone();
    for t in &mut project.tracks {
        t.mixer.mute = t.id != track;
        t.mixer.solo = false;
        t.adaptive.influence = 0.0;
        t.adaptive.locked_until_event = None;
        if prefader && t.id == track {
            t.mixer.volume_db = 0.0;
            t.mixer.pan = 0.0;
            t.mixer.sidechain = 0.0;
        }
    }
    project.master.limiter = false;
    project.master.volume_db = 0.0;
    let steps = f64::from(bars) * f64::from(project.steps_per_bar());
    let frames = (steps * project.seconds_per_step() * f64::from(sample_rate)).round() as usize;
    let loaded = Arc::new(with_project(base, project));
    let (mut engine, _controller) = Engine::new(loaded, sample_rate);
    engine.apply(Command::LaunchSection(section));
    engine.apply(Command::Play);
    let mut out = vec![0.0; frames * 2];
    engine.process(&mut out, 2);
    out
}

/// Renders a one-shot clip (a stinger) plus `tail_seconds`.
pub fn render_oneshot(
    base: &LoadedProject,
    track: TrackId,
    clip: aa_core::ClipId,
    tail_seconds: f32,
    sample_rate: f32,
) -> Vec<f32> {
    let mut project = base.project.clone();
    let Some(index) = project.track_index(track) else {
        return Vec::new();
    };
    let length = project.tracks[index]
        .clip(clip)
        .map_or(0, |c| c.length_steps);
    for t in &mut project.tracks {
        t.mixer.mute = t.id != track;
        t.adaptive.influence = 0.0;
    }
    project.master.limiter = false;
    // A throwaway section holding only this clip.
    let section = project.add_section("__oneshot", 1);
    project.set_slot(section, track, Some(clip));
    if let Some(s) = project.section_mut(section) {
        s.length_bars = 1;
    }
    let seconds = f64::from(length) * project.seconds_per_step() + f64::from(tail_seconds);
    let frames = (seconds * f64::from(sample_rate)).round() as usize;
    // Render the clip once, then let it ring with the section gone.
    let one_loop =
        (f64::from(length) * project.seconds_per_step() * f64::from(sample_rate)) as usize;
    let loaded = Arc::new(with_project(base, project));
    let (mut engine, _c) = Engine::new(loaded, sample_rate);
    engine.apply(Command::LaunchSection(section));
    engine.apply(Command::Play);
    let mut out = vec![0.0; frames * 2];
    let (head, tail) = out.split_at_mut((one_loop * 2).min(frames * 2));
    engine.process(head, 2);
    engine.apply(Command::Stop);
    engine.process(tail, 2);
    out
}

/// Plays a scenario offline: the same commands the Simulator sends, at their times.
pub fn render_scenario(
    base: &LoadedProject,
    scenario: &Scenario,
    seconds: f32,
    sample_rate: f32,
) -> (Vec<f32>, Engine) {
    let loaded = Arc::new(base.share());
    let project = &loaded.project;
    let mut steps: Vec<_> = scenario.steps.clone();
    steps.sort_by(|a, b| a.at_seconds.total_cmp(&b.at_seconds));
    let commands: Vec<(f32, Command)> = steps
        .iter()
        .filter_map(|s| {
            let c = match &s.action {
                ScenarioAction::SetMode(m) => Command::RequestMode(project.mode_index(m)?),
                ScenarioAction::SetParam {
                    name,
                    value,
                    over_seconds,
                } => Command::RampParam {
                    index: project.param_index(name)?,
                    value: *value,
                    seconds: *over_seconds,
                },
                ScenarioAction::Trigger(e) => Command::TriggerEvent(project.event_index(e)?),
            };
            Some((s.at_seconds, c))
        })
        .collect();
    let (mut engine, _c) = Engine::new(Arc::clone(&loaded), sample_rate);
    engine.apply(Command::Play);
    let total = (seconds * sample_rate) as usize;
    let block = 256;
    let mut out = vec![0.0; total * 2];
    let mut queue = commands.into_iter().peekable();
    let mut frame = 0;
    while frame < total {
        let now = frame as f32 / sample_rate;
        while let Some((_, c)) = queue.next_if(|(at, _)| *at <= now) {
            engine.apply(c);
        }
        let n = block.min(total - frame);
        engine.process(&mut out[frame * 2..(frame + n) * 2], 2);
        frame += n;
    }
    (out, engine)
}
