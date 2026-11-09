//! Settings: audio, MIDI, project behaviour, the AI assistant, look, updates and keys.

use crate::actions;
use crate::state::App;
use crate::theme::{self, font};
use crate::update::{self, Release};
use crate::widgets::{
    body, button, caps, card, dropdown, field_row, screen_header, segmented, text_field, toggle,
    vscroll,
};
use aa_assistant::Provider;
use rune::{Color, El, Tx, col, el, row, text};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

pub const TABS: [&str; 7] = [
    "Audio",
    "MIDI",
    "Project",
    "Assistant",
    "Appearance",
    "Updates",
    "Shortcuts",
];
const BUFFERS: [Option<u32>; 5] = [None, Some(128), Some(256), Some(512), Some(1024)];
const AUTOSAVE: [u32; 4] = [0, 30, 60, 300];
const CLAUDE_MODELS: [&str; 3] = ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"];

// ---------------------------------------------------------------------------------- updates

#[derive(Clone, Debug, Default, PartialEq)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Installing(Release),
    /// Installed; the app restarts into the new version.
    Installed,
    Failed(String),
}

pub enum UpdateJob {
    Checked(Result<Option<Release>, String>),
    Installed(Result<std::path::PathBuf, String>),
}

#[derive(Default)]
pub struct UpdateUi {
    pub status: UpdateStatus,
    rx: Option<Receiver<UpdateJob>>,
    /// A check the user did not ask for stays quiet unless it finds something.
    quiet: bool,
}

/// Asks GitHub for the latest release. `quiet` is the check at launch.
pub fn check_updates(s: &mut App, quiet: bool) {
    let repo = s.settings.update_repo.trim().to_string();
    if s.ext.update.rx.is_some() {
        return;
    }
    if repo.is_empty() {
        if !quiet {
            s.ext.update.status =
                UpdateStatus::Failed("Enter the GitHub repository that publishes releases.".into());
        }
        return;
    }
    let (tx, rx) = channel();
    let wake = s.live.waker_fn();
    s.ext.update.rx = Some(rx);
    s.ext.update.quiet = quiet;
    s.ext.update.status = UpdateStatus::Checking;
    std::thread::spawn(move || {
        let _ = tx.send(UpdateJob::Checked(update::check(&repo)));
        wake();
    });
}

fn install_update(s: &mut App) {
    let UpdateStatus::Available(release) = s.ext.update.status.clone() else {
        return;
    };
    let Some(bundle) = update::current_bundle() else {
        return;
    };
    let (tx, rx) = channel();
    let wake = s.live.waker_fn();
    s.ext.update.rx = Some(rx);
    s.ext.update.status = UpdateStatus::Installing(release.clone());
    std::thread::spawn(move || {
        let result = update::install(&release, &bundle).map(|()| bundle);
        let _ = tx.send(UpdateJob::Installed(result));
        wake();
    });
}

pub fn poll(s: &mut App, tx: &mut Tx) -> bool {
    let Some(rx) = &s.ext.update.rx else {
        return false;
    };
    let job = match rx.try_recv() {
        Ok(job) => job,
        Err(TryRecvError::Empty) => {
            tx.wake_after(Duration::from_millis(300));
            return false;
        }
        Err(TryRecvError::Disconnected) => {
            s.ext.update.rx = None;
            s.ext.update.status = UpdateStatus::Idle;
            return true;
        }
    };
    s.ext.update.rx = None;
    let quiet = std::mem::take(&mut s.ext.update.quiet);
    match job {
        UpdateJob::Checked(Ok(Some(release))) => {
            s.toast(&format!(
                "Adaptaudio {} is available: Settings → Updates",
                release.version
            ));
            s.ext.update.status = UpdateStatus::Available(release);
        }
        UpdateJob::Checked(Ok(None)) => s.ext.update.status = UpdateStatus::UpToDate,
        UpdateJob::Checked(Err(e)) => {
            s.ext.update.status = if quiet {
                UpdateStatus::Idle
            } else {
                UpdateStatus::Failed(e)
            }
        }
        UpdateJob::Installed(Ok(bundle)) => {
            // Never restart over unsaved work: the new version starts on the next launch.
            if s.dirty {
                s.ext.update.status = UpdateStatus::Installed;
                s.toast("The update is installed. Save your project and restart Adaptaudio.");
            } else {
                match update::relaunch(&bundle) {
                    Ok(()) => tx.quit(),
                    Err(e) => s.ext.update.status = UpdateStatus::Failed(e),
                }
            }
        }
        UpdateJob::Installed(Err(e)) => s.ext.update.status = UpdateStatus::Failed(e),
    }
    true
}

