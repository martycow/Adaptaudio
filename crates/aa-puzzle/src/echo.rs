//! Echo puzzles: the game plays a phrase (the call), the player repeats it (the answer).

use aa_core::{EchoNote, Puzzle, PuzzleKind};

use crate::{
    DEFAULT_MAX_MISTAKES, Judgement, LOOSE_FACTOR, PressResult, grade, heard_offset, ms_per_beat,
    was_fooled,
};

/// One note of the phrase, with when it is played and when it must be answered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EchoTarget {
    /// When the game plays the note in the call, in ms since the puzzle started.
    pub call_ms: f64,
    /// When the player must press it in the answer, in ms since the puzzle started.
    pub answer_ms: f64,
    pub lane: u8,
    pub pitch: u8,
    /// `None` while not yet answered; then Perfect, Good, Late or Miss.
    pub judgement: Option<Judgement>,
}

/// An Echo puzzle being played.
///
/// The call starts at 0 ms. The answer is the same phrase, `answer_offset_beats` later: note
/// `i` of the call at beat `b` must be answered at beat `answer_offset_beats + b`. Notes are
/// kept in time order and must be answered in that order.
#[derive(Clone, Debug, PartialEq)]
pub struct EchoSession {
    notes: Vec<EchoTarget>,
    answer_start_ms: f64,
    next: usize,
    window_ms: f64,
    paranoia_shift_ms: f32,
    mistakes: u32,
    fooled: u32,
    /// How many mistakes are allowed while still solving the puzzle (default 2).
    pub max_mistakes: u32,
}

impl EchoSession {
    /// Starts an Echo puzzle at a tempo. `None` when the puzzle is of another kind.
    pub fn new(puzzle: &Puzzle, bpm: f32) -> Option<Self> {
        match &puzzle.kind {
            PuzzleKind::Echo {
                phrase,
                answer_offset_beats,
            } => Some(Self::build(puzzle, phrase, *answer_offset_beats, bpm)),
            _ => None,
        }
    }

    pub(crate) fn build(
        puzzle: &Puzzle,
        phrase: &[EchoNote],
        answer_offset_beats: f32,
        bpm: f32,
    ) -> Self {
        let beat_ms = ms_per_beat(bpm);
        let mut phrase = phrase.to_vec();
        phrase.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        let notes = phrase
            .iter()
            .map(|n| EchoTarget {
                call_ms: f64::from(n.beat) * beat_ms,
                answer_ms: (f64::from(answer_offset_beats) + f64::from(n.beat)) * beat_ms,
                lane: n.lane,
                pitch: n.pitch,
                judgement: None,
            })
            .collect();
        EchoSession {
            notes,
            answer_start_ms: f64::from(answer_offset_beats) * beat_ms,
            next: 0,
            window_ms: f64::from(puzzle.window_ms.max(0.0)),
            paranoia_shift_ms: puzzle.paranoia_shift_ms,
            mistakes: 0,
            fooled: 0,
            max_mistakes: DEFAULT_MAX_MISTAKES,
        }
    }

    pub fn targets(&self) -> &[EchoTarget] {
        &self.notes
    }

    /// The call for playback: `(time_ms, lane, pitch)`, at the true times.
    pub fn call_notes(&self) -> Vec<(f64, u8, u8)> {
        self.notes
            .iter()
            .map(|n| (n.call_ms, n.lane, n.pitch))
            .collect()
    }

    /// The call as a paranoid opossum hears it: every note moved by
    /// [`crate::heard_offset_ms`]. The answer is still judged against the true rhythm.
    pub fn call_notes_heard(&self, paranoia: f32) -> Vec<(f64, u8, u8)> {
        self.notes
            .iter()
            .enumerate()
            .map(|(i, n)| {
                (
                    n.call_ms + f64::from(heard_offset(self.paranoia_shift_ms, paranoia, i)),
                    n.lane,
                    n.pitch,
                )
            })
            .collect()
    }

    /// When the answer begins (the first possible answer note), in ms since the puzzle started.
    pub fn answer_start_ms(&self) -> f64 {
        self.answer_start_ms
    }

    /// The index of the note the player should press next.
    pub fn next_index(&self) -> usize {
        self.next
    }

