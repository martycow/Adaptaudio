//! The starter project: one night at Cedar Station, in every game mode.
//!
//! It doubles as documentation of how a full adaptive project is put together.

use crate::model::*;
use crate::music::{PitchClass, ScaleKind};

/// Drum hits from a 16-character grid: `x` = accent, `o` = normal, `.` = rest.
fn beat(name: &str, grid: &str, pitch: u8) -> Clip {
    let mut clip = Clip::new(name, grid.chars().count() as u16);
    for (step, ch) in grid.chars().enumerate() {
        let velocity = match ch {
            'x' => 1.0,
            'o' => 0.7,
            '-' => 0.4,
            _ => continue,
        };
        clip = clip.with_note(step as u16, pitch, velocity, 1.0);
    }
    clip
}

/// Melodic notes as `(step, pitch, length_steps)`.
fn phrase(name: &str, length_steps: u16, notes: &[(u16, u8, f32)], velocity: f32) -> Clip {
    let mut clip = Clip::new(name, length_steps);
    for &(step, pitch, len) in notes {
        clip = clip.with_note(step, pitch, velocity, len);
    }
    clip
}

/// A chord held for `len` steps from `step`.
fn chord(notes: &mut Vec<(u16, u8, f32)>, step: u16, pitches: &[u8], len: f32) {
    for &p in pitches {
        notes.push((step, p, len));
    }
}

fn with_probability(mut clip: Clip, every_other: f32) -> Clip {
    for n in &mut clip.notes {
        if n.step % 2 == 1 {
            n.probability = every_other;
        }
    }
    clip
}

struct Ids {
    explore: SectionId,
    stealth: SectionId,
    search: SectionId,
    alert: SectionId,
    chase: SectionId,
    relief: SectionId,
    puzzle: SectionId,
    play_dead: SectionId,
}

/// Cedar Station, night shift: D minor at 120 BPM, eight game modes.
pub fn cedar_station() -> Project {
    let mut p = Project::new("Cedar Station — Night Shift");
    p.tempo_bpm = 120.0;
    p.key = PitchClass(2);
    p.scale = ScaleKind::NaturalMinor;

    let ids = Ids {
        explore: p.add_section("Explore", 4),
        stealth: p.add_section("Stealth", 4),
        search: p.add_section("Search", 4),
        alert: p.add_section("Alert", 4),
        chase: p.add_section("Chase", 4),
        relief: p.add_section("Relief", 4),
        puzzle: p.add_section("Puzzle", 4),
        play_dead: p.add_section("PlayDead", 4),
    };

    drums(&mut p, &ids);
    bass(&mut p, &ids);
    pad(&mut p, &ids);
    arp_and_lead(&mut p, &ids);
    ocarina_and_ambience(&mut p, &ids);
    let fx = stingers(&mut p);
    modes(&mut p, &ids);
    transitions(&mut p);
    events(&mut p, fx);
    scenarios(&mut p);
    puzzles(&mut p);
    p
}