// ---------------------------------------------------------------------------------- view

pub fn view(s: &App, accent: Color) -> El {
    let tab = s.tabs.settings.min(TABS.len() - 1);
    let mut nav = col().w(180.0).gap(2.0);
    for (i, name) in TABS.iter().enumerate() {
        let on = i == tab;
        nav = nav.child(
            row()
                .key(format!("settings-tab-{i}"))
                .role("tab")
                .checked(on)
                .label(name.to_string())
                .h(34.0)
                .px(12.0)
                .items_center()
                .radius(8.0)
                .fill(if on { accent.alpha(0.14) } else { theme::bg() })
                .hover(theme::hover())
                .on_press(move |s: &mut App| {
                    s.tabs.settings = i;
                    s.open_menu = None;
                })
                .child(text(name.to_string()).font(font::LABEL).color(if on {
                    theme::text()
                } else {
                    theme::muted()
                })),
        );
    }
    let page = match tab {
        0 => audio(s, accent),
        1 => midi(s, accent),
        2 => project(s, accent),
        3 => assistant(s, accent),
        4 => appearance(s, accent),
        5 => updates(s, accent),
        _ => shortcuts(),
    };
    col()
        .key("settings")
        .w_full()
        .h_full()
        .padding(22.0)
        .gap(16.0)
        .min_h(0.0)
        .child(screen_header(
            "Settings",
            "Preferences for this computer. What belongs to a piece of music lives in its project.",
            accent,
        ))
        .child(
            row()
                .grow(1.0)
                .min_h(0.0)
                .gap(22.0)
                .child(nav)
                .child(vscroll(col().max_w(680.0).gap(14.0).child(page))),
        )
}

fn status(ok: bool, words: String) -> El {
    row()
        .gap(8.0)
        .items_center()
        .child(
            el().size(8.0, 8.0)
                .radius(4.0)
                .fill(if ok { theme::ok() } else { theme::warn() }),
        )
        .child(text(words).font(font::HEAD).color(theme::text()).wrap())
}

fn switch(key: &str, label: &str, about: &str, on: bool, accent: Color, f: fn(&mut App)) -> El {
    row()
        .gap(12.0)
        .items_center()
        .child(
            toggle(
                key.to_string(),
                if on { "On" } else { "Off" },
                on,
                accent,
                f,
            )
            .w(44.0),
        )
        .child(
            col()
                .gap(2.0)
                .grow(1.0)
                .min_w(0.0)
                .child(
                    text(label.to_string())
                        .font(font::LABEL)
                        .color(theme::text()),
                )
                .child(
                    text(about.to_string())
                        .font(font::SMALL)
                        .color(theme::faint())
                        .wrap(),
                ),
        )
}

fn save(s: &mut App) {
    if let Err(e) = s.settings.save() {
        s.toast_error(&format!("Settings were not saved: {e}"));
    }
}

fn audio(s: &App, accent: Color) -> El {
    let st = &s.settings;
    let mut outs = vec!["System default".to_string()];
    outs.extend(s.ext.devices.audio_out.iter().cloned());
    let current = st
        .audio_device
        .as_ref()
        .map_or(Some(0), |d| outs.iter().position(|o| o == d));
    let running = s.audio.as_ref().map_or_else(
        || {
            format!(
                "Sound is off: {}",
                s.audio_error.clone().unwrap_or_else(|| "no device".into())
            )
        },
        |a| {
            format!(
                "Playing through {} · {} Hz · {} channels",
                a.device_name, a.sample_rate, a.channels
            )
        },
    );
    let buffers = ["Device default", "128", "256", "512", "1024"];
    let mut out = card()
        .child(caps("Sound output"))
        .child(status(s.audio.is_some(), running))
        .child(field_row(
            "Output device",
            dropdown(s, "set-audio-out", &outs, current, 300.0, |s: &mut App, i, _| {
                s.settings.audio_device = (i > 0)
                    .then(|| s.ext.devices.audio_out.get(i - 1).cloned())
                    .flatten();
                save(s);
                s.restart_audio();
            }),
        ))
        .child(field_row(
            "Buffer",
            dropdown(
                s,
                "set-buffer",
                &buffers,
                BUFFERS.iter().position(|b| *b == st.buffer_frames),
                160.0,
                |s: &mut App, i, _| {
                    s.settings.buffer_frames = BUFFERS[i];
                    save(s);
                    s.restart_audio();
                },
            ),
        ))
        .child(body(
            "The buffer is how much sound is prepared ahead. Smaller answers faster but needs more of the computer; if you hear crackles, pick a bigger one. 256 is a good start.",
        ))
        .child(
            button("restart-audio", "Restart sound", accent, false)
                .help("Opens the output again, for example after plugging in an interface")
                .on_press(App::restart_audio),
        );
    if let Some(e) = s.audio.as_ref().and(s.audio_error.as_ref()) {
        out = out.child(
            text(e.clone())
                .font(font::SMALL)
                .color(theme::danger())
                .wrap(),
        );
    }
    let input = card()
        .child(caps("Sound input (for Capture)"))
        .child(
            text(format!(
                "{} · {}",
                st.audio_in
                    .clone()
                    .unwrap_or_else(|| "System default input".into()),
                ["both inputs", "input 1", "input 2"][usize::from(st.audio_in_channel.min(2))]
            ))
            .font(font::HEAD)
            .color(theme::text()),
        )
        .child(body(
            "Choose the input and its channel on the Capture screen, next to the Record button.",
        ));
    col().gap(14.0).child(out).child(input)
}

