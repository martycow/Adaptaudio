//! One sounding note: the built-in synth or a sampler zone.

use crate::dsp::{Env, Osc, Rng, Svf, SvfCoeffs};
use crate::samples::SampleData;
use aa_core::music::midi_to_hz;
use aa_core::{FilterMode, LfoTarget, LockTarget, Note, SamplerInstrument, SynthPatch};

/// Controls recomputed every `CONTROL_RATE` samples (pitch, filter).
const CONTROL_RATE: usize = 16;

/// Which player started the voice: a section deck, or the stinger/preview player.
pub const DECK_ONESHOT: u8 = 2;

/// What the track asks of its voices this block.
#[derive(Clone, Copy, Debug)]
pub struct VoiceCtx {
    pub sample_rate: f32,
    pub cutoff_mul: f32,
    pub detune_cents: f32,
    /// Pitch offset in cents from tape wobble, this block.
    pub wobble_cents: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Voice {
    pub active: bool,
    pub deck: u8,
    pub pitch: u8,
    pub age: u64,
    /// Samples to wait before the note starts (sample-accurate triggering inside a block).
    delay: usize,
    /// Samples until the gate closes.
    gate: usize,
    velocity: f32,
    note: f32,
    glide_from: f32,
    glide_left: f32,
    glide_total: f32,
    time: f32,
    osc: [Osc; 2],
    filter: Svf,
    amp: Env,
    fenv: Env,
    lfo_phase: f32,
    // Parameter locks.
    cutoff_lock: Option<f32>,
    res_lock: Option<f32>,
    decay_lock: Option<f32>,
    transpose: f32,
    pw_lock: Option<f32>,
    // Sampler state.
    zone: Option<usize>,
    sample_pos: f64,
}

/// Everything needed to start a voice.
#[derive(Clone, Copy, Debug)]
pub struct Trigger {
    pub pitch: u8,
    pub velocity: f32,
    pub gate_samples: usize,
    pub delay_samples: usize,
    pub deck: u8,
    pub glide_from: Option<f32>,
    pub glide_seconds: f32,
}

impl Voice {
    pub fn start(&mut self, t: Trigger, note: Option<&Note>, age: u64) {
        let mut v = Voice {
            active: true,
            deck: t.deck,
            pitch: t.pitch,
            age,
            delay: t.delay_samples,
            gate: t.gate_samples.max(1),
            velocity: t.velocity.clamp(0.0, 1.0),
            note: f32::from(t.pitch),
            glide_from: t.glide_from.unwrap_or(f32::from(t.pitch)),
            glide_left: if t.glide_from.is_some() {
                t.glide_seconds
            } else {
                0.0
            },
            glide_total: t.glide_seconds.max(1e-4),
            ..Voice::default()
        };
        // Keep the oscillators' phase so retriggers don't click; reset the filter for a clean attack.
        v.osc = self.osc;
        if let Some(n) = note {
            for lock in &n.locks {
                match lock.target {
                    LockTarget::Cutoff => v.cutoff_lock = Some(lock.value),
                    LockTarget::Resonance => v.res_lock = Some(lock.value),
                    LockTarget::Decay => v.decay_lock = Some(lock.value),
                    LockTarget::Transpose => v.transpose = lock.value,
                    LockTarget::PulseWidth => v.pw_lock = Some(lock.value),
                }
            }
        }
        v.amp.gate_on();
        v.fenv.gate_on();
        *self = v;
    }

    /// Lets the note ring out through its release.
    pub fn release(&mut self) {
        self.gate = 0;
        self.amp.gate_off();
        self.fenv.gate_off();
    }

    /// Fades the note out quickly (voice stealing, stop).
    pub fn kill(&mut self) {
        self.active = false;
    }

