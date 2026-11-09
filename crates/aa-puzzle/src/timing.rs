//! Timing puzzles: press the right lane on each beat.

use aa_core::{Puzzle, PuzzleKind};

use crate::{
    DEFAULT_MAX_MISTAKES, Judgement, LOOSE_FACTOR, PressResult, grade, heard_offset, ms_per_beat,
    was_fooled,
};

/// One beat to press.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimingTarget {
    /// True time of the beat, in milliseconds since the puzzle started.
    pub time_ms: f64,
    pub lane: u8,
    /// `None` while the target is still open; then Perfect, Good, Late or Miss.
    pub judgement: Option<Judgement>,
}

/// A Timing puzzle being played.
///
/// Targets keep the order of the puzzle's `beats`; their indices are what [`PressResult`]
/// refers to.
#[derive(Clone, Debug, PartialEq)]
pub struct TimingSession {
    targets: Vec<TimingTarget>,
    window_ms: f64,
    paranoia_shift_ms: f32,
    mistakes: u32,
    fooled: u32,
    /// How many mistakes are allowed while still solving the puzzle (default 2).
    pub max_mistakes: u32,
}

impl TimingSession {
    /// Starts a Timing puzzle at a tempo. `None` when the puzzle is of another kind.
    ///
    /// A beat without a lane in `lanes` uses lane 0.
    pub fn new(puzzle: &Puzzle, bpm: f32) -> Option<Self> {
        match &puzzle.kind {
            PuzzleKind::Timing { beats, lanes } => Some(Self::build(puzzle, beats, lanes, bpm)),
            _ => None,
        }
    }

    pub(crate) fn build(puzzle: &Puzzle, beats: &[f32], lanes: &[u8], bpm: f32) -> Self {
        let beat_ms = ms_per_beat(bpm);
        let targets = beats
            .iter()
            .enumerate()
            .map(|(i, &beat)| TimingTarget {
                time_ms: f64::from(beat) * beat_ms,
                lane: lanes.get(i).copied().unwrap_or(0),
                judgement: None,
            })
            .collect();
        TimingSession {
            targets,
            window_ms: f64::from(puzzle.window_ms.max(0.0)),
            paranoia_shift_ms: puzzle.paranoia_shift_ms,
            mistakes: 0,
            fooled: 0,
            max_mistakes: DEFAULT_MAX_MISTAKES,
        }
    }

    pub fn targets(&self) -> &[TimingTarget] {
        &self.targets
    }

    /// Number of lanes the puzzle uses (highest lane + 1).
    pub fn lane_count(&self) -> u8 {
        self.targets
            .iter()
            .map(|t| t.lane.saturating_add(1))
            .max()
            .unwrap_or(1)
    }

    /// When the game should *play* target `index` at this Paranoia (see the crate docs).
    pub fn heard_time_ms(&self, index: usize, paranoia: f32) -> Option<f64> {
        let target = self.targets.get(index)?;
        Some(target.time_ms + f64::from(heard_offset(self.paranoia_shift_ms, paranoia, index)))
    }

