//! aa-puzzle: the rules of Cedar Station's rhythm puzzles, with no UI and no audio.
//!
//! The same rules are ported to C# for Unity, so everything here is deliberately plain and
//! fully deterministic: no clocks, no randomness, no floating-point tricks. The caller passes
//! every time in milliseconds since the puzzle started. [`test_vectors`] records input runs
//! and their expected results so the Unity port can be checked against this code.
//!
//! Three kinds of puzzle, over [`aa_core::Puzzle`]:
//!
//! - [`TimingSession`]: press the right lane on each beat.
//! - [`PatternSession`]: fill a grid of steps so it matches the target rhythm.
//! - [`EchoSession`]: the game plays a phrase (the call), the player repeats it (the answer).
//!
//! ## Judging a press
//!
//! For a press near a target (`w` = the puzzle's `window_ms`):
//!
//! | Offset from the target   | Right lane | Wrong lane |
//! |--------------------------|------------|------------|
//! | within 35 % of `w`       | Perfect    | Wrong      |
//! | within `w`               | Good       | Wrong      |
//! | within `2 × w`           | Early / Late | Wrong    |
//! | further                  | Stray      | Stray      |
//!
//! Perfect and Good are hits. Early, Late, Wrong and Stray are mistakes. A target nobody hit
//! becomes Miss once it is `2 × w` in the past (when [`TimingSession::finish`] or
//! [`EchoSession::finish`] is called with a later time). A puzzle is solved when every target
//! is hit with at most `max_mistakes` mistakes (2 by default).
//!
//! ## Paranoia
//!
//! Paranoia shifts the beat the player *hears*, never the beat they are judged against. The
//! game plays target `i` at `time + heard_offset_ms(puzzle, paranoia, i)`, but presses are
//! always compared with the true time. A paranoid opossum who trusts their ears lands off the
//! beat; one who trusts the steady pulse still lands on it. Presses that were closer to the
//! false beat than to the true one are counted by `fooled()`.

mod echo;
mod pattern;
mod timing;
mod vectors;

use aa_core::{Puzzle, PuzzleKind};
use serde::{Deserialize, Serialize};

pub use echo::{EchoSession, EchoTarget};
pub use pattern::PatternSession;
pub use timing::{TimingSession, TimingTarget};
pub use vectors::{Input, InputKind, TestVector, test_vectors};

/// A press within this fraction of the window is Perfect.
pub const PERFECT_FRACTION: f64 = 0.35;
/// A press within this many windows of a target still counts as an attempt (Early/Late/Wrong).
pub const LOOSE_FACTOR: f64 = 2.0;
/// Mistakes allowed by default before a puzzle can no longer be solved.
pub const DEFAULT_MAX_MISTAKES: u32 = 2;

/// How a press (or a target) was judged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Judgement {
    /// Right lane, very close to the beat.
    Perfect,
    /// Right lane, within the window.
    Good,
    /// Right lane, too early (within twice the window). A mistake; the target stays open.
    Early,
    /// Right lane, too late (within twice the window). A mistake; the target is lost.
    Late,
    /// The wrong lane near a target, or out of order in Echo. A mistake.
    Wrong,
    /// A press near nothing. A mistake.
    Stray,
    /// A target nobody pressed in time.
    Miss,
}

impl Judgement {
    /// Perfect or Good.
    pub fn is_hit(self) -> bool {
        matches!(self, Judgement::Perfect | Judgement::Good)
    }

    /// Early, Late, Wrong or Stray: a press that counts against `max_mistakes`.
    pub fn is_mistake(self) -> bool {
        matches!(
            self,
            Judgement::Early | Judgement::Late | Judgement::Wrong | Judgement::Stray
        )
    }

    pub fn name(self) -> &'static str {
        match self {
            Judgement::Perfect => "Perfect",
            Judgement::Good => "Good",
            Judgement::Early => "Early",
            Judgement::Late => "Late",
            Judgement::Wrong => "Wrong",
            Judgement::Stray => "Stray",
            Judgement::Miss => "Miss",
        }
    }
}