    /// Judges a press on `lane` at `time_ms` against the note expected next.
    ///
    /// - If the expected note's window has already passed, it is missed and the next note is
    ///   expected instead; a press on its lane within twice the window counts as Late for it.
    /// - Right lane within the window: a hit (Perfect or Good), and the next note is expected.
    /// - Right lane within twice the window before it: Early (try again).
    /// - Another lane within twice the window: Wrong (out of order); the note is still expected.
    /// - Anything else is Stray.
    ///
    /// Presses during the call (before the answer can begin) and after the last note are
    /// Stray but cost nothing: tapping along with the call is fine.
    pub fn press(&mut self, time_ms: f64, lane: u8, paranoia: f32) -> PressResult {
        let w = self.window_ms;
        let loose = w * LOOSE_FACTOR;
        if time_ms < self.answer_start_ms - loose {
            return PressResult::stray();
        }
        while let Some(&note) = self.notes.get(self.next) {
            let i = self.next;
            let offset = time_ms - note.answer_ms;
            if offset > w {
                self.next += 1;
                let late = note.lane == lane && offset <= loose;
                self.notes[i].judgement = Some(if late {
                    Judgement::Late
                } else {
                    Judgement::Miss
                });
                if late {
                    return self.result(Judgement::Late, i, time_ms, paranoia);
                }
                continue;
            }
            if note.lane == lane {
                return match grade(offset, w) {
                    Some(j) if j.is_hit() => {
                        self.notes[i].judgement = Some(j);
                        self.next += 1;
                        self.result(j, i, time_ms, paranoia)
                    }
                    Some(j) => self.result(j, i, time_ms, paranoia),
                    None => self.stray(),
                };
            }
            return if offset >= -loose {
                self.result(Judgement::Wrong, i, time_ms, paranoia)
            } else {
                self.stray()
            };
        }
        PressResult::stray()
    }

    fn stray(&mut self) -> PressResult {
        self.mistakes += 1;
        PressResult::stray()
    }

