//! Euclidean rhythms: spread a number of hits as evenly as possible over a number of steps.
//!
//! "3 hits over 8 steps" gives `x..x..x.`, the tresillo heard all over Latin and pop music.
//! "5 over 16" is a classic bossa-like pattern. Good for drums and for rhythm puzzles alike.

use aa_core::Clip;

/// The Euclidean rhythm with `hits` hits over `steps` steps (Bjorklund's algorithm).
///
/// `true` means "play on this step". `rotation` moves the whole pattern later by that many
/// steps (negative = earlier), wrapping around, so the same rhythm can start somewhere else.
/// More hits than steps simply fills every step.
pub fn euclid(hits: u16, steps: u16, rotation: i32) -> Vec<bool> {
    let steps_n = usize::from(steps);
    let hits_n = usize::from(hits.min(steps));
    if steps_n == 0 {
        return Vec::new();
    }
    if hits_n == 0 {
        return vec![false; steps_n];
    }
    // Start with `hits` groups [x] and `steps - hits` groups [.], then keep appending the
    // remainder groups onto the front groups until at most one remainder group is left.
    let mut front: Vec<Vec<bool>> = vec![vec![true]; hits_n];
    let mut rest: Vec<Vec<bool>> = vec![vec![false]; steps_n - hits_n];
    while rest.len() > 1 {
        let pairs = front.len().min(rest.len());
        let mut joined = Vec::with_capacity(pairs);
        for (a, b) in front.iter().zip(rest.iter()) {
            let mut group = a.clone();
            group.extend_from_slice(b);
            joined.push(group);
        }
        let leftover = if front.len() > pairs {
            front[pairs..].to_vec()
        } else {
            rest[pairs..].to_vec()
        };
        front = joined;
        rest = leftover;
    }
    let mut pattern: Vec<bool> = front.into_iter().chain(rest).flatten().collect();
    let shift = rotation.rem_euclid(i32::from(steps)) as usize;
    pattern.rotate_right(shift);
    pattern
}

/// A drum clip from a Euclidean rhythm: every hit plays `pitch` at `velocity` for one step.
pub fn euclid_clip(
    name: &str,
    hits: u16,
    steps: u16,
    rotation: i32,
    pitch: u8,
    velocity: f32,
) -> Clip {
    let mut clip = Clip::new(name, steps);
    for (step, on) in euclid(hits, steps, rotation).into_iter().enumerate() {
        if on {
            clip = clip.with_note(step as u16, pitch, velocity.clamp(0.0, 1.0), 1.0);
        }
    }
    clip
}

/// A pattern as text: `x` for a hit, `.` for a rest ("x..x..x.").
pub fn pattern_string(pattern: &[bool]) -> String {
    pattern
        .iter()
        .map(|&on| if on { 'x' } else { '.' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(hits: u16, steps: u16, rotation: i32) -> String {
        pattern_string(&euclid(hits, steps, rotation))
    }

    #[test]
    fn classic_euclidean_rhythms() {
        assert_eq!(e(3, 8, 0), "x..x..x.");
        assert_eq!(e(5, 8, 0), "x.xx.xx.");
        assert_eq!(e(4, 16, 0), "x...x...x...x...");
        assert_eq!(e(2, 5, 0), "x.x..");
        assert_eq!(e(7, 12, 0), "x.xx.x.xx.x.");
    }

    #[test]
    fn hit_counts_and_edges() {
        for steps in 1..=32u16 {
            for hits in 0..=steps {
                let p = euclid(hits, steps, 0);
                assert_eq!(p.len(), usize::from(steps));
                assert_eq!(p.iter().filter(|&&x| x).count(), usize::from(hits));
            }
        }
        assert!(euclid(3, 0, 0).is_empty());
        assert_eq!(e(9, 4, 0), "xxxx");
    }

    #[test]
    fn rotation_moves_the_pattern() {
        assert_eq!(e(3, 8, 1), ".x..x..x");
        assert_eq!(e(3, 8, -1), "..x..x.x");
        assert_eq!(e(3, 8, 8), e(3, 8, 0));
    }

    #[test]
    fn clips_have_one_note_per_hit() {
        let clip = euclid_clip("Tresillo", 3, 8, 0, 36, 0.9);
        assert_eq!(clip.length_steps, 8);
        let steps: Vec<u16> = clip.notes.iter().map(|n| n.step).collect();
        assert_eq!(steps, [0, 3, 6]);
    }
}
