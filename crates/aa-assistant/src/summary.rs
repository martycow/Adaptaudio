//! The compact project description the model reads before each question.

use aa_core::music::note_name;
use aa_core::{Clip, Project, Track};
use std::fmt::Write;

/// Tempo, key, tracks (role, instrument, mixer, curves, clips), sections, modes, parameters and
/// events, one line per item. Names are quoted so the model copies them exactly into tools.
pub fn project_summary(project: &Project) -> String {
    let mut out = String::new();
    let steps_per_bar = project.steps_per_bar();
    let _ = writeln!(
        out,
        "Project \"{}\": {} BPM, {}/4 time ({steps_per_bar} steps per bar), key {} {} ({}), scale lock {}.",
        project.name,
        trim_float(project.tempo_bpm),
        project.beats_per_bar,
        project.key.name(),
        project.scale.name(),
        project.scale.mood(),
        if project.scale_lock { "on" } else { "off" },
    );

    let _ = writeln!(out, "\nGame parameters (each 0..1):");
    for param in &project.game_params {
        let _ = writeln!(
            out,
            "- {} (default {:.2}): {}",
            param.name, param.default, param.about
        );
    }

    let _ = writeln!(out, "\nTracks:");
    if project.tracks.is_empty() {
        let _ = writeln!(out, "- (none yet)");
    }
    for track in &project.tracks {
        write_track(&mut out, track);
    }

    let _ = writeln!(out, "\nSections (what a game mode plays):");
    if project.sections.is_empty() {
        let _ = writeln!(out, "- (none yet)");
    }
    for section in &project.sections {
        let slots: Vec<String> = section
            .slots
            .iter()
            .filter_map(|slot| {
                let track = project.track(slot.track)?;
                let clip = track.clip(slot.clip)?;
                Some(format!("\"{}\"=\"{}\"", track.name, clip.name))
            })
            .collect();
        let plays = if slots.is_empty() {
            "silent".to_string()
        } else {
            slots.join(", ")
        };
        let _ = writeln!(
            out,
            "- \"{}\" ({} bars): {plays}",
            section.name, section.length_bars
        );
    }

    if !project.game_modes.is_empty() {
        let _ = writeln!(out, "\nGame modes:");
        for mode in &project.game_modes {
            let section = mode
                .section
                .and_then(|id| project.section(id))
                .map_or("no section".to_string(), |s| {
                    format!("plays \"{}\"", s.name)
                });
            let presets: Vec<String> = mode
                .param_presets
                .iter()
                .map(|(name, v)| format!("{name}={v:.2}"))
                .collect();
            let presets = if presets.is_empty() {
                String::new()
            } else {
                format!("; {}", presets.join(" "))
            };
            let about = if mode.about.is_empty() {
                String::new()
            } else {
                format!(" — {}", mode.about)
            };
            let _ = writeln!(out, "- {}: {section}{presets}{about}", mode.name);
        }
    }

    if !project.events.is_empty() {
        let names: Vec<&str> = project.events.iter().map(|e| e.name.as_str()).collect();
        let _ = writeln!(out, "\nGame events: {}", names.join(", "));
    }
    out.trim_end().to_string()
}

fn write_track(out: &mut String, track: &Track) {
    let m = &track.mixer;
    let mut flags = String::new();
    if m.mute {
        flags.push_str(", muted");
    }
    if m.solo {
        flags.push_str(", solo");
    }
    let _ = writeln!(
        out,
        "- \"{}\" [{}, {}]: vol {:.1} dB, pan {:.2}, reverb {:.2}, influence {:.2}{flags}",
        track.name,
        track.role.name(),
        track.instrument.kind_name(),
        m.volume_db,
        m.pan,
        m.reverb_send,
        track.adaptive.influence,
    );
    for curve in &track.adaptive.curves {
        let points: Vec<String> = curve
            .points
            .iter()
            .map(|[x, y]| format!("[{}, {}]", trim_float(*x), trim_float(*y)))
            .collect();
        let _ = writeln!(
            out,
            "    curve {} -> {}: {}",
            curve.param,
            curve.target.name(),
            points.join(" ")
        );
    }
    if let Some(event) = &track.adaptive.locked_until_event {
        let _ = writeln!(out, "    silent until event {event}");
    }
    if track.clips.is_empty() {
        let _ = writeln!(out, "    clips: none");
    }
    for clip in &track.clips {
        let _ = writeln!(out, "    clip \"{}\": {}", clip.name, clip_overview(clip));
    }
}

/// "16 steps, 4 notes, D2..A2".
fn clip_overview(clip: &Clip) -> String {
    let count = clip.notes.len();
    let range = match (
        clip.notes.iter().map(|n| n.pitch).min(),
        clip.notes.iter().map(|n| n.pitch).max(),
    ) {
        (Some(lo), Some(hi)) if lo == hi => format!(", {}", note_name(lo)),
        (Some(lo), Some(hi)) => format!(", {}..{}", note_name(lo), note_name(hi)),
        _ => String::new(),
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("{} steps, {count} note{plural}{range}", clip.length_steps)
}

/// Every note of a clip as `step:pitch(name) vel len`, for the selection context of a clip the
/// user is looking at, so the model can rewrite it.
pub fn clip_notes_text(clip: &Clip) -> String {
    let mut out = format!(
        "Clip \"{}\", {} steps. Notes (step pitch velocity length):",
        clip.name, clip.length_steps
    );
    if clip.notes.is_empty() {
        out.push_str(" none");
    }
    for note in &clip.notes {
        let _ = write!(
            out,
            "\n{} {}({}) {:.2} {}",
            note.step,
            note.pitch,
            note_name(note.pitch),
            note.velocity,
            trim_float(note.length_steps)
        );
    }
    out
}

fn trim_float(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aa_core::{Clip, Project, TrackRole};

    #[test]
    fn summary_lists_the_essentials() {
        let mut p = Project::new("Test");
        p.tempo_bpm = 96.0;
        let bass = p.add_track("Bass", TrackRole::Bass);
        p.add_clip(
            bass,
            Clip::new("Walk", 16)
                .with_note(0, 38, 0.8, 2.0)
                .with_note(8, 45, 0.7, 2.0),
        );
        p.add_section("Stealth", 4);
        let s = project_summary(&p);
        assert!(s.contains("96 BPM"), "{s}");
        assert!(s.contains("key D Minor"), "{s}");
        assert!(s.contains("\"Bass\" [Bass, Synth]"), "{s}");
        assert!(
            s.contains("clip \"Walk\": 16 steps, 2 notes, D2..A2"),
            "{s}"
        );
        assert!(s.contains("\"Stealth\" (4 bars): silent"), "{s}");
        assert!(s.contains("Paranoia"), "{s}");
    }

    #[test]
    fn summary_of_the_demo_is_compact() {
        let s = project_summary(&aa_core::demo::cedar_station());
        assert!(s.contains("Game modes:"));
        assert!(s.contains("curve Intensity -> Volume"));
        assert!(s.len() < 16_000, "summary is {} bytes", s.len());
    }

    #[test]
    fn clip_notes_are_listed_one_per_line() {
        let clip = Clip::new("Hook", 32).with_note(4, 60, 0.5, 1.5);
        assert_eq!(
            clip_notes_text(&clip),
            "Clip \"Hook\", 32 steps. Notes (step pitch velocity length):\n4 60(C4) 0.50 1.5"
        );
    }
}