fn drums(p: &mut Project, ids: &Ids) {
    let kick = p.add_track("Kick", TrackRole::Kick);
    let chase = p.add_clip(kick, beat("Four on the floor", "x...x...x...x...", 36));
    let alert = p.add_clip(kick, beat("Half time", "x.......x..o....", 36));
    let puzzle = p.add_clip(kick, beat("Puzzle pulse", "x.......x.......", 36));
    p.set_slot(ids.chase, kick, chase);
    p.set_slot(ids.alert, kick, alert);
    p.set_slot(ids.puzzle, kick, puzzle);
    if let Some(t) = p.track_mut(kick) {
        t.mixer.volume_db = -3.0;
        t.mixer.reverb_send = 0.02;
        t.adaptive.curves.push(ResponseCurve::new(
            "Intensity",
            CurveTarget::Volume,
            &[[0.0, 0.0], [0.35, 0.15], [0.6, 1.0]],
        ));
    }

    let heart = p.add_track("Heartbeat", TrackRole::Perc);
    let thump = p.add_clip(heart, beat("Lub-dub", "x..o............", 31));
    let slow = p.add_clip(
        heart,
        beat("Lub-dub slow", "x..o............................", 29),
    );
    p.set_slot(ids.play_dead, heart, slow);
    p.set_slot(ids.stealth, heart, thump);
    if let Some(t) = p.track_mut(heart) {
        t.instrument = Instrument::Synth(SynthPatch {
            name: "Heart: muffled thump".into(),
            filter: FilterSettings {
                mode: FilterMode::LowPass,
                cutoff_hz: 220.0,
                resonance: 0.0,
                env_octaves: 1.0,
            },
            pitch_env: PitchEnv {
                semitones: 12.0,
                decay: 0.06,
            },
            ..SynthPatch::preset(TrackRole::Kick)
        });
        t.mixer.volume_db = -6.0;
        t.adaptive.curves.push(ResponseCurve::new(
            "Tension",
            CurveTarget::Volume,
            &[[0.0, 0.3], [1.0, 1.0]],
        ));
    }

    let snare = p.add_track("Snare", TrackRole::Snare);
    let chase = p.add_clip(snare, beat("Backbeat", "....x.......x..-", 38));
    let alert = p.add_clip(snare, beat("Alert snap", "............x...", 38));
    p.set_slot(ids.chase, snare, chase);
    p.set_slot(ids.alert, snare, alert);
    if let Some(t) = p.track_mut(snare) {
        t.mixer.volume_db = -8.0;
        t.mixer.reverb_send = 0.25;
        t.adaptive.curves.push(ResponseCurve::new(
            "Intensity",
            CurveTarget::Volume,
            &[[0.0, 0.0], [0.45, 0.2], [0.7, 1.0]],
        ));
    }

    let hats = p.add_track("Hats", TrackRole::Hats);
    let chase = p.add_clip(
        hats,
        with_probability(beat("Sixteenths", "xoooxoooxoooxooo", 42), 0.8),
    );
    let search = p.add_clip(hats, beat("Offbeats", "..o...o...o...o.", 42));
    let stealth = p.add_clip(hats, beat("Ticks", "......-.......-.", 42));
    let puzzle = p.add_clip(hats, beat("Puzzle eighths", "o.o.o.o.o.o.o.o.", 42));
    p.set_slot(ids.chase, hats, chase);
    p.set_slot(ids.alert, hats, search);
    p.set_slot(ids.search, hats, search);
    p.set_slot(ids.stealth, hats, stealth);
    p.set_slot(ids.puzzle, hats, puzzle);
    if let Some(t) = p.track_mut(hats) {
        t.mixer.volume_db = -12.0;
        t.mixer.pan = 0.2;
        t.adaptive.curves.push(ResponseCurve::new(
            "Intensity",
            CurveTarget::Density,
            &[[0.0, 0.3], [1.0, 1.0]],
        ));
    }
}

fn bass(p: &mut Project, ids: &Ids) {
    let bass = p.add_track("Bass", TrackRole::Bass);
    // D2 = 38, Bb1 = 34, C2 = 36, A1 = 33.
    let roots = [38u8, 34, 36, 33];
    let mut drive = Vec::new();
    for (bar, root) in roots.iter().enumerate() {
        for eighth in 0..8u16 {
            let octave_jump = if eighth % 4 == 3 { 12 } else { 0 };
            drive.push((bar as u16 * 16 + eighth * 2, root + octave_jump, 1.6));
        }
    }
    let chase = p.add_clip(bass, phrase("Driving eighths", 64, &drive, 0.9));
    let mut pulse = Vec::new();
    for (bar, root) in roots.iter().enumerate() {
        pulse.push((bar as u16 * 16, *root, 6.0));
        pulse.push((bar as u16 * 16 + 8, *root, 6.0));
    }
    let alert = p.add_clip(bass, phrase("Half-note pulse", 64, &pulse, 0.85));
    let search = p.add_clip(
        bass,
        phrase(
            "Low pulse",
            16,
            &[(0, 38, 3.0), (6, 38, 2.0), (12, 39, 3.0)],
            0.7,
        ),
    );
    let puzzle = p.add_clip(
        bass,
        phrase("Puzzle root", 32, &[(0, 38, 14.0), (16, 34, 14.0)], 0.7),
    );
    p.set_slot(ids.chase, bass, chase);
    p.set_slot(ids.alert, bass, alert);
    p.set_slot(ids.search, bass, search);
    p.set_slot(ids.puzzle, bass, puzzle);
    if let Some(t) = p.track_mut(bass) {
        t.mixer.volume_db = -7.0;
        t.mixer.sidechain = 0.6;
        t.mixer.reverb_send = 0.03;
        t.adaptive.curves.push(ResponseCurve::new(
            "Intensity",
            CurveTarget::Drive,
            &[[0.0, 0.0], [0.5, 0.1], [1.0, 0.6]],
        ));
        t.adaptive.curves.push(ResponseCurve::new(
            "Tension",
            CurveTarget::FilterCutoff,
            &[[0.0, 0.5], [1.0, 1.0]],
        ));
    }
}

