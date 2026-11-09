//! Test vectors: recorded input runs with their expected results, for checking the Unity port.
//!
//! Each vector is produced by running this crate's own sessions, so it is the reference the
//! C# code must reproduce exactly. Serialize them with serde (JSON works well) and replay each
//! `inputs` list on a fresh session of the same puzzle at `bpm`.

use aa_core::{Puzzle, PuzzleKind};
use serde::{Deserialize, Serialize};

use crate::{LOOSE_FACTOR, PressResult, Session, heard_offset_ms};

/// What an input does. Serialized as "press", "toggle" or "finish".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputKind {
    /// Timing / Echo: a press on `lane` at `time_ms` with `paranoia`.
    Press,
    /// Pattern: flip the cell at `lane`, `step`.
    Toggle,
    /// Timing / Echo: advance the clock to `time_ms`, marking missed targets.
    Finish,
}

/// One player input. Fields an input kind does not use are 0.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Input {
    pub kind: InputKind,
    pub time_ms: f64,
    pub lane: u8,
    pub step: u16,
    pub paranoia: f32,
}

impl Input {
    pub fn press(time_ms: f64, lane: u8, paranoia: f32) -> Self {
        Input {
            kind: InputKind::Press,
            time_ms,
            lane,
            step: 0,
            paranoia,
        }
    }

    pub fn toggle(lane: u8, step: u16) -> Self {
        Input {
            kind: InputKind::Toggle,
            time_ms: 0.0,
            lane,
            step,
            paranoia: 0.0,
        }
    }

    pub fn finish(time_ms: f64) -> Self {
        Input {
            kind: InputKind::Finish,
            time_ms,
            lane: 0,
            step: 0,
            paranoia: 0.0,
        }
    }
}

/// A recorded run: the inputs and what the session must answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TestVector {
    pub name: String,
    pub bpm: f32,
    pub inputs: Vec<Input>,
    /// One entry per input: the press result, or `None` for toggles and finishes.
    pub expected: Vec<Option<PressResult>>,
    /// `progress()` after the last input.
    pub expected_progress: f32,
    /// `is_solved()` after the last input.
    pub expected_solved: bool,
}

impl TestVector {
    /// Replays the inputs on a fresh session and records what it answers.
    pub fn record(puzzle: &Puzzle, bpm: f32, name: &str, inputs: Vec<Input>) -> Self {
        let mut session = Session::new(puzzle, bpm);
        let expected = inputs.iter().map(|input| session.apply(input)).collect();
        TestVector {
            name: format!("{} / {name}", puzzle.name),
            bpm,
            inputs,
            expected,
            expected_progress: session.progress(),
            expected_solved: session.is_solved(),
        }
    }

    /// True when a fresh session of `puzzle` reproduces this vector exactly.
    pub fn check(&self, puzzle: &Puzzle) -> bool {
        let mut session = Session::new(puzzle, self.bpm);
        let actual: Vec<Option<PressResult>> = self
            .inputs
            .iter()
            .map(|input| session.apply(input))
            .collect();
        actual == self.expected
            && session.progress() == self.expected_progress
            && session.is_solved() == self.expected_solved
    }
}

/// `(time_ms, lane)` of each note to press.
type Times = Vec<(f64, u8)>;

/// A run that presses each target at its time plus `offset(i)`, on lane `lane(i)`.
fn presses(
    targets: &[(f64, u8)],
    paranoia: f32,
    offset: impl Fn(usize) -> f64,
    lane: impl Fn(usize, u8) -> u8,
) -> Vec<Input> {
    targets
        .iter()
        .enumerate()
        .map(|(i, &(t, l))| Input::press(t + offset(i), lane(i, l), paranoia))
        .collect()
}