fn midi(s: &App, accent: Color) -> El {
    let st = &s.settings;
    let outs = &s.ext.devices.midi_out;
    let out_ok = s.ext.capture.scheduler.is_some();
    let out_words = match (&st.midi_out, out_ok) {
        (Some(p), true) => format!("Sending notes to {p}"),
        (Some(p), false) => format!("{p} is not connected"),
        (None, _) => {
            "No MIDI output yet: plug in a synth and it is picked up by itself".to_string()
        }
    };
    let mut output = card()
        .child(caps("MIDI output: Adaptaudio plays a hardware synth"))
        .child(status(out_ok, out_words))
        .child(field_row(
            "Output",
            dropdown(
                s,
                "set-midi-out",
                outs,
                st.midi_out.as_ref().and_then(|m| outs.iter().position(|o| o == m)),
                300.0,
                |s: &mut App, i, _| {
                    if let Some(name) = s.ext.devices.midi_out.get(i).cloned() {
                        s.settings.midi_out = Some(name);
                        save(s);
                        s.restart_audio();
                    }
                },
            ),
        ))
        .child(body(
            "Tracks set to “Hardware (MIDI)” in the Sound Lab send their notes here. Devices are looked for every two seconds; there is nothing to press.",
        ));
    if let Some(e) = &s.midi_error {
        output = output.child(
            text(e.clone())
                .font(font::SMALL)
                .color(theme::danger())
                .wrap(),
        );
    }
    let mut ins = vec!["Automatic".to_string()];
    ins.extend(s.ext.devices.midi_in.iter().cloned());
    let kb = s.ext.devices.keyboard_port();
    let input = card()
        .child(caps("MIDI keyboard: a hardware synth plays Adaptaudio"))
        .child(status(
            kb.is_some(),
            kb.map_or_else(
                || "No keyboard connected".to_string(),
                |p| format!("Listening to {p}"),
            ),
        ))
        .child(field_row(
            "Keyboard",
            dropdown(
                s,
                "set-midi-in",
                &ins,
                st.midi_in
                    .as_ref()
                    .map_or(Some(0), |m| ins.iter().position(|o| o == m)),
                300.0,
                |s: &mut App, i, _| {
                    s.settings.midi_in = (i > 0)
                        .then(|| s.ext.devices.midi_in.get(i - 1).cloned())
                        .flatten();
                    save(s);
                    crate::devices::reconnect_keyboard(s);
                },
            ),
        ))
        .child(body(
            "Keys pressed on the synth play the track selected in Adaptaudio, with its sound. Tracks set to “Hardware (MIDI)” are skipped: there the synth already plays itself.",
        ));
    let _ = accent;
    col().gap(14.0).child(output).child(input)
}

