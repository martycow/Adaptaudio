//! Pure conversions between Adaptaudio units and FMOD Studio units. No I/O, all tested.

use aa_core::{CurveTarget, Quantize, ResponseCurve};
use serde::{Deserialize, Serialize};

/// The floor FMOD treats as silence (its volume faders stop at −80 dB).
pub const SILENCE_DB: f32 = -80.0;
/// Cutoff of a fully open lowpass, used when the instrument has no filter of its own.
pub const OPEN_CUTOFF_HZ: f32 = 20_000.0;
/// `CurveTarget::FilterCutoff` spans 1/16× .. 1× of the base cutoff: four octaves.
const CUTOFF_RANGE_OCTAVES: f32 = 4.0;

/// Seconds in one bar.
pub fn seconds_per_bar(tempo_bpm: f32, beats_per_bar: u8) -> f64 {
    60.0 / f64::from(tempo_bpm.max(1.0)) * f64::from(beats_per_bar.max(1))
}

/// Frames (samples per channel) in `bars` bars.
///
/// Rounding: the exact length of the whole span is rounded to the nearest frame once. Lengths are
/// never built bar by bar, so the error stays under half a frame and does not accumulate. At
/// 120 BPM in 4/4 and 48 kHz a bar is exactly 96 000 frames and there is no rounding at all.
pub fn bars_to_frames(bars: u32, tempo_bpm: f32, beats_per_bar: u8, sample_rate: u32) -> usize {
    seconds_to_frames(
        f64::from(bars) * seconds_per_bar(tempo_bpm, beats_per_bar),
        sample_rate,
    )
}

/// Frames in `seconds`, rounded to the nearest frame.
pub fn seconds_to_frames(seconds: f64, sample_rate: u32) -> usize {
    (seconds.max(0.0) * f64::from(sample_rate)).round() as usize
}

/// A length in beats as milliseconds at the given tempo.
pub fn beats_to_ms(beats: f32, tempo_bpm: f32) -> f32 {
    beats.max(0.0) * 60_000.0 / tempo_bpm.max(1.0)
}

/// Linear gain to decibels (`20 · log10`), clamped to [`SILENCE_DB`].
pub fn gain_to_db(gain: f32) -> f32 {
    if gain.is_nan() || gain <= 0.0 {
        return SILENCE_DB;
    }
    (20.0 * gain.log10()).max(SILENCE_DB)
}

/// FMOD seek speed for a parameter that should travel its whole range in `slew_seconds`.
///
/// FMOD's seek speed is in parameter units per second, so for the 0..1 range it is
/// `1 / slew_seconds`. A slew of zero (or less) means "jump at once", which FMOD spells as a seek
/// speed of 0 (seeking off).
pub fn seek_speed(slew_seconds: f32, range: f32) -> f32 {
    if !slew_seconds.is_finite() || slew_seconds <= 0.0 {
        0.0
    } else {
        range.abs() / slew_seconds
    }
}

/// Bakes Influence into a curve value: `final = neutral + influence · (value − neutral)`.
pub fn bake_influence(neutral: f32, influence: f32, value: f32) -> f32 {
    neutral + influence.clamp(0.0, 1.0) * (value - neutral)
}

/// Samples a response curve for FMOD automation with Influence baked in.
///
/// The curve is piecewise linear, so sampling it at its own points plus the two ends of the 0..1
/// range reproduces it exactly. Returns `[parameter value, baked output]` pairs in Adaptaudio
/// units, sorted by parameter value.
pub fn bake_curve(curve: &ResponseCurve, influence: f32) -> Vec<[f32; 2]> {
    let neutral = curve.target.neutral();
    let mut xs: Vec<f32> = vec![0.0, 1.0];
    xs.extend(curve.points.iter().map(|p| p[0].clamp(0.0, 1.0)));
    xs.sort_by(f32::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    xs.into_iter()
        .map(|x| [x, bake_influence(neutral, influence, curve.eval(x))])
        .collect()
}

/// Cutoff in Hz for a `FilterCutoff` multiplier: 1 = `base_hz`, 0 = four octaves lower.
pub fn cutoff_hz(multiplier: f32, base_hz: f32) -> f32 {
    let octaves_down = (1.0 - multiplier.clamp(0.0, 1.0)) * CUTOFF_RANGE_OCTAVES;
    (base_hz / 2f32.powf(octaves_down)).clamp(10.0, 22_000.0)
}

/// What an Adaptaudio response curve becomes in FMOD Studio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FmodTarget {
    /// Track volume, dB relative to the track fader (`volume_db`).
    TrackVolume,
    /// Cutoff of a Lowpass effect on the track, Hz.
    LowpassCutoff,
    /// Level of a Distortion effect on the track, 0..1.
    DistortionLevel,
    /// Level of the track's send to the reverb return, dB.
    ReverbSendLevel,
    /// Depth of a Chorus effect, % (approximates oscillator detune).
    ChorusDepth,
    /// Mix of a slow Flanger effect, % (approximates tape wow and flutter).
    FlangerMix,
    /// Note density: FMOD can't thin notes, so this is never automated.
    Density,
}