fn pad(p: &mut Project, ids: &Ids) {
    let pad = p.add_track("Pad", TrackRole::Pad);
    let mut explore = Vec::new();
    chord(&mut explore, 0, &[50, 57, 60, 64], 30.0); // Dm9 (no 3rd on top)
    chord(&mut explore, 32, &[46, 53, 57, 62], 30.0); // Bbmaj7
    let explore = p.add_clip(pad, phrase("Mossy Dm9 → Bbmaj7", 64, &explore, 0.6));

    let mut stealth = Vec::new();
    chord(&mut stealth, 0, &[38, 50, 53], 62.0);
    chord(&mut stealth, 32, &[51], 30.0); // the Eb rubs: Phrygian shadow
    let stealth = p.add_clip(pad, phrase("Dark drone", 64, &stealth, 0.55));

    let mut chase = Vec::new();
    chord(&mut chase, 0, &[50, 53, 57], 15.0);
    chord(&mut chase, 16, &[46, 50, 53], 15.0);
    chord(&mut chase, 32, &[48, 52, 55], 15.0);
    chord(&mut chase, 48, &[45, 49, 52], 15.0); // A major: the pull back to D
    let chase = p.add_clip(pad, phrase("Dm Bb C A", 64, &chase, 0.65));

    let mut relief = Vec::new();
    chord(&mut relief, 0, &[53, 57, 60, 64], 30.0); // Fmaj7
    chord(&mut relief, 32, &[48, 55, 59, 64], 30.0); // Cmaj7 / lydian glow
    let relief = p.add_clip(pad, phrase("Warm exhale", 64, &relief, 0.55));

    p.set_slot(ids.explore, pad, explore);
    p.set_slot(ids.stealth, pad, stealth);
    p.set_slot(ids.search, pad, stealth);
    p.set_slot(ids.alert, pad, chase);
    p.set_slot(ids.chase, pad, chase);
    p.set_slot(ids.relief, pad, relief);
    p.set_slot(ids.puzzle, pad, explore);
    if let Some(t) = p.track_mut(pad) {
        t.mixer.volume_db = -11.0;
        t.mixer.reverb_send = 0.45;
        t.mixer.sidechain = 0.5;
        t.adaptive.curves.push(ResponseCurve::new(
            "Tension",
            CurveTarget::FilterCutoff,
            &[[0.0, 0.35], [1.0, 1.0]],
        ));
        t.adaptive.curves.push(ResponseCurve::new(
            "Paranoia",
            CurveTarget::Detune,
            &[[0.0, 0.0], [0.5, 8.0], [1.0, 45.0]],
        ));
        t.adaptive.curves.push(ResponseCurve::new(
            "Paranoia",
            CurveTarget::Wobble,
            &[[0.0, 0.0], [1.0, 0.8]],
        ));
    }
}