fn project(s: &App, accent: Color) -> El {
    let st = &s.settings;
    let autosave = ["Off", "Every 30 seconds", "Every minute", "Every 5 minutes"];
    let files = card()
        .child(caps("This project"))
        .child(
            text(
                s.path
                    .as_ref()
                    .map_or("Not saved yet".into(), |p| p.display().to_string()),
            )
            .font(font::BODY)
            .color(theme::text())
            .wrap()
            .selectable(true),
        )
        .child(
            row()
                .gap(8.0)
                .child(button("set-new", "New…", accent, false).on_press_tx(actions::new_project))
                .child(button("set-open", "Open…", accent, false).on_press_tx(actions::open_dialog))
                .child(button("set-save", "Save", accent, false).on_press_tx(actions::save))
                .child(button("set-save-as", "Save as…", accent, false).on_press_tx(actions::save_as)),
        )
        .child(field_row(
            "Autosave",
            dropdown(
                s,
                "set-autosave",
                &autosave,
                AUTOSAVE.iter().position(|a| *a == st.autosave_seconds),
                220.0,
                |s: &mut App, i, _| {
                    s.settings.autosave_seconds = AUTOSAVE[i];
                    save(s);
                },
            ),
        ))
        .child(body("Autosave writes into the project's own folder, so it only works once the project has been saved."));
    let behaviour = card()
        .child(caps("Behaviour"))
        .child(switch(
            "set-ask",
            "Ask which project to open at launch",
            "Off: the last project opens by itself.",
            st.ask_project_on_start,
            accent,
            |s| {
                s.settings.ask_project_on_start = !s.settings.ask_project_on_start;
                save(s);
            },
        ))
        .child(switch(
            "set-launch",
            "Start playing when a section is launched",
            "Clicking a section header on the Session screen also presses Play.",
            st.play_on_launch,
            accent,
            |s| {
                s.settings.play_on_launch = !s.settings.play_on_launch;
                save(s);
            },
        ))
        .child(switch(
            "set-preview",
            "Hear a note when it is drawn",
            "The editor plays each note as you add it.",
            st.preview_notes,
            accent,
            |s| {
                s.settings.preview_notes = !s.settings.preview_notes;
                save(s);
            },
        ));
    let export = card()
        .child(caps("Export to FMOD"))
        .child(
            text(st.export_dir.as_ref().map_or(
                "No export yet: the first export asks for a folder".to_string(),
                |d| format!("Last exported to {}", d.display()),
            ))
            .font(font::BODY)
            .color(theme::text())
            .wrap(),
        )
        .child(
            button("set-export", "Export now…", accent, false).on_press_tx(actions::export_dialog),
        );
    col().gap(14.0).child(files).child(behaviour).child(export)
}

fn provider_of(s: &App) -> Provider {
    Provider::from_id(&s.settings.assistant_provider).unwrap_or_default()
}