    /// Renders the built-in synth into `out` (mono, added), scaled by `gain` per sample.
    pub fn render_synth(
        &mut self,
        patch: &SynthPatch,
        ctx: &VoiceCtx,
        rng: &mut Rng,
        out: &mut [f32],
        gains: &[f32],
    ) {
        let sr = ctx.sample_rate;
        let mut amp_adsr = patch.amp_env;
        if let Some(d) = self.decay_lock {
            amp_adsr.decay = d.max(0.005);
        }
        let mut coeffs = SvfCoeffs::new(patch.filter.cutoff_hz, patch.filter.resonance, sr);
        let mut f1 = 0.0;
        let mut f2 = 0.0;
        let mut pw = patch.osc[0].pulse_width;
        let n = out.len();
        let mut i = 0;
        while i < n {
            if self.delay > 0 {
                let skip = self.delay.min(n - i);
                self.delay -= skip;
                i += skip;
                continue;
            }
            if !self.active {
                break;
            }
            let chunk_end = (i + CONTROL_RATE).min(n);
            let chunk_dt = (chunk_end - i) as f32 / sr;
            // Pitch: glide, pitch envelope, LFO, detune, wobble, transpose lock.
            let base = if self.glide_left > 0.0 {
                let t = 1.0 - self.glide_left / self.glide_total;
                self.glide_left -= chunk_dt;
                self.glide_from + (self.note - self.glide_from) * t.clamp(0.0, 1.0)
            } else {
                self.note
            };
            let lfo = (self.lfo_phase * std::f32::consts::TAU).sin() * patch.lfo.depth;
            self.lfo_phase = (self.lfo_phase + patch.lfo.rate_hz * chunk_dt).fract();
            let pitch_env =
                patch.pitch_env.semitones * (-self.time / patch.pitch_env.decay.max(1e-3)).exp();
            let lfo_pitch = if patch.lfo.target == LfoTarget::Pitch {
                lfo * 2.0
            } else {
                0.0
            };
            let pitch = base + self.transpose + pitch_env + lfo_pitch + ctx.wobble_cents / 100.0;
            let spread = ctx.detune_cents / 100.0;
            f1 = midi_to_hz(
                pitch + f32::from(patch.osc[0].octave) * 12.0 + patch.osc[0].detune_cents / 100.0
                    - spread * 0.5,
            );
            f2 = midi_to_hz(
                pitch
                    + f32::from(patch.osc[1].octave) * 12.0
                    + patch.osc[1].detune_cents / 100.0
                    + spread * 0.5,
            );
            pw = self.pw_lock.unwrap_or(patch.osc[0].pulse_width);
            if patch.lfo.target == LfoTarget::PulseWidth {
                pw += lfo * 0.4;
            }
            // Filter: base cutoff × game mod × envelope × LFO.
            let env_oct = self.fenv.level * patch.filter.env_octaves;
            let lfo_oct = if patch.lfo.target == LfoTarget::Cutoff {
                lfo * 2.0
            } else {
                0.0
            };
            let mul = ctx.cutoff_mul.clamp(0.0, 1.0);
            let game_oct = -4.0 * (1.0 - mul);
            let cutoff = self.cutoff_lock.unwrap_or(patch.filter.cutoff_hz)
                * 2f32.powf(env_oct + lfo_oct + game_oct);
            coeffs = SvfCoeffs::new(cutoff, self.res_lock.unwrap_or(patch.filter.resonance), sr);
            let lfo_amp = if patch.lfo.target == LfoTarget::Volume {
                1.0 - patch.lfo.depth * 0.5 * (1.0 + lfo)
            } else {
                1.0
            };

            for j in i..chunk_end {
                if self.gate > 0 {
                    self.gate -= 1;
                    if self.gate == 0 {
                        self.amp.gate_off();
                        self.fenv.gate_off();
                    }
                }
                let a = self.amp.next(&amp_adsr, sr);
                self.fenv.next(&patch.filter_env, sr);
                let s1 = self.osc[0].next(patch.osc[0].wave, f1, sr, pw, rng) * patch.osc[0].level;
                let s2 = self.osc[1].next(patch.osc[1].wave, f2, sr, patch.osc[1].pulse_width, rng)
                    * patch.osc[1].level;
                let noise = rng.bipolar() * patch.noise;
                let raw = s1 + s2 + noise;
                let filtered = self.filter.process(raw, &coeffs);
                let y = match patch.filter.mode {
                    FilterMode::LowPass => filtered.low,
                    FilterMode::HighPass => filtered.high,
                    FilterMode::BandPass => filtered.band * 1.5,
                };
                out[j] += y * a * self.velocity * lfo_amp * gains[j];
                if self.amp.is_idle() {
                    self.active = false;
                    break;
                }
            }
            self.time += chunk_dt;
            i = chunk_end;
        }
        let _ = (f1, f2, pw, coeffs);
    }