fn arp_and_lead(p: &mut Project, ids: &Ids) {
    let arp = p.add_track("Arp", TrackRole::Arp);
    let shapes: [[u8; 4]; 4] = [
        [62, 65, 69, 74],
        [58, 62, 65, 70],
        [60, 64, 67, 72],
        [57, 61, 64, 69],
    ];
    let mut notes = Vec::new();
    for (bar, shape) in shapes.iter().enumerate() {
        for i in 0..16u16 {
            let up_down = [0usize, 1, 2, 3, 2, 1, 0, 1];
            notes.push((bar as u16 * 16 + i, shape[up_down[(i % 8) as usize]], 0.8));
        }
    }
    let chase = p.add_clip(arp, phrase("Chip arpeggio", 64, &notes, 0.8));
    let explore = p.add_clip(
        arp,
        phrase(
            "Slow sparkle",
            32,
            &[
                (0, 74, 2.0),
                (6, 69, 2.0),
                (12, 72, 2.0),
                (16, 74, 2.0),
                (22, 77, 2.0),
                (28, 76, 2.0),
            ],
            0.5,
        ),
    );
    p.set_slot(ids.chase, arp, chase);
    p.set_slot(ids.explore, arp, explore);
    p.set_slot(ids.puzzle, arp, explore);
    if let Some(t) = p.track_mut(arp) {
        t.mixer.volume_db = -14.0;
        t.mixer.pan = -0.25;
        t.mixer.reverb_send = 0.3;
        t.mixer.sidechain = 0.3;
        t.adaptive.curves.push(ResponseCurve::new(
            "Intensity",
            CurveTarget::Volume,
            &[[0.0, 0.25], [0.7, 0.4], [0.85, 1.0]],
        ));
        t.adaptive.curves.push(ResponseCurve::new(
            "Paranoia",
            CurveTarget::Detune,
            &[[0.0, 0.0], [1.0, 30.0]],
        ));
    }

    let lead = p.add_track("Lead", TrackRole::Lead);
    let motif = [
        (0, 74, 3.0),
        (3, 77, 3.0),
        (6, 76, 2.0),
        (8, 74, 2.0),
        (10, 72, 2.0),
        (12, 69, 4.0),
        (16, 70, 3.0),
        (19, 74, 3.0),
        (22, 72, 2.0),
        (24, 69, 8.0),
        (32, 74, 3.0),
        (35, 77, 3.0),
        (38, 79, 2.0),
        (40, 81, 6.0),
        (48, 79, 2.0),
        (50, 77, 2.0),
        (52, 76, 4.0),
        (56, 73, 8.0),
    ];
    let chase = p.add_clip(lead, phrase("Run, possum, run", 64, &motif, 0.9));
    p.set_slot(ids.chase, lead, chase);
    if let Some(t) = p.track_mut(lead) {
        t.mixer.volume_db = -12.0;
        t.mixer.reverb_send = 0.25;
        t.adaptive.curves.push(ResponseCurve::new(
            "Intensity",
            CurveTarget::Volume,
            &[[0.0, 0.0], [0.75, 0.3], [0.9, 1.0]],
        ));
        t.adaptive.curves.push(ResponseCurve::new(
            "Paranoia",
            CurveTarget::Wobble,
            &[[0.0, 0.0], [1.0, 1.0]],
        ));
    }
}