fn assistant(s: &App, accent: Color) -> El {
    let provider = provider_of(s);
    let names: Vec<&str> = Provider::ALL.iter().map(|p| p.name()).collect();
    let mut c = card().child(caps("Who answers")).child(field_row(
        "Provider",
        dropdown(
            s,
            "set-provider",
            &names,
            Provider::ALL.iter().position(|p| *p == provider),
            220.0,
            |s: &mut App, i, _| {
                let p = Provider::ALL[i];
                s.settings.assistant_provider = p.id().to_string();
                s.settings.assistant_model = p.default_model().to_string();
                s.settings.assistant_base_url = String::new();
                s.ext.model_input = rune::TextEdit::new(p.default_model());
                s.ext.base_url_input = rune::TextEdit::new("");
                s.ext.has_key = None;
                save(s);
            },
        ),
    ));
    if provider == Provider::Anthropic {
        c = c.child(field_row(
            "Model",
            segmented(
                "model",
                &["Opus 5.5", "Sonnet 5.5", "Haiku 5.5"],
                CLAUDE_MODELS
                    .iter()
                    .position(|m| *m == s.settings.assistant_model)
                    .unwrap_or(0),
                accent,
                |s: &mut App, i| {
                    s.settings.assistant_model = CLAUDE_MODELS[i].into();
                    save(s);
                },
            ),
        ));
    } else {
        c = c
            .child(field_row(
                "Model",
                text_field(
                    "set-model",
                    &s.ext.model_input,
                    provider.default_model(),
                    300.0,
                    |s: &mut App| &mut s.ext.model_input,
                )
                .on_submit(|s: &mut App, _| apply_model(s)),
            ))
            .child(field_row(
                "API address",
                text_field(
                    "set-base-url",
                    &s.ext.base_url_input,
                    provider.default_base_url(),
                    420.0,
                    |s: &mut App| &mut s.ext.base_url_input,
                )
                .on_submit(|s: &mut App, _| apply_model(s)),
            ))
            .child(
                row()
                    .gap(10.0)
                    .items_center()
                    .child(button("set-model-apply", "Apply", accent, true).on_press(apply_model))
                    .child(
                        text(format!(
                            "In use: {} at {}",
                            s.settings.assistant_model,
                            if s.settings.assistant_base_url.is_empty() {
                                provider.default_base_url()
                            } else {
                                &s.settings.assistant_base_url
                            }
                        ))
                        .font(font::SMALL)
                        .color(theme::faint())
                        .truncate(),
                    ),
            )
            .child(body(match provider {
                Provider::OpenAi => "Any OpenAI model that supports tools over Chat Completions works; type its name exactly as OpenAI lists it. Leave the address empty for OpenAI itself.",
                _ => "Qwen models through Alibaba DashScope. The default address is the international one; accounts from mainland China use dashscope.aliyuncs.com instead.",
            }));
    }
    let has_key = s.ext.has_key;
    let key_field = rune::text_input(&s.ext.api_key_input, font::BODY.mono())
        .key("api-key-field")
        .single_line()
        .secure()
        .placeholder(provider.key_hint(), theme::faint())
        .w(320.0)
        .px(10.0)
        .py(7.0)
        .radius(8.0)
        .fill(theme::panel())
        .border(1.0, theme::line_strong())
        .on_edit(|s: &mut App| &mut s.ext.api_key_input)
        .on_submit(|s: &mut App, _| store_key(s));
    let key = card()
        .child(caps(format!("{} API key", provider.name())))
        .child(status(
            has_key == Some(true),
            match has_key {
                Some(true) => "A key is stored in the macOS Keychain".to_string(),
                Some(false) => "No key stored yet".to_string(),
                None => "Not checked yet".to_string(),
            },
        ))
        .child(
            row()
                .gap(8.0)
                .items_center()
                .child(key_field)
                .child(button("store-key", "Save key", accent, true).on_press(store_key))
                .child(
                    button("check-key", "Check", accent, false).on_press(|s: &mut App| {
                        s.ext.has_key = Some(aa_assistant::has_api_key_for(provider_of(s)))
                    }),
                ),
        )
        .child(body(
            "The key is kept in the macOS Keychain, one per provider. It never goes into a project or a settings file.",
        ));
    col().gap(14.0).child(c).child(key)
}

fn apply_model(s: &mut App) {
    let provider = provider_of(s);
    let model = s.ext.model_input.text.trim().to_string();
    s.settings.assistant_model = if model.is_empty() {
        provider.default_model().to_string()
    } else {
        model
    };
    s.settings.assistant_base_url = s.ext.base_url_input.text.trim().to_string();
    save(s);
    s.toast(&format!(
        "Assistant: {}",
        s.settings.assistant_model.clone()
    ));
}

fn store_key(s: &mut App) {
    let key = s.ext.api_key_input.text.trim().to_string();
    if key.is_empty() {
        return;
    }
    match aa_assistant::store_api_key_for(provider_of(s), &key) {
        Ok(()) => {
            s.ext.api_key_input = rune::TextEdit::new("");
            s.ext.has_key = Some(true);
            s.toast("API key saved to the Keychain");
        }
        Err(e) => s.toast_error(&e.to_string()),
    }
}

fn appearance(s: &App, accent: Color) -> El {
    card()
        .child(caps("Look and feel"))
        .child(switch(
            "calm",
            "Calm interface",
            "Turns off the soft glow around the window that pulses with the beat.",
            s.settings.calm_ui,
            accent,
            |s| {
                s.settings.calm_ui = !s.settings.calm_ui;
                save(s);
            },
        ))
        .child(switch(
            "metro-set",
            "Metronome",
            "A click on every beat while the music plays. Also the Click button next to Play.",
            s.metronome,
            accent,
            actions::toggle_metronome,
        ))
        .child(body(
            "The accent colour follows the game mode that is playing, so the window changes mood with the music.",
        ))
}