impl FmodTarget {
    pub fn for_curve(target: CurveTarget) -> Self {
        match target {
            CurveTarget::Volume => FmodTarget::TrackVolume,
            CurveTarget::FilterCutoff => FmodTarget::LowpassCutoff,
            CurveTarget::Drive => FmodTarget::DistortionLevel,
            CurveTarget::ReverbSend => FmodTarget::ReverbSendLevel,
            CurveTarget::Detune => FmodTarget::ChorusDepth,
            CurveTarget::Wobble => FmodTarget::FlangerMix,
            CurveTarget::Density => FmodTarget::Density,
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            FmodTarget::TrackVolume => "dB relative to track fader",
            FmodTarget::LowpassCutoff => "Hz",
            FmodTarget::DistortionLevel => "0..1",
            FmodTarget::ReverbSendLevel => "dB",
            FmodTarget::ChorusDepth | FmodTarget::FlangerMix => "%",
            FmodTarget::Density => "note probability 0..1",
        }
    }

    /// The FMOD effect the automation lives on, if it is not the track itself.
    pub fn effect(self) -> Option<&'static str> {
        match self {
            FmodTarget::TrackVolume | FmodTarget::Density => None,
            FmodTarget::LowpassCutoff => Some("Lowpass"),
            FmodTarget::DistortionLevel => Some("Distortion"),
            FmodTarget::ReverbSendLevel => Some("Send"),
            FmodTarget::ChorusDepth => Some("Chorus"),
            FmodTarget::FlangerMix => Some("Flanger"),
        }
    }

    /// True when the mapping is only an approximation of what Adaptaudio does.
    pub fn is_approximate(self) -> bool {
        matches!(self, FmodTarget::ChorusDepth | FmodTarget::FlangerMix)
    }

    /// Converts a baked Adaptaudio value into this target's FMOD unit.
    pub fn value(self, baked: f32, base_cutoff_hz: f32) -> f32 {
        match self {
            FmodTarget::TrackVolume | FmodTarget::ReverbSendLevel => gain_to_db(baked),
            FmodTarget::LowpassCutoff => cutoff_hz(baked, base_cutoff_hz),
            FmodTarget::DistortionLevel | FmodTarget::Density => baked.clamp(0.0, 1.0),
            // Detune is 0..100 cents; chorus depth is 0..100 %: one cent per percent.
            FmodTarget::ChorusDepth => baked.clamp(0.0, 100.0),
            FmodTarget::FlangerMix => baked.clamp(0.0, 1.0) * 100.0,
        }
    }
}

/// The quantization interval of an FMOD transition region.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantizeInterval {
    /// No quantization: jump at once.
    Free,
    Beat,
    Bar,
}

/// An Adaptaudio quantization in FMOD terms.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FmodQuantization {
    pub interval: QuantizeInterval,
    /// FMOD has no "every N bars" interval. A phrase is built from bar-quantized transition regions
    /// that cover only the last bar of each phrase (see `phrase_segments`).
    pub phrase_bars: Option<u16>,
    /// Adaptaudio's own name for it ("Bar", "4 bars"…).
    pub label: String,
}

pub fn map_quantize(quantize: Quantize) -> FmodQuantization {
    let (interval, phrase_bars) = match quantize {
        Quantize::Immediate => (QuantizeInterval::Free, None),
        Quantize::Beat => (QuantizeInterval::Beat, None),
        Quantize::Bar | Quantize::Phrase(0 | 1) => (QuantizeInterval::Bar, None),
        Quantize::Phrase(n) => (QuantizeInterval::Bar, Some(n)),
    };
    FmodQuantization {
        interval,
        phrase_bars,
        label: quantize.name(),
    }
}