fn ocarina_and_ambience(p: &mut Project, ids: &Ids) {
    let ocarina = p.add_track("Ocarina", TrackRole::Lead);
    // The opossum's leitmotif: D F G A … C A.
    let theme = [
        (0, 74, 4.0),
        (4, 77, 2.0),
        (6, 79, 2.0),
        (8, 81, 8.0),
        (20, 84, 4.0),
        (24, 81, 8.0),
        (32, 79, 4.0),
        (36, 77, 4.0),
        (40, 76, 4.0),
        (44, 74, 12.0),
    ];
    let explore = p.add_clip(ocarina, phrase("Possum theme", 64, &theme, 0.7));
    let relief_theme = [
        (0, 77, 4.0),
        (4, 81, 2.0),
        (6, 84, 2.0),
        (8, 83, 8.0),
        (20, 79, 4.0),
        (24, 76, 8.0),
        (32, 77, 16.0),
    ];
    let relief = p.add_clip(
        ocarina,
        phrase("Possum theme, home", 64, &relief_theme, 0.65),
    );
    p.set_slot(ids.explore, ocarina, explore);
    p.set_slot(ids.relief, ocarina, relief);
    if let Some(t) = p.track_mut(ocarina) {
        t.instrument = Instrument::Synth(SynthPatch {
            name: "Ocarina (synth stand-in)".into(),
            osc: [
                Oscillator::new(Waveform::Triangle, 0.9),
                Oscillator {
                    octave: 1,
                    ..Oscillator::new(Waveform::Sine, 0.15)
                },
            ],
            noise: 0.04,
            amp_env: Adsr::new(0.06, 0.2, 0.85, 0.35),
            filter: FilterSettings {
                mode: FilterMode::LowPass,
                cutoff_hz: 3500.0,
                resonance: 0.05,
                env_octaves: 0.3,
            },
            lfo: Lfo {
                rate_hz: 5.0,
                depth: 0.08,
                target: LfoTarget::Pitch,
            },
            glide: 0.02,
            ..SynthPatch::default()
        });
        t.mixer.volume_db = -10.0;
        t.mixer.reverb_send = 0.5;
        t.adaptive.curves.push(ResponseCurve::new(
            "Paranoia",
            CurveTarget::Wobble,
            &[[0.0, 0.0], [1.0, 1.0]],
        ));
        t.adaptive.locked_until_event = None;
    }

    let rain = p.add_track("Rain", TrackRole::Ambience);
    let held = p.add_clip(rain, phrase("Rain on cedar", 64, &[(0, 60, 64.0)], 0.6));
    for s in [
        ids.explore,
        ids.stealth,
        ids.search,
        ids.relief,
        ids.puzzle,
        ids.play_dead,
    ] {
        p.set_slot(s, rain, held);
    }
    if let Some(t) = p.track_mut(rain) {
        t.mixer.volume_db = -14.0;
        t.mixer.reverb_send = 0.3;
        t.adaptive.influence = 0.6;
        t.adaptive.curves.push(ResponseCurve::new(
            "Intensity",
            CurveTarget::Volume,
            &[[0.0, 1.0], [0.8, 0.3]],
        ));
    }

    // A layer only a solved puzzle unlocks: a bell line that joins the Puzzle section.
    let bells = p.add_track("Bells", TrackRole::Arp);
    let line = p.add_clip(
        bells,
        phrase(
            "Unlocked bells",
            32,
            &[
                (0, 86, 2.0),
                (4, 81, 2.0),
                (8, 84, 2.0),
                (12, 79, 2.0),
                (16, 86, 2.0),
                (20, 88, 2.0),
                (24, 84, 4.0),
            ],
            0.6,
        ),
    );
    p.set_slot(ids.puzzle, bells, line);
    p.set_slot(ids.relief, bells, line);
    if let Some(t) = p.track_mut(bells) {
        t.instrument = Instrument::Synth(SynthPatch {
            name: "Bells: tongue drum stand-in".into(),
            osc: [
                Oscillator::new(Waveform::Sine, 0.8),
                Oscillator {
                    octave: 2,
                    detune_cents: 3.0,
                    ..Oscillator::new(Waveform::Sine, 0.25)
                },
            ],
            amp_env: Adsr::new(0.001, 0.9, 0.0, 0.9),
            filter: FilterSettings {
                mode: FilterMode::LowPass,
                cutoff_hz: 6000.0,
                resonance: 0.0,
                env_octaves: 0.0,
            },
            ..SynthPatch::default()
        });
        t.mixer.volume_db = -13.0;
        t.mixer.reverb_send = 0.55;
        t.adaptive.curves.push(ResponseCurve::new(
            "PuzzleProgress",
            CurveTarget::Volume,
            &[[0.0, 0.0], [0.5, 0.4], [1.0, 1.0]],
        ));
        t.adaptive.locked_until_event = Some("PuzzleSolved".into());
    }
}