/// The result of one press.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PressResult {
    pub judgement: Judgement,
    /// The target the press was judged against (an index into the session's targets).
    pub target: Option<usize>,
    /// Press time minus the target's true time: negative = early. 0 when there is no target.
    pub offset_ms: f32,
}

impl PressResult {
    fn stray() -> Self {
        PressResult {
            judgement: Judgement::Stray,
            target: None,
            offset_ms: 0.0,
        }
    }
}

/// Grades a right-lane press by its distance from the target, or `None` when too far.
fn grade(offset_ms: f64, window_ms: f64) -> Option<Judgement> {
    let distance = offset_ms.abs();
    if distance <= window_ms * PERFECT_FRACTION {
        Some(Judgement::Perfect)
    } else if distance <= window_ms {
        Some(Judgement::Good)
    } else if distance <= window_ms * LOOSE_FACTOR {
        Some(if offset_ms < 0.0 {
            Judgement::Early
        } else {
            Judgement::Late
        })
    } else {
        None
    }
}

/// Milliseconds per beat at a tempo (tempos below 1 BPM are treated as 1).
fn ms_per_beat(bpm: f32) -> f64 {
    60_000.0 / f64::from(bpm.max(1.0))
}

fn heard_offset(paranoia_shift_ms: f32, paranoia: f32, index: usize) -> f32 {
    let sign = if index.is_multiple_of(2) { 1.0 } else { -1.0 };
    sign * paranoia.clamp(0.0, 1.0) * paranoia_shift_ms
}

/// How far the beat the player *hears* is moved from the true beat for target `index`.
///
/// Even targets are pushed later, odd ones pulled earlier, by `paranoia × paranoia_shift_ms`
/// (Paranoia is clamped to 0..1). Judging always uses the true beat; only playback moves.
pub fn heard_offset_ms(puzzle: &Puzzle, paranoia: f32, index: usize) -> f32 {
    heard_offset(puzzle.paranoia_shift_ms, paranoia, index)
}

/// True when the press was closer to the false (heard) beat than to the true one.
fn was_fooled(time_ms: f64, target_ms: f64, heard_ms: f32, paranoia: f32) -> bool {
    paranoia > 0.0
        && heard_ms != 0.0
        && (time_ms - (target_ms + f64::from(heard_ms))).abs() < (time_ms - target_ms).abs()
}

/// One line about a puzzle for the UI, e.g. "Timing: hit 10 beats on 2 lanes, ±80 ms".
pub fn describe(puzzle: &Puzzle) -> String {
    let paranoia = if puzzle.paranoia_shift_ms > 0.0 {
        format!(
            "; Paranoia shifts the beat up to {} ms",
            puzzle.paranoia_shift_ms
        )
    } else {
        String::new()
    };
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    match &puzzle.kind {
        PuzzleKind::Timing { beats, lanes } => {
            let lane_count = lanes
                .iter()
                .copied()
                .max()
                .map_or(1, |m| usize::from(m) + 1);
            format!(
                "Timing: hit {} beat{} on {} lane{}, ±{} ms{paranoia}",
                beats.len(),
                plural(beats.len()),
                lane_count,
                plural(lane_count),
                puzzle.window_ms
            )
        }
        PuzzleKind::Pattern {
            lanes,
            steps,
            target,
            ..
        } => format!(
            "Pattern: fill {} cell{} on a {} × {} grid{paranoia}",
            target.len(),
            plural(target.len()),
            lanes,
            steps
        ),
        PuzzleKind::Echo {
            phrase,
            answer_offset_beats,
            ..
        } => format!(
            "Echo: repeat a {}-note call, answer after {} beats, ±{} ms{paranoia}",
            phrase.len(),
            answer_offset_beats,
            puzzle.window_ms
        ),
    }
}

/// Any of the three sessions, driven by the same [`Input`]s. Handy for tools and tests.
#[derive(Clone, Debug, PartialEq)]
pub enum Session {
    Timing(TimingSession),
    Pattern(PatternSession),
    Echo(EchoSession),
}

