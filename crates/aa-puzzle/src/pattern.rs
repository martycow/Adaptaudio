//! Pattern puzzles: fill a lanes × steps grid so it matches the target rhythm.

use aa_core::{Puzzle, PuzzleKind};

/// A Pattern puzzle being played: a grid of cells, each on or off.
#[derive(Clone, Debug, PartialEq)]
pub struct PatternSession {
    lanes: u8,
    steps: u16,
    /// Target cells as `[lane, step]`, deduplicated, sorted by step then lane.
    target: Vec<[u16; 2]>,
    pitches: Vec<u8>,
    /// `lanes × steps` cells, row by row (lane 0 first).
    grid: Vec<bool>,
}

impl PatternSession {
    /// Starts a Pattern puzzle with an empty grid. `None` when the puzzle is of another kind.
    ///
    /// Target cells outside the grid are ignored.
    pub fn new(puzzle: &Puzzle) -> Option<Self> {
        match &puzzle.kind {
            PuzzleKind::Pattern {
                lanes,
                steps,
                target,
                pitches,
            } => Some(Self::build(*lanes, *steps, target, pitches)),
            _ => None,
        }
    }

    pub(crate) fn build(lanes: u8, steps: u16, target: &[[u16; 2]], pitches: &[u8]) -> Self {
        let mut cells: Vec<[u16; 2]> = target
            .iter()
            .copied()
            .filter(|&[lane, step]| lane < u16::from(lanes) && step < steps)
            .collect();
        cells.sort_by_key(|&[lane, step]| (step, lane));
        cells.dedup();
        PatternSession {
            lanes,
            steps,
            target: cells,
            pitches: pitches.to_vec(),
            grid: vec![false; usize::from(lanes) * usize::from(steps)],
        }
    }

    pub fn lanes(&self) -> u8 {
        self.lanes
    }

    pub fn steps(&self) -> u16 {
        self.steps
    }

    /// The cells to find, as `[lane, step]`, earliest step first.
    pub fn target(&self) -> &[[u16; 2]] {
        &self.target
    }

    fn index(&self, lane: u16, step: u16) -> Option<usize> {
        (lane < u16::from(self.lanes) && step < self.steps)
            .then(|| usize::from(lane) * usize::from(self.steps) + usize::from(step))
    }

    pub fn is_on(&self, lane: u8, step: u16) -> bool {
        self.index(u16::from(lane), step)
            .and_then(|i| self.grid.get(i).copied())
            .unwrap_or(false)
    }

    /// Flips a cell. Returns its new state; cells outside the grid are ignored (`false`).
    pub fn toggle(&mut self, lane: u8, step: u16) -> bool {
        match self.index(u16::from(lane), step) {
            Some(i) => {
                self.grid[i] = !self.grid[i];
                self.grid[i]
            }
            None => false,
        }
    }

    /// Every cell that is on, as `[lane, step]`, lane 0 first then by step.
    pub fn cells(&self) -> Vec<[u16; 2]> {
        let steps = usize::from(self.steps.max(1));
        self.grid
            .iter()
            .enumerate()
            .filter(|&(_, &on)| on)
            .map(|(i, _)| [(i / steps) as u16, (i % steps) as u16])
            .collect()
    }

    fn is_on_cell(&self, [lane, step]: [u16; 2]) -> bool {
        self.index(lane, step)
            .and_then(|i| self.grid.get(i).copied())
            .unwrap_or(false)
    }

    /// Target cells that are on.
    pub fn correct(&self) -> usize {
        self.target.iter().filter(|&&c| self.is_on_cell(c)).count()
    }

    /// Cells that are on but not in the target.
    pub fn wrong_cells(&self) -> Vec<[u16; 2]> {
        self.cells()
            .into_iter()
            .filter(|c| !self.target.contains(c))
            .collect()
    }

    /// (correct − wrong) / target cells, clamped to 0..1 (1 for an empty target).
    pub fn progress(&self) -> f32 {
        if self.target.is_empty() {
            return if self.wrong_cells().is_empty() {
                1.0
            } else {
                0.0
            };
        }
        let score = self.correct() as f32 - self.wrong_cells().len() as f32;
        (score / self.target.len() as f32).clamp(0.0, 1.0)
    }

    /// The grid matches the target exactly.
    pub fn is_solved(&self) -> bool {
        self.correct() == self.target.len() && self.wrong_cells().is_empty()
    }

    /// A target cell that is still off (the earliest step first), or `None` when none is
    /// missing. There may still be wrong cells to clear: see [`PatternSession::wrong_cells`].
    pub fn hint(&self) -> Option<[u16; 2]> {
        self.target.iter().copied().find(|&c| !self.is_on_cell(c))
    }

    /// What the grid plays: `(step, pitch)` for every cell that is on, by step then lane.
    /// A lane without a pitch in the puzzle plays middle C (60).
    pub fn notes_for_playback(&self) -> Vec<(u16, u8)> {
        let mut cells = self.cells();
        cells.sort_by_key(|&[lane, step]| (step, lane));
        cells
            .into_iter()
            .map(|[lane, step]| {
                (
                    step,
                    self.pitches.get(usize::from(lane)).copied().unwrap_or(60),
                )
            })
            .collect()
    }

    /// Clears the grid.
    pub fn reset(&mut self) {
        self.grid.fill(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aa_core::demo::cedar_station;

    /// "Fuse box rhythm": 3 lanes × 8 steps, 6 target cells.
    fn session() -> PatternSession {
        PatternSession::new(&cedar_station().puzzles[1]).expect("demo puzzle 1 is a Pattern puzzle")
    }

    #[test]
    fn filling_the_target_solves_it() {
        let mut s = session();
        assert_eq!(s.progress(), 0.0);
        assert_eq!(s.hint(), Some([0, 0]));
        for [lane, step] in s.target().to_vec() {
            assert!(s.toggle(lane as u8, step));
        }
        assert!(s.is_solved());
        assert_eq!(s.progress(), 1.0);
        assert_eq!(s.hint(), None);
        assert_eq!(
            s.notes_for_playback(),
            [(0, 50), (2, 57), (3, 62), (4, 50), (6, 57), (7, 62)]
        );
        assert!(PatternSession::new(&cedar_station().puzzles[0]).is_none());
    }

    #[test]
    fn wrong_cells_cost_progress() {
        let mut s = session();
        s.toggle(0, 0);
        s.toggle(0, 4);
        s.toggle(1, 2);
        assert_eq!(s.progress(), 0.5);
        s.toggle(2, 0); // not in the target
        assert!((s.progress() - 2.0 / 6.0).abs() < 1e-6);
        assert_eq!(s.wrong_cells(), vec![[2, 0]]);
        assert_eq!(s.hint(), Some([2, 3]));
        assert!(!s.toggle(2, 0));
        assert_eq!(s.progress(), 0.5);
        for _ in 0..10 {
            s.toggle(1, 1);
            s.toggle(1, 1);
        }
        s.toggle(1, 7);
        s.toggle(1, 5);
        s.toggle(1, 3);
        s.toggle(2, 1);
        assert_eq!(s.progress(), 0.0, "never below zero");
    }

    #[test]
    fn out_of_range_toggles_are_ignored_and_reset_clears() {
        let mut s = session();
        assert!(!s.toggle(3, 0));
        assert!(!s.toggle(0, 8));
        assert!(s.cells().is_empty());
        s.toggle(1, 6);
        assert!(s.is_on(1, 6));
        assert_eq!(s.cells(), vec![[1, 6]]);
        s.reset();
        assert!(s.cells().is_empty());
    }
}