    /// Renders a sampler zone into `out` (mono, added).
    #[allow(clippy::too_many_arguments)]
    pub fn render_sampler(
        &mut self,
        inst: &SamplerInstrument,
        data: &SampleData,
        root: u8,
        ctx: &VoiceCtx,
        out: &mut [f32],
        gains: &[f32],
        loop_points: Option<[u32; 2]>,
    ) {
        let sr = ctx.sample_rate;
        let pitch =
            f32::from(self.pitch) + self.transpose + ctx.wobble_cents / 100.0 - f32::from(root);
        let rate = f64::from(2f32.powf(pitch / 12.0) * data.sample_rate / sr);
        let len = data.frames.len();
        for (j, o) in out.iter_mut().enumerate() {
            if self.delay > 0 {
                self.delay -= 1;
                continue;
            }
            if !self.active {
                break;
            }
            if self.gate > 0 {
                self.gate -= 1;
                if self.gate == 0 {
                    self.amp.gate_off();
                }
            }
            let a = self.amp.next(&inst.amp_env, sr);
            let idx = self.sample_pos as usize;
            if idx + 1 >= len {
                self.active = false;
                break;
            }
            let frac = (self.sample_pos - idx as f64) as f32;
            let s = data.frames[idx] * (1.0 - frac) + data.frames[idx + 1] * frac;
            *o += s * a * self.velocity * gains[j];
            self.sample_pos += rate;
            if let Some([start, end]) = loop_points {
                let (start, end) = (f64::from(start), f64::from(end));
                if end > start + 1.0 && self.sample_pos >= end && self.gate > 0 {
                    self.sample_pos -= end - start;
                }
            }
            if self.amp.is_idle() {
                self.active = false;
                break;
            }
        }
    }

    pub fn set_zone(&mut self, zone: Option<usize>) {
        self.zone = zone;
        self.sample_pos = 0.0;
    }

    pub fn zone(&self) -> Option<usize> {
        self.zone
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aa_core::TrackRole;

    fn trigger(pitch: u8) -> Trigger {
        Trigger {
            pitch,
            velocity: 1.0,
            gate_samples: 4_800,
            delay_samples: 0,
            deck: 0,
            glide_from: None,
            glide_seconds: 0.0,
        }
    }

    fn ctx() -> VoiceCtx {
        VoiceCtx {
            sample_rate: 48_000.0,
            cutoff_mul: 1.0,
            detune_cents: 0.0,
            wobble_cents: 0.0,
        }
    }

    #[test]
    fn every_preset_makes_sound_and_then_stops() {
        for role in TrackRole::ALL {
            let patch = SynthPatch::preset(role);
            let mut v = Voice::default();
            v.start(trigger(role.default_note()), None, 0);
            let mut rng = Rng::new(3);
            let gains = vec![1.0; 512];
            let mut peak: f32 = 0.0;
            for _ in 0..(48_000 * 8 / 512) {
                let mut out = vec![0.0; 512];
                v.render_synth(&patch, &ctx(), &mut rng, &mut out, &gains);
                peak = peak.max(out.iter().fold(0.0f32, |m, s| m.max(s.abs())));
                assert!(out.iter().all(|s| s.is_finite()));
            }
            assert!(peak > 0.01, "{role:?} is silent");
            assert!(!v.active, "{role:?} never ends");
        }
    }

    #[test]
    fn a_delayed_trigger_starts_late_in_the_block() {
        let patch = SynthPatch::preset(TrackRole::Lead);
        let mut v = Voice::default();
        v.start(
            Trigger {
                delay_samples: 300,
                ..trigger(69)
            },
            None,
            0,
        );
        let mut out = vec![0.0; 512];
        v.render_synth(&patch, &ctx(), &mut Rng::new(1), &mut out, &[1.0; 512]);
        assert!(out[..300].iter().all(|s| *s == 0.0));
        assert!(out[300..].iter().any(|s| *s != 0.0));
    }
}
