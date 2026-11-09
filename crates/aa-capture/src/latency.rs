//! Latency calibration: how long after we send something does it come back on the input.
//!
//! Send a click (or a MIDI note) at a known sample position of the recording, find where it
//! shows up, and the difference is the round trip. External tracks are then shifted earlier by
//! that much so hardware and built-in tracks line up.

use crate::analysis::{SILENCE_DB, detect_onset, peak_db};

const CLICK_MS: f32 = 10.0;
const CLICK_FREQ_HZ: f32 = 1000.0;
const CLICK_DECAY_MS: f32 = 2.0;
const CLICK_LEVEL: f32 = 0.9;

/// A 10 ms click to play out of the speakers or into an input: a decaying 1 kHz cosine that
/// starts at full level on its very first sample, so its onset is sharp.
pub fn click_signal(sample_rate: u32) -> Vec<f32> {
    let sr = sample_rate.max(1) as f32;
    let len = (CLICK_MS / 1000.0 * sr) as usize;
    let decay = CLICK_DECAY_MS / 1000.0 * sr;
    (0..len)
        .map(|i| {
            let t = i as f32;
            CLICK_LEVEL
                * (std::f32::consts::TAU * CLICK_FREQ_HZ * t / sr).cos()
                * (-t / decay).exp()
        })
        .collect()
}

/// The returning sound must stand this far above the noise before it.
const MIN_SIGNAL_TO_NOISE_DB: f32 = 12.0;
/// The onset threshold sits at least this far above the loudest noise sample...
const THRESHOLD_ABOVE_NOISE_DB: f32 = 10.0;
/// ...and no lower than this far below the returning sound's peak.
const THRESHOLD_BELOW_PEAK_DB: f32 = 30.0;
/// How much noise to look at when nothing was recorded before the click went out.
const NOISE_PROBE_MS: u32 = 5;

/// Milliseconds between `sent_at_sample` (where the click or note went out, as a position in
/// `recording`) and its onset in `recording`. `None` if nothing clearly louder than the noise
/// came back.
///
/// The noise level is taken from the recording before `sent_at_sample`; the onset threshold
/// is set between that noise and the returning peak, low enough to catch slow attacks early.
pub fn measure_latency(sent_at_sample: usize, recording: &[f32], sample_rate: u32) -> Option<f32> {
    if sent_at_sample >= recording.len() || sample_rate == 0 {
        return None;
    }
    let before = &recording[..sent_at_sample];
    let after = &recording[sent_at_sample..];
    let probe = (NOISE_PROBE_MS * sample_rate / 1000) as usize;
    let noise_db = if before.len() >= probe {
        peak_db(before)
    } else {
        SILENCE_DB
    };
    let signal_db = peak_db(after);
    if signal_db - noise_db < MIN_SIGNAL_TO_NOISE_DB {
        return None;
    }
    let threshold = (noise_db + THRESHOLD_ABOVE_NOISE_DB).max(signal_db - THRESHOLD_BELOW_PEAK_DB);
    let onset = detect_onset(after, threshold)?;
    Some(onset as f32 * 1000.0 / sample_rate as f32)
}

/// A latency in milliseconds as a whole number of samples.
pub fn latency_to_samples(latency_ms: f32, sample_rate: u32) -> usize {
    (latency_ms.max(0.0) * sample_rate as f32 / 1000.0).round() as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::db_to_gain;

    fn noise(len: usize, level_db: f32, mut seed: u32) -> Vec<f32> {
        let level = db_to_gain(level_db);
        (0..len)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((seed >> 8) as f32 / (1 << 24) as f32 - 0.5) * 2.0 * level
            })
            .collect()
    }

    #[test]
    fn click_starts_loud_and_dies_away() {
        let click = click_signal(48_000);
        assert_eq!(click.len(), 480);
        assert!((click[0] - 0.9).abs() < 1e-6);
        assert!(click[479].abs() < 0.01, "below -40 dB at the end");
    }

    #[test]
    fn finds_a_delayed_click_in_noise() {
        let sr = 48_000;
        let sent_at = 4800; // the click went out 100 ms into the recording
        let delay = 617; // 12.854 ms round trip
        let mut recording = noise(sr as usize, -50.0, 7);
        for (i, s) in click_signal(sr).iter().enumerate() {
            // The way back makes it quieter.
            recording[sent_at + delay + i] += s * 0.3;
        }
        let latency = measure_latency(sent_at, &recording, sr).unwrap();
        let expected = delay as f32 * 1000.0 / sr as f32;
        assert!(
            (latency - expected).abs() < 0.05,
            "measured {latency} ms, expected {expected} ms"
        );
        assert_eq!(latency_to_samples(latency, sr), delay);
    }

    #[test]
    fn finds_a_slow_synth_attack_early() {
        let sr = 48_000;
        let sent_at = 4800;
        let delay = 960; // 20 ms
        let mut recording = noise(sr as usize, -60.0, 3);
        // A 30 ms linear attack of a 110 Hz tone.
        for i in 0..sr as usize / 2 {
            let env = (i as f32 / (0.03 * sr as f32)).min(1.0);
            recording[sent_at + delay + i] +=
                0.5 * env * (std::f32::consts::TAU * 110.0 * i as f32 / sr as f32).sin();
        }
        let latency = measure_latency(sent_at, &recording, sr).unwrap();
        assert!((20.0..23.0).contains(&latency), "measured {latency} ms");
    }

    #[test]
    fn no_answer_means_no_latency() {
        let sr = 48_000;
        let recording = noise(sr as usize, -50.0, 11);
        assert_eq!(measure_latency(4800, &recording, sr), None);
        assert_eq!(
            measure_latency(10 * sr as usize, &recording, sr),
            None,
            "sent after the end"
        );
        assert_eq!(measure_latency(0, &[], sr), None);
    }

    #[test]
    fn works_without_a_noise_reference() {
        let sr = 48_000;
        let mut recording = vec![0.0; 2000];
        recording[300] = 0.5;
        let latency = measure_latency(0, &recording, sr).unwrap();
        assert!((latency - 6.25).abs() < 1e-3);
    }
}