fn updates(s: &App, accent: Color) -> El {
    let up = &s.ext.update;
    let bundle = update::current_bundle();
    let busy = matches!(
        up.status,
        UpdateStatus::Checking | UpdateStatus::Installing(_)
    );
    let words = match &up.status {
        UpdateStatus::Idle => "Not checked yet".to_string(),
        UpdateStatus::Checking => "Checking…".to_string(),
        UpdateStatus::UpToDate => "This is the latest version".to_string(),
        UpdateStatus::Available(r) => format!("Version {} is available", r.version),
        UpdateStatus::Installing(r) => format!("Downloading and installing {}…", r.version),
        UpdateStatus::Installed => "Installed: restart Adaptaudio to use it".to_string(),
        UpdateStatus::Failed(e) => e.clone(),
    };
    let ok = !matches!(up.status, UpdateStatus::Failed(_));
    let mut c = card()
        .child(caps("Updates"))
        .child(
            text(format!("Adaptaudio {}", update::CURRENT_VERSION))
                .font(font::TITLE)
                .color(theme::text()),
        )
        .child(status(ok, words))
        .child(field_row(
            "Repository",
            row()
                .gap(8.0)
                .items_center()
                .child(
                    text_field(
                        "set-repo",
                        &s.ext.repo_input,
                        "owner/name on GitHub",
                        280.0,
                        |s: &mut App| &mut s.ext.repo_input,
                    )
                    .on_submit(|s: &mut App, _| apply_repo(s)),
                )
                .child(button("set-repo-apply", "Check now", accent, true).disabled(busy).on_press(apply_repo)),
        ))
        .child(body(
            "Updates come from this repository's GitHub Releases. Each download is checked against the checksum published with it before anything is replaced.",
        ))
        .child(switch(
            "set-auto-update",
            "Check when Adaptaudio starts",
            "Only looks; installing always waits for your click.",
            s.settings.check_updates_on_start,
            accent,
            |s| {
                s.settings.check_updates_on_start = !s.settings.check_updates_on_start;
                save(s);
            },
        ));
    if let UpdateStatus::Available(r) = &up.status {
        let page = r.page_url.clone();
        let mut actions = row().gap(8.0).items_center();
        if bundle.is_some() {
            actions = actions.child(
                button(
                    "update-install",
                    format!("Install {} and restart", r.version),
                    accent,
                    true,
                )
                .on_press(install_update),
            );
        }
        actions = actions.child(
            button(
                "update-page",
                "Open the release page",
                accent,
                bundle.is_none(),
            )
            .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                if let Err(e) = tx.open_url(&page) {
                    s.toast_error(&format!("Can't open the page: {e:?}"));
                }
            }),
        );
        c = c.child(actions);
        if !r.notes.trim().is_empty() {
            c = c.child(caps("What is new")).child(body(r.notes.clone()));
        }
        if s.dirty && bundle.is_some() {
            c = c.child(
                text("There are unsaved changes: the app will not restart until you save.")
                    .font(font::SMALL)
                    .color(theme::warn()),
            );
        }
    }
    if bundle.is_none() {
        c = c.child(body(
            "This copy was started from source, not from Adaptaudio.app, so it can't replace itself: it can only tell you a new version exists. Build the app with tools/bundle-macos/bundle.sh.",
        ));
    }
    c
}

fn apply_repo(s: &mut App) {
    let typed = s.ext.repo_input.text.trim().to_string();
    let repo = if typed.is_empty() {
        String::new()
    } else {
        match update::parse_repo(&typed) {
            Some(repo) => repo,
            None => {
                s.ext.update.status = UpdateStatus::Failed(
                    "That is not a GitHub repository. Write it as owner/name.".into(),
                );
                return;
            }
        }
    };
    s.ext.repo_input = rune::TextEdit::new(repo.clone());
    s.settings.update_repo = repo;
    save(s);
    check_updates(s, false);
}

fn shortcuts() -> El {
    let rows = [
        ("Space", "Play or stop"),
        ("1 – 8", "Switch to a game mode"),
        ("⌘N", "New project"),
        ("⌘O", "Open a project"),
        ("⌘S  /  ⇧⌘S", "Save  /  save as"),
        ("⌘E", "Export to FMOD"),
        ("⌘Z  /  ⇧⌘Z", "Undo  /  redo"),
        ("⌘1 – ⌘8", "Go to a screen"),
        ("⌘/", "Open the assistant"),
        (
            "Esc",
            "Close a list, the editor, the assistant; stop a scenario",
        ),
    ];
    let mut c = card().child(caps("Keys"));
    for (keys, what) in rows {
        c = c.child(
            row()
                .gap(14.0)
                .items_center()
                .child(text(keys).font(font::NUM).color(theme::text()).w(130.0))
                .child(text(what).font(font::BODY).color(theme::muted())),
        );
    }
    c.child(body(
        "Every command is also in the menu bar at the top of the screen.",
    ))
}
