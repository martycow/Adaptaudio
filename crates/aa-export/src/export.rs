//! Writing a scene to disk: stems, manifest, puzzle data and a short README.

use crate::convert::{bars_to_frames, seconds_per_bar};
use crate::manifest::{Manifest, RenderJob, STEMS_DIR, plan, validate_manifest};
use aa_core::{Clip, ClipId, Project, Puzzle, SectionId, TrackId};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

/// Renders audio for the exporter. `aa-export` doesn't depend on the engine; the app passes one in.
pub trait StemRenderer {
    /// Render `bars` bars of one track playing its clip in `section`: stereo interleaved `f32` at
    /// `sample_rate`, so `2 × frames` samples.
    ///
    /// Playback starts at the top of the section and keeps looping it for all `bars` (the exporter
    /// asks for several passes and keeps the last one). The adaptive layer is neutral: Influence is
    /// ignored and every game parameter sits at its default. The stem is pre-fader: the instrument,
    /// note-level detail, the track's static drive and its share of the shared reverb are baked in,
    /// but not the track volume or pan, which FMOD applies.
    fn render(
        &mut self,
        project: &Project,
        section: SectionId,
        track: TrackId,
        bars: u32,
        sample_rate: u32,
    ) -> Vec<f32>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BitDepth {
    Int24,
    Float32,
}

impl BitDepth {
    pub fn bits(self) -> u16 {
        match self {
            BitDepth::Int24 => 24,
            BitDepth::Float32 => 32,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportOptions {
    pub sample_rate: u32,
    pub bit_depth: BitDepth,
    /// How many times each loop is rendered back to back; only the last pass is kept, so reverb
    /// and delay tails from the end already ring into the start and the loop wraps seamlessly.
    /// 2 is enough for tails shorter than the section; 1 renders a dry first pass.
    pub loop_passes: u32,
    pub include_stingers: bool,
    /// Silence rendered after a stinger clip so its release and reverb can ring out.
    pub stinger_tail_seconds: f32,
    /// Folder and event name; empty = the project name. Sanitized either way.
    pub scene_name: String,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            sample_rate: 48_000,
            bit_depth: BitDepth::Int24,
            loop_passes: 2,
            include_stingers: true,
            stinger_tail_seconds: 2.0,
            scene_name: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExportReport {
    /// `out_dir/<scene>`.
    pub scene_dir: PathBuf,
    /// The manifest as written to `manifest.json`.
    pub manifest: Manifest,
    /// Every file written, stems first.
    pub files: Vec<PathBuf>,
    pub stems_written: usize,
    /// `manifest.warnings`: prototype-only features and render problems.
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum ExportError {
    InvalidOptions(String),
    /// The manifest failed validation (a bug or a broken project); nothing was written.
    InvalidManifest(Vec<String>),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Wav {
        path: PathBuf,
        source: hound::Error,
    },
    Json(serde_json::Error),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::InvalidOptions(why) => write!(f, "bad export options: {why}"),
            ExportError::InvalidManifest(problems) => {
                write!(f, "export is inconsistent: {}", problems.join("; "))
            }
            ExportError::Io { path, source } => {
                write!(f, "can't write {}: {source}", path.display())
            }
            ExportError::Wav { path, source } => {
                write!(f, "can't write WAV {}: {source}", path.display())
            }
            ExportError::Json(e) => write!(f, "can't encode JSON: {e}"),
        }
    }
}

impl std::error::Error for ExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ExportError::Io { source, .. } => Some(source),
            ExportError::Wav { source, .. } => Some(source),
            ExportError::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for ExportError {
    fn from(e: serde_json::Error) -> Self {
        ExportError::Json(e)
    }
}

pub const MANIFEST_FILE: &str = "manifest.json";
pub const PUZZLES_FILE: &str = "puzzles.json";
pub const PUZZLE_VECTORS_FILE: &str = "puzzle_vectors.json";
pub const README_FILE: &str = "README.txt";
pub const PUZZLES_VERSION: u32 = 1;

/// Exports one scene into `out_dir/<scene>/`:
///
/// ```text
/// stems/*.wav           one per section × track with a clip, exactly length_bars long; stingers
/// manifest.json         see `Manifest`
/// puzzles.json          the project's puzzles plus tempo
/// puzzle_vectors.json   reference inputs → results from the puzzle crate (as given)
/// README.txt            how to import it in FMOD Studio
/// ```
///
/// Existing files with the same names are overwritten; other files in the folder are left alone.
pub fn export_scene(
    project: &Project,
    out_dir: &Path,
    renderer: &mut dyn StemRenderer,
    puzzle_vectors: Option<serde_json::Value>,
    options: &ExportOptions,
) -> Result<ExportReport, ExportError> {
    check_options(options)?;
    let plan = plan(project, options);
    let problems = validate_manifest(&plan.manifest);
    if !problems.is_empty() {
        return Err(ExportError::InvalidManifest(problems));
    }
    let mut manifest = plan.manifest;
    let scene_dir = out_dir.join(&manifest.scene);
    let stems_dir = scene_dir.join(STEMS_DIR);
    std::fs::create_dir_all(&stems_dir).map_err(|source| ExportError::Io {
        path: stems_dir.clone(),
        source,
    })?;

    let mut files = Vec::new();
    let mut render_warnings = Vec::new();
    for job in &plan.jobs {
        let (file, samples) = match job {
            RenderJob::Loop {
                file,
                section,
                track,
                bars,
            } => (
                file,
                render_loop(
                    project,
                    renderer,
                    *section,
                    *track,
                    *bars,
                    options,
                    &mut render_warnings,
                    file,
                ),
            ),
            RenderJob::Stinger {
                file,
                track,
                clip,
                frames,
            } => {
                let samples = render_stinger(
                    project,
                    renderer,
                    *track,
                    *clip,
                    *frames,
                    options.sample_rate,
                )
                .unwrap_or_else(|why| {
                    render_warnings.push(format!("{file}: {why}; written as silence."));
                    vec![0.0; frames * 2]
                });
                (file, samples)
            }
        };
        let path = scene_dir.join(file);
        write_wav(&path, &samples, options.sample_rate, options.bit_depth).map_err(|source| {
            ExportError::Wav {
                path: path.clone(),
                source,
            }
        })?;
        files.push(path);
    }
    let stems_written = files.len();

    if puzzle_vectors.is_none() && !project.puzzles.is_empty() {
        render_warnings.push(format!(
            "No puzzle test vectors were supplied: {PUZZLE_VECTORS_FILE} is empty."
        ));
    }
    manifest.warnings.extend(render_warnings);

    let puzzles = PuzzlesFile {
        puzzles_version: PUZZLES_VERSION,
        tempo_bpm: project.tempo_bpm,
        beats_per_bar: project.beats_per_bar,
        seconds_per_beat: seconds_per_bar(project.tempo_bpm, 1),
        puzzles: &project.puzzles,
    };
    let vectors = puzzle_vectors.unwrap_or_else(|| serde_json::json!({ "vectors": [] }));
    let texts = [
        (MANIFEST_FILE, serde_json::to_string_pretty(&manifest)?),
        (PUZZLES_FILE, serde_json::to_string_pretty(&puzzles)?),
        (PUZZLE_VECTORS_FILE, serde_json::to_string_pretty(&vectors)?),
        (README_FILE, readme_text(&manifest)),
    ];
    for (name, text) in texts {
        let path = scene_dir.join(name);
        std::fs::write(&path, text).map_err(|source| ExportError::Io {
            path: path.clone(),
            source,
        })?;
        files.push(path);
    }

    let warnings = manifest.warnings.clone();
    Ok(ExportReport {
        scene_dir,
        manifest,
        files,
        stems_written,
        warnings,
    })
}

#[derive(Serialize)]
struct PuzzlesFile<'a> {
    puzzles_version: u32,
    tempo_bpm: f32,
    beats_per_bar: u8,
    seconds_per_beat: f64,
    puzzles: &'a [Puzzle],
}

fn check_options(o: &ExportOptions) -> Result<(), ExportError> {
    if !(8_000..=384_000).contains(&o.sample_rate) {
        return Err(ExportError::InvalidOptions(format!(
            "sample rate {} Hz is out of range",
            o.sample_rate
        )));
    }
    if o.loop_passes == 0 {
        return Err(ExportError::InvalidOptions(
            "loop_passes must be at least 1".into(),
        ));
    }
    if !(o.stinger_tail_seconds.is_finite() && o.stinger_tail_seconds >= 0.0) {
        return Err(ExportError::InvalidOptions(
            "stinger tail must be zero or more seconds".into(),
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn render_loop(
    project: &Project,
    renderer: &mut dyn StemRenderer,
    section: SectionId,
    track: TrackId,
    bars: u32,
    options: &ExportOptions,
    warnings: &mut Vec<String>,
    file: &str,
) -> Vec<f32> {
    let (tempo, bpb, sr) = (
        project.tempo_bpm,
        project.beats_per_bar,
        options.sample_rate,
    );
    let passes = options.loop_passes.max(1);
    let buffer = renderer.render(project, section, track, bars * passes, sr);
    let expected = bars_to_frames(bars * passes, tempo, bpb, sr) * 2;
    if buffer.len().abs_diff(expected) > 4 {
        warnings.push(format!(
            "{file}: the renderer returned {} samples, expected {expected}; padded or trimmed to length.",
            buffer.len()
        ));
    }
    take_frames(
        &buffer,
        bars_to_frames(bars * (passes - 1), tempo, bpb, sr),
        bars_to_frames(bars, tempo, bpb, sr),
    )
}

/// Renders a stinger once: its clip padded with silence to whole bars, in a scratch section of a
/// copy of the project, trimmed to the clip length plus the tail.
fn render_stinger(
    project: &Project,
    renderer: &mut dyn StemRenderer,
    track: TrackId,
    clip: ClipId,
    frames: usize,
    sample_rate: u32,
) -> Result<Vec<f32>, String> {
    let mut scratch = project.clone();
    let source = scratch
        .clip(track, clip)
        .cloned()
        .ok_or("the stinger clip is missing")?;
    let bar_frames =
        seconds_per_bar(project.tempo_bpm, project.beats_per_bar) * f64::from(sample_rate);
    let bars = ((frames as f64 / bar_frames).ceil() as u32).max(1);
    let length_steps =
        u16::try_from(bars * scratch.steps_per_bar()).map_err(|_| "the stinger is too long")?;
    let bars_u16 = u16::try_from(bars).map_err(|_| "the stinger is too long")?;
    let padded = scratch
        .add_clip(
            track,
            Clip {
                length_steps,
                ..source
            },
        )
        .ok_or("the stinger track is missing")?;
    let section = scratch.add_section("Adaptaudio stinger render", bars_u16);
    scratch.set_slot(section, track, Some(padded));
    let buffer = renderer.render(&scratch, section, track, bars, sample_rate);
    Ok(take_frames(&buffer, 0, frames))
}

/// `frames` stereo frames starting at `start_frame`, padded with silence where the buffer is short.
fn take_frames(buffer: &[f32], start_frame: usize, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; frames * 2];
    let start = start_frame * 2;
    if start < buffer.len() {
        let end = (start + frames * 2).min(buffer.len());
        out[..end - start].copy_from_slice(&buffer[start..end]);
    }
    out
}

/// Writes stereo interleaved samples as a WAV file (24-bit integer or 32-bit float).
pub fn write_wav(
    path: &Path,
    samples: &[f32],
    sample_rate: u32,
    bit_depth: BitDepth,
) -> Result<(), hound::Error> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: bit_depth.bits(),
        sample_format: match bit_depth {
            BitDepth::Int24 => hound::SampleFormat::Int,
            BitDepth::Float32 => hound::SampleFormat::Float,
        },
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    match bit_depth {
        BitDepth::Int24 => {
            const FULL_SCALE: f32 = 8_388_607.0;
            for &s in samples {
                // NaN clamps to NaN, which casts to 0: silence rather than a crash.
                writer.write_sample((s.clamp(-1.0, 1.0) * FULL_SCALE).round() as i32)?;
            }
        }
        BitDepth::Float32 => {
            for &s in samples {
                writer.write_sample(if s.is_finite() { s } else { 0.0 })?;
            }
        }
    }
    writer.finalize()
}

fn readme_text(m: &Manifest) -> String {
    let warnings = if m.warnings.is_empty() {
        "Нет.".to_string()
    } else {
        m.warnings
            .iter()
            .map(|w| format!("  - {w}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let game_side = m
        .game_side
        .iter()
        .map(|w| format!("  - {w}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Adaptaudio → FMOD Studio: сцена {scene}\n\
         =========================================\n\
         \n\
         Проект: {project}. Темп {tempo} BPM, {bpb}/4, {key} {scale}. {sr} Гц, {bits} бит, стерео.\n\
         Стемов: {stems}. Разделов: {regions}. Переходов: {transitions}.\n\
         \n\
         Как импортировать\n\
         -----------------\n\
         1. Скопируйте эту папку целиком (вместе с stems/) куда-нибудь рядом с проектом FMOD Studio.\n\
         2. Положите tools/fmod-import/import_adaptaudio.js из репозитория Adaptaudio в папку Scripts\n\
            проекта FMOD Studio и выберите Scripts → Reload.\n\
         3. Scripts → Adaptaudio → Import scene… и укажите эту папку.\n\
         4. Скрипт создаст событие event:/{event} и стингеры, а в конце выведет список того, что\n\
            не смог сделать сам (Window → Console). Доделайте эти пункты руками.\n\
         Подробно, по шагам: tools/fmod-import/README.md.\n\
         \n\
         Файлы\n\
         -----\n\
         stems/*.wav           раздел__трек.wav — луп ровно на длину раздела; stinger__событие.wav — стингеры\n\
         manifest.json         всё для FMOD: параметры, регионы, треки, автоматизация, переходы, события\n\
         puzzles.json          головоломки для C#-модуля в Unity\n\
         puzzle_vectors.json   эталонные входы и ответы для C#-тестов головоломок\n\
         \n\
         Что делает игра (FMOD этого не умеет)\n\
         -------------------------------------\n\
         {game_side}\n\
         \n\
         Что не переносится или переносится примерно\n\
         -------------------------------------------\n\
         {warnings}\n",
        scene = m.scene,
        project = m.project_name,
        tempo = m.tempo_bpm,
        bpb = m.beats_per_bar,
        key = m.key,
        scale = m.scale,
        sr = m.sample_rate,
        bits = m.bit_depth.bits(),
        stems = m.stems.len(),
        regions = m.regions.len(),
        transitions = m.transitions.len(),
        event = m.event_path,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::StemKind;
    use aa_core::demo::cedar_station;
    use aa_core::{ExternalInstrument, Instrument};

    /// A constant buffer of exactly the requested length, plus a log of what was asked.
    struct FakeRenderer {
        calls: Vec<(SectionId, TrackId, u32)>,
    }

    impl StemRenderer for FakeRenderer {
        fn render(
            &mut self,
            project: &Project,
            section: SectionId,
            track: TrackId,
            bars: u32,
            sample_rate: u32,
        ) -> Vec<f32> {
            self.calls.push((section, track, bars));
            let frames =
                bars_to_frames(bars, project.tempo_bpm, project.beats_per_bar, sample_rate);
            (0..frames * 2)
                .map(|i| if i / 2 < frames / 2 { 0.25 } else { 0.5 })
                .collect()
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("aa-export-test-{name}-{}", std::process::id()))
    }

    fn wav_frames(path: &Path) -> (u32, hound::WavSpec) {
        let reader = hound::WavReader::open(path).unwrap();
        (reader.duration(), reader.spec())
    }

    #[test]
    fn exports_the_demo_scene() {
        let project = cedar_station();
        let dir = temp_dir("demo");
        let mut renderer = FakeRenderer { calls: Vec::new() };
        let options = ExportOptions {
            scene_name: "Station Night".into(),
            ..ExportOptions::default()
        };
        let vectors = serde_json::json!({ "vectors": [{ "puzzle": "Cedar door knocks", "presses": [], "expected": "fail" }] });
        let report = export_scene(
            &project,
            &dir,
            &mut renderer,
            Some(vectors.clone()),
            &options,
        )
        .unwrap();

        let scene = dir.join("Station_Night");
        assert_eq!(report.scene_dir, scene);
        let m = &report.manifest;
        let loops = m.stems.iter().filter(|s| s.kind == StemKind::Loop).count();
        let stingers = m
            .stems
            .iter()
            .filter(|s| s.kind == StemKind::Stinger)
            .count();
        let slots: usize = project.sections.iter().map(|s| s.slots.len()).sum();
        assert_eq!(loops, slots);
        assert_eq!(stingers, 3);
        let wavs = std::fs::read_dir(scene.join(STEMS_DIR)).unwrap().count();
        assert_eq!(wavs, loops + stingers);
        assert_eq!(report.stems_written, wavs);
        assert_eq!(report.files.len(), wavs + 4);

        // Every loop stem is exactly 4 bars at 120 BPM / 48 kHz, and the second pass was kept.
        for s in m.stems.iter().filter(|s| s.kind == StemKind::Loop) {
            let (frames, spec) = wav_frames(&scene.join(&s.file));
            assert_eq!(frames, 384_000, "{}", s.file);
            assert_eq!(
                (spec.channels, spec.bits_per_sample, spec.sample_rate),
                (2, 24, 48_000)
            );
            let mut reader = hound::WavReader::open(scene.join(&s.file)).unwrap();
            let first = reader.samples::<i32>().next().unwrap().unwrap();
            assert_eq!(
                first,
                (0.5f32 * 8_388_607.0).round() as i32,
                "second pass kept"
            );
        }
        // Loops were asked for twice their length.
        assert!(renderer.calls.iter().filter(|c| c.2 == 8).count() >= loops);

        // Detected: an 8-step clip (1 s) plus a 2 s tail = 3 s.
        let detected = m
            .stems
            .iter()
            .find(|s| s.file == "stems/stinger__Detected.wav")
            .unwrap();
        assert_eq!(detected.length_frames, 144_000);
        assert_eq!(wav_frames(&scene.join(&detected.file)).0, 144_000);

        let text = std::fs::read_to_string(scene.join(MANIFEST_FILE)).unwrap();
        let back: Manifest = serde_json::from_str(&text).unwrap();
        assert_eq!(&back, m);
        assert_eq!(validate_manifest(&back), Vec::<String>::new());
        let raw: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(raw["manifest_version"], 1);
        assert_eq!(raw["regions"][0]["loop"], true);

        for needle in [
            "Scenarios",
            "note probability",
            "Hats': Intensity → Density",
            "Chorus",
            "sidechain",
        ] {
            assert!(
                report.warnings.iter().any(|w| w.contains(needle)),
                "missing warning '{needle}': {:#?}",
                report.warnings
            );
        }
        let back_vectors: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(scene.join(PUZZLE_VECTORS_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(back_vectors, vectors);
        let puzzles: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(scene.join(PUZZLES_FILE)).unwrap())
                .unwrap();
        assert_eq!(puzzles["puzzles"].as_array().map(Vec::len), Some(3));
        assert!(
            std::fs::read_to_string(scene.join(README_FILE))
                .unwrap()
                .contains("Scripts")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn external_instruments_and_missing_vectors_are_flagged() {
        let mut project = cedar_station();
        if let Some(t) = project.tracks.iter_mut().find(|t| t.name == "Lead") {
            t.instrument = Instrument::External(ExternalInstrument {
                port_name: "Hydrasynth".into(),
                channel: 1,
                latency_ms: 0.0,
            });
        }
        let dir = temp_dir("external");
        let mut renderer = FakeRenderer { calls: Vec::new() };
        let options = ExportOptions {
            bit_depth: BitDepth::Float32,
            include_stingers: false,
            sample_rate: 8_000,
            ..ExportOptions::default()
        };
        let report = export_scene(&project, &dir, &mut renderer, None, &options).unwrap();
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("'Lead'") && w.contains("External MIDI"))
        );
        assert!(report.warnings.iter().any(|w| w.contains("test vectors")));
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("include_stingers"))
        );
        assert!(
            report
                .manifest
                .stems
                .iter()
                .all(|s| s.kind == StemKind::Loop)
        );
        assert!(
            report
                .manifest
                .transitions
                .iter()
                .all(|t| t.stinger_stem.is_none())
        );
        let first = &report.manifest.stems[0];
        let (frames, spec) = wav_frames(&report.scene_dir.join(&first.file));
        assert_eq!(
            (frames, spec.bits_per_sample, spec.sample_format),
            (64_000, 32, hound::SampleFormat::Float)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn short_renders_are_padded_and_reported() {
        struct Short;
        impl StemRenderer for Short {
            fn render(
                &mut self,
                _: &Project,
                _: SectionId,
                _: TrackId,
                _: u32,
                _: u32,
            ) -> Vec<f32> {
                vec![0.1; 100]
            }
        }
        let dir = temp_dir("short");
        let options = ExportOptions {
            sample_rate: 8_000,
            loop_passes: 1,
            ..ExportOptions::default()
        };
        let report = export_scene(&cedar_station(), &dir, &mut Short, None, &options).unwrap();
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("padded or trimmed"))
        );
        let s = &report.manifest.stems[0];
        assert_eq!(
            u64::from(wav_frames(&report.scene_dir.join(&s.file)).0),
            s.length_frames
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_options_are_refused() {
        let mut renderer = FakeRenderer { calls: Vec::new() };
        let options = ExportOptions {
            loop_passes: 0,
            ..ExportOptions::default()
        };
        let result = export_scene(
            &cedar_station(),
            &temp_dir("bad"),
            &mut renderer,
            None,
            &options,
        );
        assert!(matches!(result, Err(ExportError::InvalidOptions(_))));
        assert!(renderer.calls.is_empty());
    }

    #[test]
    fn take_frames_pads_and_offsets() {
        let buf = [1.0, 1.0, 2.0, 2.0, 3.0, 3.0];
        assert_eq!(take_frames(&buf, 1, 2), vec![2.0, 2.0, 3.0, 3.0]);
        assert_eq!(take_frames(&buf, 2, 2), vec![3.0, 3.0, 0.0, 0.0]);
        assert_eq!(take_frames(&buf, 5, 1), vec![0.0, 0.0]);
    }
}