    /// The open target closest to `time_ms` within `max_distance` that passes `filter`.
    /// Ties go to the lower index.
    fn nearest_open(
        &self,
        time_ms: f64,
        max_distance: f64,
        filter: impl Fn(&TimingTarget) -> bool,
    ) -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        for (i, t) in self.targets.iter().enumerate() {
            if t.judgement.is_some() || !filter(t) {
                continue;
            }
            let distance = (time_ms - t.time_ms).abs();
            if distance <= max_distance && best.is_none_or(|(_, d)| distance < d) {
                best = Some((i, distance));
            }
        }
        best.map(|(i, _)| i)
    }

    /// Judges a press on `lane` at `time_ms` (since the puzzle started).
    ///
    /// 1. The closest open target on this lane within the window is hit (Perfect or Good).
    /// 2. Otherwise the closest open target on any lane within twice the window: on this lane
    ///    the press is Early (the target stays open) or Late (the target is lost); on another
    ///    lane it is Wrong (the target stays open).
    /// 3. Otherwise the press is Stray.
    ///
    /// Everything but a hit is a mistake, except presses after every target is done, which
    /// are Stray but free. `paranoia` never moves the judging point; see the crate docs.
    pub fn press(&mut self, time_ms: f64, lane: u8, paranoia: f32) -> PressResult {
        if self.is_over() {
            return PressResult::stray();
        }
        let w = self.window_ms;
        let (index, judgement) = if let Some(i) = self.nearest_open(time_ms, w, |t| t.lane == lane)
        {
            let offset = time_ms - self.targets[i].time_ms;
            (Some(i), grade(offset, w).unwrap_or(Judgement::Good))
        } else if let Some(i) = self.nearest_open(time_ms, w * LOOSE_FACTOR, |_| true) {
            let t = self.targets[i];
            let judgement = if t.lane != lane {
                Judgement::Wrong
            } else if time_ms < t.time_ms {
                Judgement::Early
            } else {
                Judgement::Late
            };
            (Some(i), judgement)
        } else {
            (None, Judgement::Stray)
        };

        let Some(i) = index else {
            self.mistakes += 1;
            return PressResult::stray();
        };
        let target_ms = self.targets[i].time_ms;
        if judgement.is_hit() || judgement == Judgement::Late {
            self.targets[i].judgement = Some(judgement);
        }
        if judgement.is_mistake() {
            self.mistakes += 1;
        }
        if judgement != Judgement::Perfect
            && was_fooled(
                time_ms,
                target_ms,
                heard_offset(self.paranoia_shift_ms, paranoia, i),
                paranoia,
            )
        {
            self.fooled += 1;
        }
        PressResult {
            judgement,
            target: Some(i),
            offset_ms: (time_ms - target_ms) as f32,
        }
    }

    /// Advances the clock: every open target more than twice the window before `time_ms`
    /// becomes Miss. Call it every frame and once at the end. Returns the newly missed targets.
    pub fn finish(&mut self, time_ms: f64) -> Vec<usize> {
        let limit = self.window_ms * LOOSE_FACTOR;
        let mut missed = Vec::new();
        for (i, t) in self.targets.iter_mut().enumerate() {
            if t.judgement.is_none() && time_ms > t.time_ms + limit {
                t.judgement = Some(Judgement::Miss);
                missed.push(i);
            }
        }
        missed
    }

    /// Number of targets hit (Perfect or Good).
    pub fn hits(&self) -> usize {
        self.targets
            .iter()
            .filter(|t| t.judgement.is_some_and(Judgement::is_hit))
            .count()
    }

    /// Fraction of targets hit, 0..1 (1 for a puzzle without targets).
    pub fn progress(&self) -> f32 {
        if self.targets.is_empty() {
            1.0
        } else {
            self.hits() as f32 / self.targets.len() as f32
        }
    }

    pub fn mistakes(&self) -> u32 {
        self.mistakes
    }

    /// Presses that followed the false, Paranoia-shifted beat rather than the true one.
    pub fn fooled(&self) -> u32 {
        self.fooled
    }

    /// Every target hit, with no more than `max_mistakes` mistakes.
    pub fn is_solved(&self) -> bool {
        self.hits() == self.targets.len() && self.mistakes <= self.max_mistakes
    }

    /// True when no target is open any more.
    pub fn is_over(&self) -> bool {
        self.targets.iter().all(|t| t.judgement.is_some())
    }

    /// Starts again from the top (keeps `max_mistakes`).
    pub fn reset(&mut self) {
        for t in &mut self.targets {
            t.judgement = None;
        }
        self.mistakes = 0;
        self.fooled = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heard_offset_ms;
    use aa_core::demo::cedar_station;

    /// "Cedar door knocks" at 120 BPM: a beat is 500 ms, the window ±80 ms.
    fn session() -> (Puzzle, TimingSession) {
        let puzzle = cedar_station().puzzles[0].clone();
        let session = TimingSession::new(&puzzle, 120.0).expect("demo puzzle 0 is a Timing puzzle");
        (puzzle, session)
    }

    #[test]
    fn targets_follow_the_beats() {
        let (_, s) = session();
        let times: Vec<f64> = s.targets().iter().map(|t| t.time_ms).collect();
        assert_eq!(
            times,
            [
                0.0, 500.0, 1000.0, 1250.0, 1500.0, 2000.0, 2500.0, 3000.0, 3250.0, 3500.0
            ]
        );
        assert_eq!(s.lane_count(), 2);
        assert!(TimingSession::new(&cedar_station().puzzles[1], 120.0).is_none());
    }

    #[test]
    fn a_perfect_run_solves_it() {
        let (_, mut s) = session();
        for i in 0..s.targets().len() {
            let t = s.targets()[i];
            let r = s.press(t.time_ms, t.lane, 0.0);
            assert_eq!(
                r,
                PressResult {
                    judgement: Judgement::Perfect,
                    target: Some(i),
                    offset_ms: 0.0
                }
            );
            assert!((s.progress() - (i + 1) as f32 / 10.0).abs() < 1e-6);
        }
        assert!(s.is_solved());
        assert!(s.is_over());
        // A press after the end is Stray but costs nothing.
        assert_eq!(s.press(4000.0, 0, 0.0).judgement, Judgement::Stray);
        assert_eq!(s.mistakes(), 0);
    }

    #[test]
    fn judgements_by_distance_and_lane() {
        let (_, mut s) = session();
        assert_eq!(s.press(-20.0, 0, 0.0).judgement, Judgement::Perfect);
        assert_eq!(s.press(560.0, 0, 0.0).judgement, Judgement::Good);
        // Target 2 (1000 ms) is on lane 1.
        let wrong = s.press(1000.0, 0, 0.0);
        assert_eq!((wrong.judgement, wrong.target), (Judgement::Wrong, Some(2)));
        let early = s.press(880.0, 1, 0.0);
        assert_eq!(
            (early.judgement, early.offset_ms),
            (Judgement::Early, -120.0)
        );
        assert_eq!(s.press(1000.0, 1, 0.0).judgement, Judgement::Perfect);
        // Target 3 (1250 ms, lane 1) pressed 100 ms late: lost.
        let late = s.press(1350.0, 1, 0.0);
        assert_eq!((late.judgement, late.target), (Judgement::Late, Some(3)));
        assert_eq!(s.targets()[3].judgement, Some(Judgement::Late));
        assert_eq!(s.mistakes(), 3);
        assert!(!s.is_solved());
    }

    #[test]
    fn stray_presses_and_too_many_mistakes() {
        let (_, mut s) = session();
        assert_eq!(s.press(-500.0, 0, 0.0), PressResult::stray());
        assert_eq!(s.press(-400.0, 0, 0.0).judgement, Judgement::Stray);
        assert_eq!(s.press(-300.0, 0, 0.0).judgement, Judgement::Stray);
        for i in 0..s.targets().len() {
            let t = s.targets()[i];
            s.press(t.time_ms, t.lane, 0.0);
        }
        assert_eq!(s.progress(), 1.0);
        assert!(!s.is_solved(), "3 mistakes > 2");
        s.reset();
        assert_eq!(s.progress(), 0.0);
        assert_eq!(s.mistakes(), 0);
    }

    #[test]
    fn finish_marks_misses_after_twice_the_window() {
        let (_, mut s) = session();
        s.press(0.0, 0, 0.0);
        assert!(s.finish(660.0).is_empty(), "target 1 could still be Late");
        assert_eq!(s.finish(661.0), vec![1]);
        assert_eq!(s.targets()[1].judgement, Some(Judgement::Miss));
        assert_eq!(s.finish(10_000.0).len(), 8);
        assert!(s.is_over());
        assert!((s.progress() - 0.1).abs() < 1e-6);
    }

    #[test]
    fn paranoia_moves_what_is_heard_not_what_is_judged() {
        let (p, mut s) = session();
        assert_eq!(s.heard_time_ms(0, 1.0), Some(60.0));
        assert_eq!(s.heard_time_ms(1, 1.0), Some(440.0));
        // Following the false beat: 60 ms off, which is only Good.
        for i in 0..s.targets().len() {
            let t = s.targets()[i];
            let heard = t.time_ms + f64::from(heard_offset_ms(&p, 1.0, i));
            let r = s.press(heard, t.lane, 1.0);
            assert_eq!(r.judgement, Judgement::Good);
            assert_eq!(r.offset_ms.abs(), 60.0);
        }
        assert!(s.is_solved());
        assert_eq!(s.fooled(), 10);
    }
}
