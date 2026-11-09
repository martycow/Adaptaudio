---
category: tech
owner: marty
created_at: 2026-11-09
last_verified: 2026-11-09
source_of_truth: how coding agents must work in this repository (setup, commands, conventions, boundaries)
guard: none
---

# AGENTS.md — Adaptaudio

## Project overview

Adaptaudio is an internal tool for composing, prototyping and exporting the adaptive music of
**Cedar Station**: a psychological-thriller puzzle adventure with rhythmic puzzles, about an
anthropomorphic opossum living in the Pacific Northwest.

Adaptaudio combines three things:

- **Sequencer:** tracks by role (Pad, Bass, Kick…), clips with step patterns, per-note sound settings.
- **Sound lab:** a built-in synth with a sound library, a sampler, and hardware synths played
  over MIDI and recorded back in (auto-sampling).
- **Adaptive engine:** game parameters, game modes, events, per-track Influence, transitions,
  and a Playtest screen that plays the game's role.

Music is exported to **FMOD Studio**. In the game, FMOD plays it, not Adaptaudio.

The user is new to music theory. Use plain words in UI text and docs, and add new terms to
[docs/GLOSSARY.md](docs/GLOSSARY.md).

Full picture: [docs/VISION.md](docs/VISION.md).

## Key decisions

| Topic | Decision |
|---|---|
| Game engine | Unity |
| Game audio runtime | FMOD. No custom native plugin: Adaptaudio exports stems + `manifest.json` |
| UI framework | Rune and its component kit `rune-kit`, pinned to one git revision of `dev` |
| Assistant providers | Claude, Codex (OpenAI), Qwen (DashScope), each by API key in the Keychain |
| App updates | GitHub Releases; the repository is set in Settings → Updates |
| Game parameters | Intensity, Tension, Paranoia, PuzzleProgress (0..1) |
| Game modes | Explore, Stealth, Search, Alert, Chase, Relief, Puzzle, PlayDead |
| Puzzle kinds | Timing, Pattern, Echo |