impl Session {
    pub fn new(puzzle: &Puzzle, bpm: f32) -> Self {
        match &puzzle.kind {
            PuzzleKind::Timing { beats, lanes } => {
                Session::Timing(TimingSession::build(puzzle, beats, lanes, bpm))
            }
            PuzzleKind::Pattern {
                lanes,
                steps,
                target,
                pitches,
            } => Session::Pattern(PatternSession::build(*lanes, *steps, target, pitches)),
            PuzzleKind::Echo {
                phrase,
                answer_offset_beats,
            } => Session::Echo(EchoSession::build(
                puzzle,
                phrase,
                *answer_offset_beats,
                bpm,
            )),
        }
    }

    /// Applies one input. Returns the press result for presses; `None` for toggles, finishes
    /// and inputs the puzzle kind ignores (a toggle in a Timing puzzle, a press in Pattern).
    pub fn apply(&mut self, input: &Input) -> Option<PressResult> {
        match (self, input.kind) {
            (Session::Timing(s), InputKind::Press) => {
                Some(s.press(input.time_ms, input.lane, input.paranoia))
            }
            (Session::Timing(s), InputKind::Finish) => {
                s.finish(input.time_ms);
                None
            }
            (Session::Echo(s), InputKind::Press) => {
                Some(s.press(input.time_ms, input.lane, input.paranoia))
            }
            (Session::Echo(s), InputKind::Finish) => {
                s.finish(input.time_ms);
                None
            }
            (Session::Pattern(s), InputKind::Toggle) => {
                s.toggle(input.lane, input.step);
                None
            }
            _ => None,
        }
    }

    pub fn progress(&self) -> f32 {
        match self {
            Session::Timing(s) => s.progress(),
            Session::Pattern(s) => s.progress(),
            Session::Echo(s) => s.progress(),
        }
    }

    pub fn is_solved(&self) -> bool {
        match self {
            Session::Timing(s) => s.is_solved(),
            Session::Pattern(s) => s.is_solved(),
            Session::Echo(s) => s.is_solved(),
        }
    }

    pub fn reset(&mut self) {
        match self {
            Session::Timing(s) => s.reset(),
            Session::Pattern(s) => s.reset(),
            Session::Echo(s) => s.reset(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grading_thresholds() {
        assert_eq!(grade(0.0, 100.0), Some(Judgement::Perfect));
        assert_eq!(grade(-35.0, 100.0), Some(Judgement::Perfect));
        assert_eq!(grade(36.0, 100.0), Some(Judgement::Good));
        assert_eq!(grade(-100.0, 100.0), Some(Judgement::Good));
        assert_eq!(grade(-150.0, 100.0), Some(Judgement::Early));
        assert_eq!(grade(200.0, 100.0), Some(Judgement::Late));
        assert_eq!(grade(201.0, 100.0), None);
    }

    #[test]
    fn heard_offsets_alternate_and_scale_with_paranoia() {
        let p = &aa_core::demo::cedar_station().puzzles[0];
        assert_eq!(heard_offset_ms(p, 0.0, 0), 0.0);
        assert_eq!(heard_offset_ms(p, 1.0, 0), 60.0);
        assert_eq!(heard_offset_ms(p, 1.0, 1), -60.0);
        assert_eq!(heard_offset_ms(p, 0.5, 2), 30.0);
        assert_eq!(heard_offset_ms(p, 7.0, 3), -60.0);
    }

    #[test]
    fn descriptions_name_the_kind() {
        let puzzles = aa_core::demo::cedar_station().puzzles;
        assert_eq!(
            describe(&puzzles[0]),
            "Timing: hit 10 beats on 2 lanes, ±80 ms; Paranoia shifts the beat up to 60 ms"
        );
        assert_eq!(
            describe(&puzzles[1]),
            "Pattern: fill 6 cells on a 3 × 8 grid"
        );
        assert!(
            describe(&puzzles[2]).starts_with("Echo: repeat a 4-note call, answer after 4 beats")
        );
    }
}
