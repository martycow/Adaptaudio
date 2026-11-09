//! Small DSP building blocks. Every type here is plain data with no allocation.

use aa_core::{Adsr, Waveform};
use std::f32::consts::{PI, TAU};

pub fn db_to_gain(db: f32) -> f32 {
    if db <= -96.0 {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

pub fn gain_to_db(gain: f32) -> f32 {
    if gain <= 1e-5 {
        -100.0
    } else {
        20.0 * gain.log10()
    }
}

/// xorshift64*: fast, deterministic, good enough for noise and note probability.
#[derive(Clone, Copy, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in 0..1.
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in -1..1.
    pub fn bipolar(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }
}

/// PolyBLEP residual that rounds off a hard edge to cut aliasing.
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Osc {
    pub phase: f32,
}

impl Osc {
    /// One sample at `freq` Hz. `pw` is the square's pulse width.
    pub fn next(
        &mut self,
        wave: Waveform,
        freq: f32,
        sample_rate: f32,
        pw: f32,
        rng: &mut Rng,
    ) -> f32 {
        let dt = (freq / sample_rate).clamp(0.0, 0.49);
        let t = self.phase;
        let out = match wave {
            Waveform::Sine => (t * TAU).sin(),
            Waveform::Triangle => 1.0 - 4.0 * (t - 0.5).abs(),
            Waveform::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
            Waveform::Square => {
                let pw = pw.clamp(0.05, 0.95);
                let naive = if t < pw { 1.0 } else { -1.0 };
                naive + poly_blep(t, dt) - poly_blep((t - pw).rem_euclid(1.0), dt)
            }
            Waveform::Noise => rng.bipolar(),
        };
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        out
    }
}

/// State-variable filter (Simper's trapezoidal form): stable under fast modulation.
#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct SvfCoeffs {
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl SvfCoeffs {
    pub fn new(cutoff_hz: f32, resonance: f32, sample_rate: f32) -> Self {
        let fc = cutoff_hz.clamp(20.0, sample_rate * 0.45);
        let g = (PI * fc / sample_rate).tan();
        let k = 2.0 - 1.96 * resonance.clamp(0.0, 1.0);
        let a1 = 1.0 / (1.0 + g * (g + k));
        SvfCoeffs {
            a1,
            a2: g * a1,
            a3: g * g * a1,
            k,
        }
    }
}

pub struct SvfOut {
    pub low: f32,
    pub band: f32,
    pub high: f32,
}

impl Svf {
    pub fn process(&mut self, x: f32, c: &SvfCoeffs) -> SvfOut {
        let v3 = x - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        SvfOut {
            low: v2,
            band: v1,
            high: x - c.k * v1 - v2,
        }
    }

    pub fn reset(&mut self) {
        *self = Svf::default();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Stage {
    #[default]
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// ADSR envelope: linear attack, exponential decay and release.
#[derive(Clone, Copy, Debug, Default)]
pub struct Env {
    pub stage: Stage,
    pub level: f32,
}

/// Per-sample coefficient that covers ~99% of the distance in `seconds`.
fn exp_coeff(seconds: f32, sample_rate: f32) -> f32 {
    1.0 - (-4.6 / (seconds.max(1e-4) * sample_rate)).exp()
}

impl Env {
    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn gate_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    pub fn next(&mut self, adsr: &Adsr, sample_rate: f32) -> f32 {
        match self.stage {
            Stage::Idle => self.level = 0.0,
            Stage::Attack => {
                self.level += 1.0 / (adsr.attack.max(1e-4) * sample_rate);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                let s = adsr.sustain.clamp(0.0, 1.0);
                self.level += (s - self.level) * exp_coeff(adsr.decay, sample_rate);
                if (self.level - s).abs() < 1e-4 {
                    self.level = s;
                    self.stage = if s <= 1e-4 {
                        Stage::Idle
                    } else {
                        Stage::Sustain
                    };
                }
            }
            Stage::Sustain => self.level = adsr.sustain.clamp(0.0, 1.0),
            Stage::Release => {
                self.level -= self.level * exp_coeff(adsr.release, sample_rate);
                if self.level < 1e-4 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

/// One-pole smoother for control values, against zipper noise.
#[derive(Clone, Copy, Debug)]
pub struct Smoothed {
    pub value: f32,
}

impl Smoothed {
    pub fn new(value: f32) -> Self {
        Smoothed { value }
    }

    /// Moves towards `target` with coefficient `k` (0..1 per call).
    pub fn toward(&mut self, target: f32, k: f32) -> f32 {
        self.value += (target - self.value) * k;
        self.value
    }
}

/// Smooth saturation that keeps unity gain for small signals.
pub fn saturate(x: f32, drive: f32) -> f32 {
    if drive <= 1e-4 {
        return x;
    }
    let g = 1.0 + drive * 8.0;
    (x * g).tanh() / g.tanh()
}

/// Equal-power pan: returns (left, right) gains for pan -1..1.
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let a = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    (a.cos(), a.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decibels_round_trip() {
        assert!((db_to_gain(-6.0) - 0.501).abs() < 1e-3);
        assert!((gain_to_db(db_to_gain(-12.0)) + 12.0).abs() < 1e-3);
        assert_eq!(db_to_gain(-120.0), 0.0);
    }

    #[test]
    fn oscillators_stay_in_range() {
        let mut rng = Rng::new(7);
        for wave in Waveform::ALL {
            let mut o = Osc::default();
            for _ in 0..48_000 {
                let s = o.next(wave, 440.0, 48_000.0, 0.3, &mut rng);
                assert!(s.abs() <= 1.3, "{wave:?} {s}");
            }
        }
    }

    #[test]
    fn a_lowpass_attenuates_highs() {
        let c = SvfCoeffs::new(200.0, 0.0, 48_000.0);
        let mut f = Svf::default();
        let mut o = Osc::default();
        let mut rng = Rng::new(1);
        let mut peak: f32 = 0.0;
        for i in 0..48_000 {
            let y = f
                .process(o.next(Waveform::Sine, 8_000.0, 48_000.0, 0.5, &mut rng), &c)
                .low;
            if i > 1_000 {
                peak = peak.max(y.abs());
            }
        }
        assert!(peak < 0.01, "{peak}");
    }

    #[test]
    fn the_envelope_finishes_its_release() {
        let adsr = Adsr::new(0.01, 0.1, 0.5, 0.05);
        let mut e = Env::default();
        e.gate_on();
        for _ in 0..4_800 {
            e.next(&adsr, 48_000.0);
        }
        assert!((e.level - 0.5).abs() < 0.01);
        e.gate_off();
        for _ in 0..9_600 {
            e.next(&adsr, 48_000.0);
        }
        assert!(e.is_idle());
    }

    #[test]
    fn panning_keeps_power() {
        for p in [-1.0, -0.3, 0.0, 0.5, 1.0] {
            let (l, r) = pan_gains(p);
            assert!((l * l + r * r - 1.0).abs() < 1e-5);
        }
    }
}
