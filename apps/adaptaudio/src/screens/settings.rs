//! Settings: audio, MIDI, autosave, AI assistant, project.

use crate::actions;
use crate::state::App;
use crate::theme::{self, font};
use crate::widgets::{body, button, caps, card, screen_header, segmented, toggle};
use rune::{Color, El, col, row, text, text_input};

const BUFFERS: [Option<u32>; 5] = [None, Some(128), Some(256), Some(512), Some(1024)];
const AUTOSAVE: [u32; 4] = [0, 30, 60, 300];
const MODELS: [&str; 3] = ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"];

pub fn view(s: &App, accent: Color) -> El {
    let st = &s.settings;
    let audio = card()
        .child(caps("Audio output"))
        .child(
            text(s.audio.as_ref().map_or_else(
                || format!("Not running: {}", s.audio_error.clone().unwrap_or_default()),
                |a| {
                    format!(
                        "{} · {} Hz · {} ch",
                        a.device_name, a.sample_rate, a.channels
                    )
                },
            ))
            .font(font::HEAD)
            .color(if s.audio.is_some() {
                theme::text()
            } else {
                theme::danger()
            }),
        )
        .child(body(
            "Device: system default (change it in macOS Sound settings, then press Restart).",
        ))
        .child(
            row()
                .gap(8.0)
                .items_center()
                .child(text("Buffer").font(font::LABEL).color(theme::muted()))
                .child(segmented(
                    "buf",
                    &["Default", "128", "256", "512", "1024"],
                    BUFFERS
                        .iter()
                        .position(|b| *b == st.buffer_frames)
                        .unwrap_or(0),
                    accent,
                    |s: &mut App, i| {
                        s.settings.buffer_frames = BUFFERS[i];
                        let _ = s.settings.save();
                        s.restart_audio();
                    },
                )),
        )
        .child(body(
            "Smaller buffers answer faster but need more CPU. 256 is a good start.",
        ))
        .child(
            button("restart-audio", "Restart audio", accent, false).on_press(App::restart_audio),
        );
    let midi = card()
        .child(caps("MIDI"))
        .child(
            text(format!(
                "Output: {}",
                st.midi_out.clone().unwrap_or_else(|| "none".into())
            ))
            .font(font::HEAD)
            .color(theme::text()),
        )
        .child(body("Pick devices on the Capture screen."));
    let project = card()
        .child(caps("Project"))
        .child(
            text(
                s.path
                    .as_ref()
                    .map_or("Not saved yet".into(), |p| p.display().to_string()),
            )
            .font(font::BODY)
            .color(theme::text())
            .wrap(),
        )
        .child(
            row()
                .gap(8.0)
                .items_center()
                .child(text("Autosave").font(font::LABEL).color(theme::muted()))
                .child(segmented(
                    "autosave",
                    &["Off", "30 s", "1 min", "5 min"],
                    AUTOSAVE
                        .iter()
                        .position(|a| *a == st.autosave_seconds)
                        .unwrap_or(2),
                    accent,
                    |s: &mut App, i| {
                        s.settings.autosave_seconds = AUTOSAVE[i];
                        let _ = s.settings.save();
                    },
                )),
        )
        .child(
            row()
                .gap(8.0)
                .child(button("save-as", "Save as…", accent, false).on_press_tx(actions::save_as))
                .child(button("open", "Open…", accent, false).on_press_tx(actions::open_dialog))
                .child(button("demo", "New from demo", accent, false).on_press(actions::new_demo)),
        );
    let look = card()
        .child(caps("Look and feel"))
        .child(toggle(
            "calm",
            "Calm UI (no beat glow)",
            st.calm_ui,
            accent,
            |s: &mut App| {
                s.settings.calm_ui = !s.settings.calm_ui;
                let _ = s.settings.save();
            },
        ))
        .child(toggle(
            "metro-set",
            "Metronome",
            s.metronome,
            accent,
            actions::toggle_metronome,
        ));
    let has_key = s.ext.has_key.unwrap_or(false);
    let key_field = text_input(&s.ext.api_key_input, font::BODY.mono())
        .key("api-key-field")
        .single_line()
        .secure()
        .placeholder("sk-ant-…", theme::faint())
        .w(320.0)
        .h(32.0)
        .px(10.0)
        .radius(9.0)
        .fill(theme::panel())
        .border(1.0, theme::line_strong())
        .on_edit(|s: &mut App| &mut s.ext.api_key_input)
        .on_submit(|s: &mut App, _| store_key(s));
    let ai = card()
        .child(caps("AI assistant"))
        .child(
            text(match s.ext.has_key {
                Some(true) => "API key: stored in the macOS Keychain",
                Some(false) => "No API key yet",
                None => "API key: not checked",
            })
            .font(font::HEAD)
            .color(if has_key { theme::ok() } else { theme::muted() }),
        )
        .child(
            row()
                .gap(8.0)
                .items_center()
                .child(key_field)
                .child(button("store-key", "Save key", accent, true).on_press(store_key))
                .child(
                    button("check-key", "Check", accent, false)
                        .on_press(|s: &mut App| s.ext.has_key = Some(aa_assistant::has_api_key())),
                ),
        )
        .child(body(
            "The key never goes into the project file. Get one at console.anthropic.com.",
        ))
        .child(
            row()
                .gap(8.0)
                .items_center()
                .child(text("Model").font(font::LABEL).color(theme::muted()))
                .child(segmented(
                    "model",
                    &["Opus 5.5", "Sonnet 5.5", "Haiku 5.5"],
                    MODELS
                        .iter()
                        .position(|m| *m == st.assistant_model)
                        .unwrap_or(0),
                    accent,
                    |s: &mut App, i| {
                        s.settings.assistant_model = MODELS[i].into();
                        let _ = s.settings.save();
                    },
                )),
        );
    let keys = card().child(caps("Keys")).child(body("Space play/stop · 1–8 game modes · ⌘1–⌘0 screens · ⌘Z / ⇧⌘Z undo/redo · ⌘S save · ⌘O open · ⌘E export to FMOD · ⌘/ assistant · Esc close / stop scenario · F J D K puzzle pads"));
    col()
        .key("settings")
        .w_full()
        .h_full()
        .padding(22.0)
        .gap(16.0)
        .min_h(0.0)
        .child(screen_header(
            "Settings",
            "App-wide preferences. Project settings live in the project itself.",
            accent,
        ))
        .child(
            col()
                .grow(1.0)
                .min_h(0.0)
                .scroll(rune::el::ScrollSpec {
                    stick_bottom: false,
                })
                .child(
                    row()
                        .gap(14.0)
                        .items_start()
                        .flex_wrap()
                        .child(
                            col()
                                .w(460.0)
                                .gap(14.0)
                                .child(audio)
                                .child(midi)
                                .child(look),
                        )
                        .child(
                            col()
                                .w(560.0)
                                .gap(14.0)
                                .child(ai)
                                .child(project)
                                .child(keys),
                        ),
                ),
        )
}

fn store_key(s: &mut App) {
    let key = s.ext.api_key_input.text.trim().to_string();
    if key.is_empty() {
        return;
    }
    match aa_assistant::store_api_key(&key) {
        Ok(()) => {
            s.ext.api_key_input = rune::TextEdit::new("");
            s.ext.has_key = Some(true);
            s.toast("API key saved to the Keychain");
        }
        Err(e) => s.toast_error(&e.to_string()),
    }
}