/// Several runs per puzzle: a perfect run, a sloppy one, wrong lanes, Paranoia, misses…
///
/// The expected results come from running this crate's sessions on the inputs.
pub fn test_vectors(puzzle: &Puzzle, bpm: f32) -> Vec<TestVector> {
    let record = |name: &str, inputs: Vec<Input>| TestVector::record(puzzle, bpm, name, inputs);
    match &puzzle.kind {
        PuzzleKind::Pattern { lanes, steps, .. } => {
            let Session::Pattern(s) = Session::new(puzzle, bpm) else {
                return Vec::new();
            };
            let target: Vec<Input> = s
                .target()
                .iter()
                .map(|&[l, st]| Input::toggle(l as u8, st))
                .collect();
            let wrong_cell = (0..*lanes)
                .flat_map(|l| (0..*steps).map(move |st| [u16::from(l), st]))
                .find(|c| !s.target().contains(c))
                .map(|[l, st]| Input::toggle(l as u8, st));
            let half = target.len() / 2;
            let mut vectors = vec![record("perfect", target.clone())];
            if let Some(wrong) = wrong_cell {
                vectors.push(record("extra cell", [target.clone(), vec![wrong]].concat()));
                vectors.push(record(
                    "fix a mistake",
                    [vec![wrong], target.clone(), vec![wrong]].concat(),
                ));
            }
            vectors.push(record("half done", target[..half].to_vec()));
            if let Some(&first) = target.first() {
                vectors.push(record(
                    "toggled off again",
                    [target.clone(), vec![first]].concat(),
                ));
            }
            vectors.push(record(
                "outside the grid",
                [
                    vec![Input::toggle(*lanes, 0), Input::toggle(0, *steps)],
                    target,
                ]
                .concat(),
            ));
            vectors
        }
        PuzzleKind::Timing { .. } | PuzzleKind::Echo { .. } => {
            let (targets, window, before_answer): (Times, f64, Times) =
                match Session::new(puzzle, bpm) {
                    Session::Timing(s) => (
                        s.targets().iter().map(|t| (t.time_ms, t.lane)).collect(),
                        f64::from(puzzle.window_ms),
                        Vec::new(),
                    ),
                    Session::Echo(s) => (
                        s.targets().iter().map(|t| (t.answer_ms, t.lane)).collect(),
                        f64::from(puzzle.window_ms),
                        s.call_notes().iter().map(|&(t, l, _)| (t, l)).collect(),
                    ),
                    Session::Pattern(_) => return Vec::new(),
                };
            let end = targets.iter().map(|t| t.0).fold(0.0, f64::max) + window * LOOSE_FACTOR + 1.0;
            let finish = Input::finish(end);
            let same = |_: usize, l: u8| l;
            let with_finish = |mut inputs: Vec<Input>| {
                inputs.push(finish);
                inputs
            };
            let sloppy = [0.5, -0.8, 0.2, -0.6, 0.9];

            let mut vectors = vec![
                record(
                    "perfect",
                    with_finish(presses(&targets, 0.0, |_| 0.0, same)),
                ),
                record(
                    "sloppy",
                    with_finish(presses(
                        &targets,
                        0.0,
                        |i| sloppy[i % sloppy.len()] * window,
                        same,
                    )),
                ),
                record(
                    "paranoia: following the heard beat",
                    with_finish(presses(
                        &targets,
                        1.0,
                        |i| f64::from(heard_offset_ms(puzzle, 1.0, i)),
                        same,
                    )),
                ),
                record(
                    "misses",
                    with_finish(
                        presses(&targets, 0.0, |_| 0.0, same)
                            .into_iter()
                            .step_by(2)
                            .collect(),
                    ),
                ),
            ];
            if let Some(&(t0, l0)) = targets.first() {
                let rest = presses(&targets[1..], 0.0, |_| 0.0, same);
                let wrong = vec![
                    Input::press(t0, l0.wrapping_add(1), 0.0),
                    Input::press(t0 + 0.5 * window, l0, 0.0),
                ];
                vectors.push(record(
                    "wrong lane, then right",
                    with_finish([wrong, rest.clone()].concat()),
                ));
                let late = vec![Input::press(t0 + 1.5 * window, l0, 0.0)];
                vectors.push(record(
                    "late first press",
                    with_finish([late, rest].concat()),
                ));
            }
            let early_then_right: Vec<Input> = targets
                .iter()
                .enumerate()
                .flat_map(|(i, &(t, l))| {
                    let early = (i < 3).then(|| Input::press(t - 1.5 * window, l, 0.0));
                    early.into_iter().chain([Input::press(t, l, 0.0)])
                })
                .collect();
            vectors.push(record("three early presses", with_finish(early_then_right)));
            if targets.len() >= 2 {
                // Swap the lanes of the first two presses: out of order in Echo.
                let swapped = presses(
                    &targets,
                    0.0,
                    |_| 0.0,
                    |i, l| match i {
                        0 => targets[1].1,
                        1 => targets[0].1,
                        _ => l,
                    },
                );
                vectors.push(record("first two lanes swapped", with_finish(swapped)));
            }
            if !before_answer.is_empty() {
                let along = presses(&before_answer, 0.0, |_| 0.0, same);
                let answer = presses(&targets, 0.0, |_| 0.0, same);
                vectors.push(record(
                    "tapping along with the call",
                    with_finish([along, answer].concat()),
                ));
            }
            vectors
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Judgement;
    use aa_core::demo::cedar_station;

    #[test]
    fn every_vector_replays_exactly_and_survives_json() {
        for puzzle in cedar_station().puzzles {
            let vectors = test_vectors(&puzzle, 120.0);
            assert!(
                vectors.len() >= 5,
                "{}: only {} vectors",
                puzzle.name,
                vectors.len()
            );
            for v in &vectors {
                assert_eq!(v.inputs.len(), v.expected.len());
                assert!(v.check(&puzzle), "{} does not replay", v.name);
                let json = serde_json::to_string(v).expect("vectors serialize");
                let back: TestVector = serde_json::from_str(&json).expect("vectors deserialize");
                assert_eq!(&back, v);
                assert!(back.check(&puzzle));
            }
        }
    }

    #[test]
    fn vectors_cover_solved_and_unsolved_runs() {
        for puzzle in cedar_station().puzzles {
            let vectors = test_vectors(&puzzle, 120.0);
            let perfect = &vectors[0];
            assert!(perfect.expected_solved, "{}", perfect.name);
            assert_eq!(perfect.expected_progress, 1.0);
            assert!(
                vectors.iter().any(|v| !v.expected_solved),
                "{} has no failing run",
                puzzle.name
            );
        }
    }

    #[test]
    fn timing_vectors_contain_the_expected_judgements() {
        let puzzle = &cedar_station().puzzles[0];
        let vectors = test_vectors(puzzle, 120.0);
        let find = |name: &str| {
            vectors
                .iter()
                .find(|v| v.name.ends_with(name))
                .expect("vector exists")
        };
        let judgements = |v: &TestVector| -> Vec<Judgement> {
            v.expected.iter().flatten().map(|r| r.judgement).collect()
        };
        assert!(
            judgements(find("paranoia: following the heard beat"))
                .iter()
                .all(|&j| j == Judgement::Good)
        );
        assert_eq!(
            judgements(find("wrong lane, then right"))[..2],
            [Judgement::Wrong, Judgement::Good]
        );
        assert_eq!(judgements(find("late first press"))[0], Judgement::Late);
        assert_eq!(
            judgements(find("three early presses"))
                .iter()
                .filter(|&&j| j == Judgement::Early)
                .count(),
            3
        );
        assert!(!find("three early presses").expected_solved);
        assert!((find("misses").expected_progress - 0.5).abs() < 1e-6);
    }

    #[test]
    fn inputs_serialize_with_lowercase_kinds() {
        let json = serde_json::to_string(&Input::toggle(1, 2)).expect("serializes");
        assert!(json.contains("\"kind\":\"toggle\""), "{json}");
    }
}
