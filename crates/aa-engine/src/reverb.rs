//! A Freeverb-style stereo reverb: eight damped combs and four allpasses per side.
//! Buffers are allocated once at construction.

const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    store: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Comb {
            buf: vec![0.0; len.max(1)],
            idx: 0,
            store: 0.0,
        }
    }

    fn process(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buf[self.idx];
        self.store = out * (1.0 - damp) + self.store * damp;
        self.buf[self.idx] = x + self.store * feedback;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Allpass {
            buf: vec![0.0; len.max(1)],
            idx: 0,
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let b = self.buf[self.idx];
        self.buf[self.idx] = x + b * 0.5;
        self.idx = (self.idx + 1) % self.buf.len();
        b - x
    }
}

pub struct Reverb {
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
    feedback: f32,
    damp: f32,
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / 44_100.0;
        let len = |n: usize, side: usize| ((n + side * STEREO_SPREAD) as f32 * scale) as usize;
        let side = |s: usize| {
            COMBS
                .iter()
                .map(|&n| Comb::new(len(n, s)))
                .collect::<Vec<_>>()
        };
        let aps = |s: usize| {
            ALLPASSES
                .iter()
                .map(|&n| Allpass::new(len(n, s)))
                .collect::<Vec<_>>()
        };
        Reverb {
            combs: [side(0), side(1)],
            allpasses: [aps(0), aps(1)],
            feedback: 0.84,
            damp: 0.35,
        }
    }

    /// Room size 0..1.
    pub fn set_size(&mut self, size: f32) {
        self.feedback = 0.7 + 0.28 * size.clamp(0.0, 1.0);
    }

    /// Processes one sample of mono send into a stereo wet pair.
    pub fn process(&mut self, input: f32) -> (f32, f32) {
        let x = input * 0.015;
        let mut out = [0.0f32; 2];
        for (side, o) in out.iter_mut().enumerate() {
            let mut acc = 0.0;
            for c in &mut self.combs[side] {
                acc += c.process(x, self.feedback, self.damp);
            }
            for a in &mut self.allpasses[side] {
                acc = a.process(acc);
            }
            *o = acc;
        }
        (out[0], out[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_impulse_rings_out_and_decays() {
        let mut r = Reverb::new(48_000.0);
        r.set_size(0.5);
        let (mut early, mut late) = (0.0f32, 0.0f32);
        for i in 0..(48_000 * 6) {
            let (l, rr) = r.process(if i == 0 { 1.0 } else { 0.0 });
            let e = l.abs() + rr.abs();
            if i < 48_000 {
                early = early.max(e);
            } else if i > 48_000 * 5 {
                late = late.max(e);
            }
        }
        assert!(early > 1e-4);
        assert!(late < early * 0.01, "late {late} early {early}");
    }
}
