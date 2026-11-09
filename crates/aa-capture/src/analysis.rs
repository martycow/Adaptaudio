//! Measuring and cleaning up recorded audio. Pure functions on sample slices.
//!
//! Levels are in dBFS: 0 dB is full scale (a sample of ±1.0). Functions that take a position
//! expect mono samples; use [`to_mono`] on interleaved recordings first. Level functions
//! ([`peak_db`], [`rms_db`]) work on mono and interleaved buffers alike.

use std::ops::Range;

/// The level reported for silence (instead of minus infinity).
pub const SILENCE_DB: f32 = -120.0;

pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Level in dB of a linear gain, never below [`SILENCE_DB`].
pub fn gain_to_db(gain: f32) -> f32 {
    if gain <= 0.0 || !gain.is_finite() {
        SILENCE_DB
    } else {
        (20.0 * gain.log10()).max(SILENCE_DB)
    }
}

/// Mixes interleaved frames down to mono by averaging the channels.
pub fn to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    let scale = 1.0 / channels as f32;
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() * scale)
        .collect()
}

/// Loudest sample, in dB.
pub fn peak_db(samples: &[f32]) -> f32 {
    gain_to_db(peak(samples))
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// Average power, in dB.
pub fn rms_db(samples: &[f32]) -> f32 {
    gain_to_db(rms(samples))
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

/// Index of the first sample at or above `threshold_db`.
pub fn detect_onset(samples: &[f32], threshold_db: f32) -> Option<usize> {
    let threshold = db_to_gain(threshold_db);
    samples.iter().position(|s| s.abs() >= threshold)
}

/// Kept before the first loud sample, so the attack is not clipped.
const TRIM_LEAD_MS: u32 = 1;
/// Kept after the last loud sample, so the tail does not end abruptly.
const TRIM_TAIL_MS: u32 = 5;

/// The part of `samples` between the first and last sample at or above `threshold_db`, with a
/// millisecond of room before and a few after. Empty (`0..0`) when everything is quieter.
pub fn trim_silence(samples: &[f32], sample_rate: u32, threshold_db: f32) -> Range<usize> {
    let threshold = db_to_gain(threshold_db);
    let Some(first) = samples.iter().position(|s| s.abs() >= threshold) else {
        return 0..0;
    };
    let last = samples
        .iter()
        .rposition(|s| s.abs() >= threshold)
        .unwrap_or(first);
    let lead = ms(TRIM_LEAD_MS, sample_rate);
    let tail = ms(TRIM_TAIL_MS, sample_rate);
    first.saturating_sub(lead)..(last + 1 + tail).min(samples.len())
}

fn ms(ms: u32, sample_rate: u32) -> usize {
    (u64::from(ms) * u64::from(sample_rate) / 1000) as usize
}

/// Scales `samples` so the loudest one sits at `target_db`. Returns the gain used
/// (1.0 for silence, which is left alone).
pub fn normalize(samples: &mut [f32], target_db: f32) -> f32 {
    let p = peak(samples);
    if p <= 0.0 {
        return 1.0;
    }
    let gain = db_to_gain(target_db) / p;
    for s in samples.iter_mut() {
        *s *= gain;
    }
    gain
}

/// Linear fade-in over the first `fade_in_ms` and fade-out over the last `fade_out_ms`, so
/// cut points do not click.
pub fn apply_fades(samples: &mut [f32], sample_rate: u32, fade_in_ms: f32, fade_out_ms: f32) {
    let len = samples.len();
    let to_samples = |ms: f32| ((ms.max(0.0) * sample_rate as f32 / 1000.0) as usize).min(len);
    let fade_in = to_samples(fade_in_ms);
    for (i, s) in samples[..fade_in].iter_mut().enumerate() {
        *s *= i as f32 / fade_in as f32;
    }
    let fade_out = to_samples(fade_out_ms);
    for (i, s) in samples[len - fade_out..].iter_mut().rev().enumerate() {
        *s *= i as f32 / fade_out as f32;
    }
}

/// Window of the loudness curve used to find the sustain.
const ENVELOPE_WINDOW_MS: u32 = 10;
/// The sustain is where the level stays within this much of the peak.
const SUSTAIN_RANGE_DB: f32 = 6.0;
const MIN_SAMPLE_MS: u32 = 300;
const MIN_LOOP_MS: u32 = 50;

/// A loop `[start, end)` inside the sustain, so a short sample can be held for any length:
/// playback jumps from `end - 1` back to `start`.
///
/// Basic method: find where the level stays near its peak after the attack, put the start on
/// an upward zero crossing there, and pick the end among upward zero crossings near the end
/// of the sustain whose slope best matches the start. `None` for short, silent or very
/// percussive sounds without a sustain.
pub fn find_loop_points(samples: &[f32], sample_rate: u32) -> Option<[u32; 2]> {
    let window = ms(ENVELOPE_WINDOW_MS, sample_rate).max(1);
    if samples.len() < ms(MIN_SAMPLE_MS, sample_rate).max(window * 3) {
        return None;
    }
    let envelope: Vec<f32> = samples.chunks(window).map(|w| gain_to_db(rms(w))).collect();
    let peak_level = envelope.iter().copied().fold(SILENCE_DB, f32::max);
    if peak_level <= SILENCE_DB + 1.0 {
        return None;
    }
    // On a flat sustain every window is "the peak"; take the first one that gets close.
    let peak_window = envelope
        .iter()
        .position(|&level| level >= peak_level - 1.0)?;
    // Skip the attack and the first part of the decay.
    let start_window = (peak_window + 5).max(envelope.len() * 15 / 100);
    let end_window = envelope
        .iter()
        .rposition(|&level| level >= peak_level - SUSTAIN_RANGE_DB)?;
    if end_window <= start_window {
        return None;
    }
    let region = start_window * window..end_window * window;
    let min_loop = ms(MIN_LOOP_MS, sample_rate).max(2);
    if region.len() < min_loop * 2 {
        return None;
    }

    let is_upward_crossing = |i: usize| samples[i - 1] < 0.0 && samples[i] >= 0.0;
    let slope = |i: usize| samples[i] - samples[i - 1];
    let start = (region.start.max(1)..region.end).find(|&i| is_upward_crossing(i))?;
    let start_slope = slope(start);

    let search_from = (region.end - region.len() / 4).max(start + min_loop);
    let end = (search_from..region.end)
        .filter(|&i| is_upward_crossing(i))
        .min_by(|&a, &b| {
            (slope(a) - start_slope)
                .abs()
                .total_cmp(&(slope(b) - start_slope).abs())
        })?;
    Some([u32::try_from(start).ok()?, u32::try_from(end).ok()?])
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::f32::consts::TAU;

    /// A test tone: `freq` Hz with a 20 ms attack, a flat sustain and a 200 ms release.
    pub(crate) fn tone(freq: f32, seconds: f32, amplitude: f32, sample_rate: u32) -> Vec<f32> {
        let n = (seconds * sample_rate as f32) as usize;
        let attack = 0.02 * sample_rate as f32;
        let release = 0.2 * sample_rate as f32;
        (0..n)
            .map(|i| {
                let t = i as f32;
                let env = (t / attack).min(1.0).min((n as f32 - t) / release);
                amplitude * env * (TAU * freq * t / sample_rate as f32).sin()
            })
            .collect()
    }

    #[test]
    fn levels_in_db() {
        assert!((peak_db(&[0.0, -0.5, 0.25]) - (-6.0206)).abs() < 1e-3);
        assert!((rms_db(&[1.0, -1.0, 1.0, -1.0]) - 0.0).abs() < 1e-6);
        assert_eq!(peak_db(&[]), SILENCE_DB);
        assert_eq!(rms_db(&[0.0; 10]), SILENCE_DB);
        assert!((db_to_gain(-6.0206) - 0.5).abs() < 1e-4);
        assert_eq!(gain_to_db(f32::NAN), SILENCE_DB);
    }

    #[test]
    fn sine_rms_is_3_db_below_peak() {
        let sine = tone(1000.0, 1.0, 1.0, 48_000);
        assert!((rms_db(&sine[2000..40000]) - (-3.0103)).abs() < 0.05);
    }

    #[test]
    fn stereo_mixes_down_to_mono() {
        assert_eq!(to_mono(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(to_mono(&[0.1, 0.2], 1), vec![0.1, 0.2]);
    }

    #[test]
    fn onset_is_the_first_loud_sample() {
        let mut samples = vec![0.001; 100];
        samples[42] = 0.5;
        samples[70] = 0.9;
        assert_eq!(detect_onset(&samples, -20.0), Some(42));
        assert_eq!(detect_onset(&samples, 0.0), None);
    }

    #[test]
    fn trim_keeps_the_sound_and_a_little_room() {
        let sr = 1000; // 1 sample per millisecond keeps the numbers readable.
        let mut samples = vec![0.0; 1000];
        for s in &mut samples[200..500] {
            *s = 0.5;
        }
        assert_eq!(trim_silence(&samples, sr, -40.0), 199..505);
        assert_eq!(trim_silence(&[0.0001; 50], sr, -40.0), 0..0);
        // Room is clamped at the edges.
        assert_eq!(trim_silence(&[0.5; 3], sr, -40.0), 0..3);
    }

    #[test]
    fn normalize_sets_the_peak() {
        let mut samples = vec![0.1, -0.25, 0.2];
        let gain = normalize(&mut samples, -6.0206);
        assert!((gain - 2.0).abs() < 1e-3);
        assert!((samples[1] + 0.5).abs() < 1e-4);
        let mut silent = vec![0.0; 4];
        assert_eq!(normalize(&mut silent, 0.0), 1.0);
    }

    #[test]
    fn fades_start_and_end_at_zero() {
        let mut samples = vec![1.0; 100];
        apply_fades(&mut samples, 1000, 10.0, 20.0);
        assert_eq!(samples[0], 0.0);
        assert!((samples[5] - 0.5).abs() < 1e-6);
        assert_eq!(samples[50], 1.0);
        assert_eq!(samples[99], 0.0);
        assert!((samples[89] - 0.5).abs() < 1e-6);
        // Fades longer than the sample do not panic.
        apply_fades(&mut [1.0; 3], 1000, 100.0, 100.0);
        apply_fades(&mut [], 1000, 1.0, 1.0);
    }

    #[test]
    fn loop_points_sit_on_matching_zero_crossings_in_the_sustain() {
        let sr = 48_000;
        let samples = tone(220.0, 1.5, 0.5, sr);
        let [start, end] = find_loop_points(&samples, sr).unwrap();
        let (start, end) = (start as usize, end as usize);
        assert!(start > (0.02 * sr as f32) as usize, "after the attack");
        assert!(
            end < samples.len() - (0.2 * sr as f32) as usize + (0.05 * sr as f32) as usize,
            "before the release"
        );
        assert!(end - start >= sr as usize / 20, "at least 50 ms long");
        assert!(samples[start - 1] < 0.0 && samples[start] >= 0.0);
        assert!(samples[end - 1] < 0.0 && samples[end] >= 0.0);
        // Jumping from end-1 to start continues the wave smoothly.
        assert!((samples[end] - samples[start]).abs() < 0.02);
    }

    #[test]
    fn no_loop_for_silence_or_short_sounds() {
        assert_eq!(find_loop_points(&vec![0.0; 48_000], 48_000), None);
        assert_eq!(
            find_loop_points(&tone(220.0, 0.1, 0.5, 48_000), 48_000),
            None
        );
        assert_eq!(find_loop_points(&[], 48_000), None);
    }
}