/// Returns (track, detected clip, solved clip, lost clip).
fn stingers(p: &mut Project) -> (TrackId, Option<ClipId>, Option<ClipId>, Option<ClipId>) {
    let fx = p.add_track("Stingers", TrackRole::Fx);
    if let Some(t) = p.track_mut(fx) {
        t.instrument = Instrument::Synth(SynthPatch {
            name: "Stab: detuned saw hit".into(),
            osc: [
                Oscillator::new(Waveform::Saw, 0.6),
                Oscillator {
                    detune_cents: 25.0,
                    ..Oscillator::new(Waveform::Saw, 0.6)
                },
            ],
            noise: 0.2,
            amp_env: Adsr::new(0.001, 0.6, 0.0, 0.5),
            filter: FilterSettings {
                mode: FilterMode::LowPass,
                cutoff_hz: 1200.0,
                resonance: 0.4,
                env_octaves: 3.0,
            },
            filter_env: Adsr::new(0.001, 0.4, 0.0, 0.3),
            pitch_env: PitchEnv {
                semitones: 7.0,
                decay: 0.08,
            },
            ..SynthPatch::default()
        });
        t.mixer.volume_db = -6.0;
        t.mixer.reverb_send = 0.5;
        t.adaptive.influence = 0.0;
    }
    let mut detected = Vec::new();
    chord(&mut detected, 0, &[38, 50, 51, 57], 6.0); // D + Eb: the alarm rub
    let detected = p.add_clip(fx, phrase("Detected!", 8, &detected, 1.0));
    let mut solved = Vec::new();
    chord(&mut solved, 0, &[62, 66, 69, 74], 12.0); // D major: Picardy relief
    let solved = p.add_clip(fx, phrase("Solved", 16, &solved, 0.8));
    let lost = p.add_clip(
        fx,
        phrase(
            "Lost them",
            16,
            &[(0, 57, 4.0), (4, 53, 4.0), (8, 50, 8.0)],
            0.6,
        ),
    );
    (fx, detected, solved, lost)
}

fn modes(p: &mut Project, ids: &Ids) {
    let mode =
        |name: &str, section: SectionId, color: &str, presets: &[(&str, f32)], about: &str| {
            GameModeDef {
                name: name.into(),
                section: Some(section),
                color: color.into(),
                param_presets: presets
                    .iter()
                    .map(|(n, v)| ((*n).to_string(), *v))
                    .collect(),
                about: about.into(),
            }
        };
    p.game_modes = vec![
        mode(
            "Explore",
            ids.explore,
            "#7fb069",
            &[("Intensity", 0.1), ("Tension", 0.1)],
            "Curiosity, cozy PNW forest",
        ),
        mode(
            "Stealth",
            ids.stealth,
            "#2a9d8f",
            &[("Intensity", 0.2), ("Tension", 0.45)],
            "Hold your breath",
        ),
        mode(
            "Search",
            ids.search,
            "#e9a23b",
            &[("Intensity", 0.4), ("Tension", 0.6)],
            "They are looking for you",
        ),
        mode(
            "Alert",
            ids.alert,
            "#e63946",
            &[("Intensity", 0.6), ("Tension", 0.8)],
            "You've been spotted!",
        ),
        mode(
            "Chase",
            ids.chase,
            "#ff2bd6",
            &[("Intensity", 1.0), ("Tension", 0.9)],
            "Run! The peak",
        ),
        mode(
            "Relief",
            ids.relief,
            "#f4c95d",
            &[("Intensity", 0.1), ("Tension", 0.0)],
            "Exhale",
        ),
        mode(
            "Puzzle",
            ids.puzzle,
            "#9b5de5",
            &[("Intensity", 0.3), ("Tension", 0.3)],
            "Focus, rhythm",
        ),
        mode(
            "PlayDead",
            ids.play_dead,
            "#8d99ae",
            &[("Intensity", 0.0), ("Tension", 0.7)],
            "The opossum plays dead",
        ),
    ];
}