/// Where transition regions go inside a source region, as `(start_bar, length_bars)`.
///
/// Without a phrase the whole region is one segment. With an N-bar phrase only the last bar of
/// every phrase is covered (plus the last bar of the region, which is a loop boundary): a
/// bar-quantized transition that becomes true earlier waits there and fires on the phrase end.
pub fn phrase_segments(
    start_bar: u32,
    length_bars: u32,
    phrase_bars: Option<u16>,
) -> Vec<(u32, u32)> {
    let Some(n) = phrase_bars.map(u32::from).filter(|&n| n > 1) else {
        return vec![(start_bar, length_bars)];
    };
    (0..length_bars)
        .filter(|i| (i + 1) % n == 0 || i + 1 == length_bars)
        .map(|i| (start_bar + i, 1))
        .collect()
}

/// Turns any name into something safe for file names and FMOD paths: ASCII letters and digits,
/// with every run of other characters replaced by one `_`. Case is kept.
pub fn sanitize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut gap = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            if gap && !out.is_empty() {
                out.push('_');
            }
            gap = false;
            out.push(c);
        } else {
            gap = true;
        }
    }
    if out.is_empty() {
        "unnamed".into()
    } else {
        out
    }
}

/// Sanitized names made unique (case-insensitively, so they are safe on macOS file systems) by
/// appending `_2`, `_3`… Names in `reserved` are never handed out.
pub fn unique_keys<'a>(names: impl IntoIterator<Item = &'a str>, reserved: &[&str]) -> Vec<String> {
    let mut taken: Vec<String> = reserved.iter().map(|r| r.to_ascii_lowercase()).collect();
    let mut out = Vec::new();
    for name in names {
        let base = sanitize_name(name);
        let mut key = base.clone();
        let mut n = 2;
        while taken.contains(&key.to_ascii_lowercase()) {
            key = format!("{base}_{n}");
            n += 1;
        }
        taken.push(key.to_ascii_lowercase());
        out.push(key);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_to_frames_is_exact_at_120_bpm() {
        assert_eq!(bars_to_frames(1, 120.0, 4, 48_000), 96_000);
        assert_eq!(bars_to_frames(4, 120.0, 4, 48_000), 384_000);
        assert_eq!(bars_to_frames(4, 120.0, 3, 44_100), 264_600);
    }

    #[test]
    fn bars_to_frames_rounds_the_whole_span_once() {
        // 7/8 at 133 BPM: 3.157894… s per bar.
        let one = bars_to_frames(1, 133.0, 7, 48_000);
        let many = bars_to_frames(16, 133.0, 7, 48_000);
        let exact = 16.0 * 7.0 * 60.0 / 133.0 * 48_000.0;
        assert!((many as f64 - exact).abs() <= 0.5);
        assert_eq!(one, (7.0f64 * 60.0 / 133.0 * 48_000.0).round() as usize);
    }

    #[test]
    fn crossfade_beats_to_ms() {
        assert!((beats_to_ms(1.0, 120.0) - 500.0).abs() < 1e-3);
        assert!((beats_to_ms(4.0, 90.0) - 2666.667).abs() < 1e-2);
        assert_eq!(beats_to_ms(-1.0, 120.0), 0.0);
    }

    #[test]
    fn gain_to_db_matches_20_log10_and_clamps() {
        assert!(gain_to_db(1.0).abs() < 1e-6);
        assert!((gain_to_db(0.5) + 6.0206).abs() < 1e-3);
        assert_eq!(gain_to_db(0.0), SILENCE_DB);
        assert_eq!(gain_to_db(1e-9), SILENCE_DB);
        assert_eq!(gain_to_db(f32::NAN), SILENCE_DB);
    }

    #[test]
    fn seek_speed_is_range_over_slew() {
        assert!((seek_speed(2.0, 1.0) - 0.5).abs() < 1e-6);
        assert!((seek_speed(0.25, 1.0) - 4.0).abs() < 1e-6);
        assert_eq!(seek_speed(0.0, 1.0), 0.0);
    }

    #[test]
    fn names_are_sanitized() {
        assert_eq!(
            sanitize_name("Cedar Station — Night Shift"),
            "Cedar_Station_Night_Shift"
        );
        assert_eq!(sanitize_name("  Lub-dub (slow)! "), "Lub_dub_slow");
        assert_eq!(sanitize_name("Hats_8th"), "Hats_8th");
        assert_eq!(sanitize_name("Ёлка"), "unnamed");
        assert_eq!(sanitize_name("__a__b__"), "a_b");
    }

    #[test]
    fn keys_are_unique_ignoring_case_and_reserved_names() {
        let keys = unique_keys(["Pad", "pad", "Pad!", "stinger"], &["stinger"]);
        assert_eq!(keys, vec!["Pad", "pad_2", "Pad_3", "stinger_2"]);
    }

    #[test]
    fn influence_is_baked_into_the_curve() {
        let c = ResponseCurve::new("Intensity", CurveTarget::Volume, &[[0.0, 1.0], [0.8, 0.3]]);
        let full = bake_curve(&c, 1.0);
        assert_eq!(full, vec![[0.0, 1.0], [0.8, 0.3], [1.0, 0.3]]);
        let half = bake_curve(&c, 0.6);
        // neutral 1 + 0.6 · (0.3 − 1) = 0.58
        assert!((half[1][1] - 0.58).abs() < 1e-6);
        assert!((half[0][1] - 1.0).abs() < 1e-6);
        let none = bake_curve(&c, 0.0);
        assert!(none.iter().all(|p| (p[1] - 1.0).abs() < 1e-6));
    }

    #[test]
    fn curve_sampling_adds_both_ends_once() {
        let c = ResponseCurve::new("Tension", CurveTarget::Drive, &[[0.2, 0.0], [0.6, 1.0]]);
        let xs: Vec<f32> = bake_curve(&c, 1.0).iter().map(|p| p[0]).collect();
        assert_eq!(xs, vec![0.0, 0.2, 0.6, 1.0]);
        assert_eq!(
            FmodTarget::DistortionLevel.value(bake_curve(&c, 1.0)[3][1], 0.0),
            1.0
        );
    }

    #[test]
    fn cutoff_spans_four_octaves_below_base() {
        assert!((cutoff_hz(1.0, 1400.0) - 1400.0).abs() < 1e-3);
        assert!((cutoff_hz(0.0, 1600.0) - 100.0).abs() < 1e-3);
        assert!((cutoff_hz(0.5, 1600.0) - 400.0).abs() < 1e-3);
    }

    #[test]
    fn quantize_maps_to_fmod_intervals() {
        assert_eq!(
            map_quantize(Quantize::Immediate).interval,
            QuantizeInterval::Free
        );
        assert_eq!(
            map_quantize(Quantize::Beat).interval,
            QuantizeInterval::Beat
        );
        assert_eq!(map_quantize(Quantize::Bar).interval, QuantizeInterval::Bar);
        let phrase = map_quantize(Quantize::Phrase(2));
        assert_eq!(
            (phrase.interval, phrase.phrase_bars),
            (QuantizeInterval::Bar, Some(2))
        );
        assert_eq!(map_quantize(Quantize::Phrase(1)).phrase_bars, None);
    }

    #[test]
    fn phrases_cover_the_last_bar_of_each_phrase() {
        assert_eq!(phrase_segments(8, 4, None), vec![(8, 4)]);
        assert_eq!(phrase_segments(8, 4, Some(2)), vec![(9, 1), (11, 1)]);
        // 6 bars in 4-bar phrases: the phrase end and the loop end.
        assert_eq!(phrase_segments(0, 6, Some(4)), vec![(3, 1), (5, 1)]);
    }

    #[test]
    fn fmod_values_per_target() {
        assert!((FmodTarget::TrackVolume.value(0.5, 0.0) + 6.0206).abs() < 1e-3);
        assert_eq!(FmodTarget::ReverbSendLevel.value(0.0, 0.0), SILENCE_DB);
        assert_eq!(FmodTarget::ChorusDepth.value(45.0, 0.0), 45.0);
        assert_eq!(FmodTarget::FlangerMix.value(0.8, 0.0), 80.0);
        assert_eq!(
            FmodTarget::for_curve(CurveTarget::Density),
            FmodTarget::Density
        );
    }
}