Decision log and open questions: [docs/VISION.md](docs/VISION.md#решения).

## Stack

- **Rust:** the whole application (Cargo workspace).
- **[Rune](https://github.com/anomalyco/rune):** GPU-rendered native UI with headless mode.
  Pre-release, so pin the git revision and bump it only as a separate change.
- **FMOD Studio + FMOD for Unity:** the runtime in the game. Not a dependency of this repo.

## Equipment

| Device | Use |
|---|---|
| ASM Hydrasynth Explorer | Main synth: live over MIDI and auto-sampled |
| Arturia MiniFreak | Second synth: arps and textures |
| Teenage Engineering EP-1320 | Sampler and groovebox, synced by MIDI Clock |
| Focusrite Scarlett Solo (2nd gen) | USB audio interface: input 1 microphone, input 2 instrument or line |
| Zoom H4essential | Field recorder |
| Tongue drum, ocarina | Recorded as samples |
| Mac Mini | Host machine (macOS, Apple Silicon) |

## Repository layout

Update this section when the structure changes. Details are in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

```
AGENTS.md
README.md
Cargo.toml            workspace; rust-toolchain.toml pins Rust 1.99.1 (Rune needs it)
clippy.toml           allows unwrap/expect in tests only
docs/                 product, audio, architecture, glossary and roadmap docs
crates/
  aa-core/            data model and serialization
  aa-theory/          scales, chords, rhythms (pure functions)
  aa-adaptive/        parameters, curves, modes, transitions (deterministic)
  aa-puzzle/          rhythmic puzzle logic: Timing / Pattern / Echo
  aa-engine/          real-time audio engine
  aa-midi/            MIDI I/O and clock
  aa-capture/         recording: hardware auto-sampling, clock-synced loops, free takes, latency
  aa-export/          FMOD export: stems, manifest, puzzle data and test vectors
  aa-assistant/       AI assistant: Claude, OpenAI and Qwen APIs via curl, proposals applied with Undo
apps/
  adaptaudio/         Rune application: one module per screen in src/screens/
tools/
  fmod-import/        FMOD Studio JavaScript import script
  bundle-macos/       builds Adaptaudio.app and the release zip the in-app updater downloads
```

## Setup and commands

Headless screenshots: `cargo run -p adaptaudio -- --snap shot.png --scene chase --at 3`
(scenes are listed in `apps/adaptaudio/src/main.rs`). Render a scenario to WAV:
`cargo run --release -p aa-engine --example render_scenario -- "Full night" night.wav`.

| Task | Command |
|---|---|
| Install dependencies | `cargo fetch` |
| Run locally | `cargo run -p adaptaudio` |
| Run tests | `cargo test --workspace` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Format | `cargo fmt --all` |
| Build | `cargo build --release -p adaptaudio` |
| Build the .app and release zip | `tools/bundle-macos/bundle.sh` |

Run the tests and the linter before you report a task as done.
If a command fails, report the output. Do not hide or skip failures.

## Coding conventions

- Follow the style of the existing code before introducing a new pattern.
- Keep changes small and focused on the task. Do not refactor unrelated code.
- Prefer clear names over comments. Comment only the non-obvious "why".
- Do not add new dependencies without asking first.
- Keep music, adaptive and puzzle logic in pure crates, separate from audio I/O, rendering and
  input, so it can be tested without a UI or an audio device.
- Add or update tests when you change behavior.
- Name game modes `GameMode` and musical modes `Scale`, never just "mode". In the UI they are
  «Режим» and «Лад».

### Real-time audio rules

The audio callback (`aa-engine`) must not:

- allocate or free memory;
- take locks, or wait on anything;
- do I/O or logging.

Send commands to the audio thread through lock-free queues (`aa_engine::Controller`). Read its
state back through `aa_engine::Telemetry` atomics. Projects the engine replaces travel back to
the UI thread to be freed (`Controller::collect_garbage`). Schedule events in samples from the
transport position, never by wall-clock time.

Listing audio inputs can block while macOS asks for microphone access: call it only from a
worker thread (see `apps/adaptaudio/src/screens/capture.rs`).

### FMOD compatibility

Every feature either exports to FMOD or is marked "prototype only" in the UI. When you add a
feature, update the mapping table in
[docs/ADAPTIVE_AUDIO.md](docs/ADAPTIVE_AUDIO.md#как-это-ложится-на-fmod).

## Git and commits

- Do not commit or push unless the user asks.
- Use short, imperative commit messages (e.g. `Add hint system for grid puzzles`).
- Never commit secrets, API keys, or local config files.
- Do not add credit lines to commits or PRs (no "made by Claude", no `Co-Authored-By`).

## Boundaries

**Always**
- Read the relevant code and docs before you change them.
- Verify changes by running tests or the app.

**Ask first**
- Adding or upgrading dependencies, including bumping the Rune revision.
- Deleting files, renaming public APIs, or changing the project structure.
- Changes to build, CI, or deployment config.

**Never**
- Edit generated files, vendored code, or build output by hand.
- Overwrite user assets (samples, recordings, project files, puzzle data) without confirmation.
- Run destructive commands (`rm -rf`, `git reset --hard`, force push).

## Documentation

- Markdown docs live in `docs/` (and `README.md`, `AGENTS.md` at the root).
- Docs in `docs/` are written in Russian. `AGENTS.md` and code are in English.
- Every `.md` file starts with the standard YAML frontmatter block
  (`category`, `owner`, `created_at`, `last_verified`, `source_of_truth`, `guard`).
  Respect the `guard` value before you edit a file.
- Update the docs when your change makes them wrong.

| Doc | Covers |
|---|---|
| [docs/VISION.md](docs/VISION.md) | What Adaptaudio is, hardware, UI, helpers, decisions, open questions |
| [docs/ADAPTIVE_AUDIO.md](docs/ADAPTIVE_AUDIO.md) | Parameters, modes, events, transitions, puzzles, FMOD mapping |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crates, threads, data model, project format, FMOD pipeline |
| [docs/GLOSSARY.md](docs/GLOSSARY.md) | Music and audio terms in plain language |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Phases and their done criteria |