fn transitions(p: &mut Project) {
    let rule = |from: Option<&str>,
                to: Option<&str>,
                quantize: Quantize,
                crossfade_beats: f32,
                stinger: Option<&str>| TransitionRule {
        from: from.map(Into::into),
        to: to.map(Into::into),
        spec: TransitionSpec {
            quantize,
            crossfade_beats,
            stinger_event: stinger.map(Into::into),
            min_dwell_seconds: 1.0,
        },
    };
    p.default_transition = TransitionSpec {
        quantize: Quantize::Bar,
        crossfade_beats: 1.0,
        stinger_event: None,
        min_dwell_seconds: 1.0,
    };
    p.transitions = vec![
        rule(Some("Stealth"), Some("Search"), Quantize::Bar, 2.0, None),
        rule(
            Some("Stealth"),
            Some("Alert"),
            Quantize::Beat,
            0.0,
            Some("Detected"),
        ),
        rule(
            Some("Search"),
            Some("Alert"),
            Quantize::Beat,
            0.0,
            Some("Detected"),
        ),
        rule(Some("Alert"), Some("Chase"), Quantize::Bar, 1.0, None),
        rule(
            Some("Chase"),
            Some("Relief"),
            Quantize::Phrase(2),
            4.0,
            Some("Lost"),
        ),
        rule(None, Some("PlayDead"), Quantize::Immediate, 0.5, None),
        rule(Some("PlayDead"), Some("Stealth"), Quantize::Bar, 8.0, None),
    ];
}

fn events(p: &mut Project, fx: (TrackId, Option<ClipId>, Option<ClipId>, Option<ClipId>)) {
    let (track, detected, solved, lost) = fx;
    let ev = |name: &str, clip: Option<ClipId>, quantize: Quantize, unlocks: bool, about: &str| {
        EventDef {
            name: name.into(),
            stinger: clip.map(|clip| Stinger { track, clip }),
            quantize,
            unlocks,
            unlock_track: None,
            about: about.into(),
        }
    };
    p.events = vec![
        ev(
            "Detected",
            detected,
            Quantize::Beat,
            false,
            "An enemy saw the opossum",
        ),
        ev(
            "Lost",
            lost,
            Quantize::Bar,
            false,
            "The enemy lost the trail",
        ),
        ev(
            "PuzzleSolved",
            solved,
            Quantize::Bar,
            true,
            "A puzzle is solved: unlocks a layer",
        ),
        ev(
            "PuzzleFail",
            None,
            Quantize::Immediate,
            false,
            "A wrong input",
        ),
        ev("Heartbeat", None, Quantize::Beat, false, "One heartbeat"),
    ];
}

fn scenarios(p: &mut Project) {
    let step = |at: f32, action: ScenarioAction| ScenarioStep {
        at_seconds: at,
        action,
    };
    let mode = |m: &str| ScenarioAction::SetMode(m.into());
    let param = |n: &str, v: f32, over: f32| ScenarioAction::SetParam {
        name: n.into(),
        value: v,
        over_seconds: over,
    };
    p.scenarios = vec![
        Scenario {
            name: "Full night".into(),
            steps: vec![
                step(0.0, mode("Explore")),
                step(0.0, param("Intensity", 0.1, 0.0)),
                step(0.0, param("Tension", 0.1, 0.0)),
                step(16.0, mode("Stealth")),
                step(16.0, param("Tension", 0.45, 8.0)),
                step(32.0, ScenarioAction::Trigger("Detected".into())),
                step(32.0, mode("Alert")),
                step(32.0, param("Intensity", 0.6, 1.0)),
                step(40.0, mode("Chase")),
                step(40.0, param("Intensity", 1.0, 4.0)),
                step(48.0, param("Paranoia", 0.7, 6.0)),
                step(64.0, mode("Relief")),
                step(64.0, param("Intensity", 0.1, 8.0)),
                step(64.0, param("Tension", 0.0, 8.0)),
                step(64.0, param("Paranoia", 0.0, 6.0)),
                step(80.0, mode("Puzzle")),
                step(80.0, param("PuzzleProgress", 1.0, 16.0)),
                step(96.0, ScenarioAction::Trigger("PuzzleSolved".into())),
                step(104.0, mode("PlayDead")),
            ],
        },
        Scenario {
            name: "Spotted!".into(),
            steps: vec![
                step(0.0, mode("Stealth")),
                step(0.0, param("Tension", 0.5, 0.0)),
                step(6.0, ScenarioAction::Trigger("Detected".into())),
                step(6.0, mode("Alert")),
                step(10.0, mode("Chase")),
                step(10.0, param("Intensity", 1.0, 3.0)),
            ],
        },
    ];
}