    fn result(
        &mut self,
        judgement: Judgement,
        i: usize,
        time_ms: f64,
        paranoia: f32,
    ) -> PressResult {
        let target_ms = self.notes[i].answer_ms;
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

    /// Advances the clock: every expected note more than twice the window before `time_ms`
    /// becomes Miss. Call it every frame and once at the end. Returns the newly missed notes.
    pub fn finish(&mut self, time_ms: f64) -> Vec<usize> {
        let limit = self.window_ms * LOOSE_FACTOR;
        let mut missed = Vec::new();
        while let Some(note) = self.notes.get_mut(self.next) {
            if time_ms <= note.answer_ms + limit {
                break;
            }
            note.judgement = Some(Judgement::Miss);
            missed.push(self.next);
            self.next += 1;
        }
        missed
    }

    /// Number of notes answered (Perfect or Good).
    pub fn hits(&self) -> usize {
        self.notes
            .iter()
            .filter(|n| n.judgement.is_some_and(Judgement::is_hit))
            .count()
    }

    /// Fraction of notes answered, 0..1 (1 for an empty phrase).
    pub fn progress(&self) -> f32 {
        if self.notes.is_empty() {
            1.0
        } else {
            self.hits() as f32 / self.notes.len() as f32
        }
    }

    pub fn mistakes(&self) -> u32 {
        self.mistakes
    }

    /// Presses that followed the false, Paranoia-shifted rhythm rather than the true one.
    pub fn fooled(&self) -> u32 {
        self.fooled
    }

    /// Every note answered, in order, with no more than `max_mistakes` mistakes.
    pub fn is_solved(&self) -> bool {
        self.hits() == self.notes.len() && self.mistakes <= self.max_mistakes
    }

    /// True when every note has been answered or missed.
    pub fn is_over(&self) -> bool {
        self.next >= self.notes.len()
    }

    /// Starts again: the game plays the call again from 0 ms (keeps `max_mistakes`).
    pub fn reset(&mut self) {
        for n in &mut self.notes {
            n.judgement = None;
        }
        self.next = 0;
        self.mistakes = 0;
        self.fooled = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aa_core::demo::cedar_station;

    /// "Owl call" at 120 BPM: notes at beats 0, 1, 1.5, 2 (lanes 0, 2, 1, 0), answered from
    /// beat 4 (2000 ms), window ±110 ms.
    fn session() -> EchoSession {
        EchoSession::new(&cedar_station().puzzles[2], 120.0)
            .expect("demo puzzle 2 is an Echo puzzle")
    }

    fn answers(s: &EchoSession) -> Vec<(f64, u8)> {
        s.targets().iter().map(|n| (n.answer_ms, n.lane)).collect()
    }

    #[test]
    fn call_and_answer_times() {
        let s = session();
        assert_eq!(
            s.call_notes(),
            [
                (0.0, 0, 74),
                (500.0, 2, 81),
                (750.0, 1, 77),
                (1000.0, 0, 74)
            ]
        );
        assert_eq!(s.answer_start_ms(), 2000.0);
        assert_eq!(
            answers(&s),
            [(2000.0, 0), (2500.0, 2), (2750.0, 1), (3000.0, 0)]
        );
        assert_eq!(s.call_notes_heard(1.0)[1].0, 460.0);
        assert!(EchoSession::new(&cedar_station().puzzles[0], 120.0).is_none());
    }

    #[test]
    fn answering_in_order_solves_it() {
        let mut s = session();
        // Tapping along with the call is free.
        for (t, lane, _) in s.call_notes() {
            assert_eq!(s.press(t, lane, 0.0).judgement, Judgement::Stray);
        }
        assert_eq!(s.mistakes(), 0);
        for (i, (t, lane)) in answers(&s).into_iter().enumerate() {
            let r = s.press(t + 50.0, lane, 0.0);
            assert_eq!(
                (r.judgement, r.target, r.offset_ms),
                (Judgement::Good, Some(i), 50.0)
            );
        }
        assert!(s.is_solved());
        assert!(s.is_over());
        assert_eq!(s.press(3500.0, 0, 0.0).judgement, Judgement::Stray);
        assert_eq!(s.mistakes(), 0);
    }

    #[test]
    fn out_of_order_is_wrong_and_the_note_stays_expected() {
        let mut s = session();
        assert_eq!(s.press(2000.0, 0, 0.0).judgement, Judgement::Perfect);
        // Pressing note 2's lane (1) when note 1 (lane 2) is expected.
        let r = s.press(2500.0, 1, 0.0);
        assert_eq!((r.judgement, r.target), (Judgement::Wrong, Some(1)));
        assert_eq!(s.next_index(), 1);
        assert_eq!(s.press(2520.0, 2, 0.0).judgement, Judgement::Perfect);
        assert_eq!(s.press(2600.0, 1, 0.0).judgement, Judgement::Early);
        assert_eq!(s.press(2750.0, 1, 0.0).judgement, Judgement::Perfect);
        assert_eq!(s.press(3000.0, 0, 0.0).judgement, Judgement::Perfect);
        assert_eq!(s.mistakes(), 2);
        assert!(s.is_solved(), "2 mistakes are allowed");
    }

    #[test]
    fn late_and_missed_notes() {
        let mut s = session();
        let late = s.press(2150.0, 0, 0.0);
        assert_eq!((late.judgement, late.target), (Judgement::Late, Some(0)));
        // Skip note 1 entirely: pressing note 2 on time misses note 1 and hits note 2.
        assert_eq!(s.press(2750.0, 1, 0.0).judgement, Judgement::Perfect);
        assert_eq!(s.targets()[1].judgement, Some(Judgement::Miss));
        assert!(s.finish(3220.0).is_empty());
        assert_eq!(s.finish(3221.0), vec![3]);
        assert!(s.is_over());
        assert_eq!(s.progress(), 0.25);
        assert!(!s.is_solved());
        s.reset();
        assert_eq!((s.progress(), s.next_index(), s.mistakes()), (0.0, 0, 0));
    }

    #[test]
    fn paranoia_copies_the_false_rhythm() {
        let mut s = session();
        let heard = s.call_notes_heard(1.0);
        for (i, (t, lane)) in answers(&s).into_iter().enumerate() {
            let shift = heard[i].0 - s.call_notes()[i].0;
            assert_eq!(s.press(t + shift, lane, 1.0).judgement, Judgement::Good);
        }
        assert_eq!(s.fooled(), 4);
        assert!(s.is_solved());
    }
}