fn puzzles(p: &mut Project) {
    let id = p.alloc_id();
    p.puzzles.push(Puzzle {
        id,
        name: "Cedar door knocks".into(),
        kind: PuzzleKind::Timing {
            beats: vec![0.0, 1.0, 2.0, 2.5, 3.0, 4.0, 5.0, 6.0, 6.5, 7.0],
            lanes: vec![0, 0, 1, 1, 0, 0, 0, 1, 1, 0],
        },
        window_ms: 80.0,
        paranoia_shift_ms: 60.0,
    });
    let id = p.alloc_id();
    p.puzzles.push(Puzzle {
        id,
        name: "Fuse box rhythm".into(),
        kind: PuzzleKind::Pattern {
            lanes: 3,
            steps: 8,
            target: vec![[0, 0], [0, 4], [1, 2], [1, 6], [2, 3], [2, 7]],
            pitches: vec![50, 57, 62],
        },
        window_ms: 0.0,
        paranoia_shift_ms: 0.0,
    });
    let id = p.alloc_id();
    p.puzzles.push(Puzzle {
        id,
        name: "Owl call".into(),
        kind: PuzzleKind::Echo {
            phrase: vec![
                EchoNote {
                    beat: 0.0,
                    lane: 0,
                    pitch: 74,
                },
                EchoNote {
                    beat: 1.0,
                    lane: 2,
                    pitch: 81,
                },
                EchoNote {
                    beat: 1.5,
                    lane: 1,
                    pitch: 77,
                },
                EchoNote {
                    beat: 2.0,
                    lane: 0,
                    pitch: 74,
                },
            ],
            answer_offset_beats: 4.0,
        },
        window_ms: 110.0,
        paranoia_shift_ms: 40.0,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_has_a_section_and_every_slot_points_at_a_real_clip() {
        let p = cedar_station();
        assert_eq!(p.game_modes.len(), 8);
        for m in &p.game_modes {
            assert!(
                m.section.and_then(|s| p.section(s)).is_some(),
                "{} has no section",
                m.name
            );
        }
        for s in &p.sections {
            for slot in &s.slots {
                assert!(
                    p.clip(slot.track, slot.clip).is_some(),
                    "{}: dangling slot",
                    s.name
                );
            }
        }
    }

    #[test]
    fn stingers_and_transitions_name_real_events() {
        let p = cedar_station();
        for e in &p.events {
            if let Some(st) = e.stinger {
                assert!(p.clip(st.track, st.clip).is_some());
            }
        }
        for r in &p.transitions {
            if let Some(ev) = &r.spec.stinger_event {
                assert!(p.event_index(ev).is_some(), "unknown event {ev}");
            }
            for m in r.from.iter().chain(r.to.iter()) {
                assert!(p.mode_index(m).is_some(), "unknown mode {m}");
            }
        }
    }

    #[test]
    fn ids_are_unique() {
        let p = cedar_station();
        let mut ids: Vec<u32> = p.tracks.iter().map(|t| t.id).collect();
        ids.extend(p.tracks.iter().flat_map(|t| t.clips.iter().map(|c| c.id)));
        ids.extend(p.sections.iter().map(|s| s.id));
        ids.extend(p.puzzles.iter().map(|z| z.id));
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }
}
