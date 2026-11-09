//! The project chooser: shown when the app starts and by File → New. Pick a recent project,
//! open one from disk, or make a new one.

use crate::actions;
use crate::state::App;
use crate::theme::{self, font};
use crate::widgets::{body, button, caps, dropdown, field_row, segmented, text_field};
use aa_core::{PitchClass, ScaleKind};
use rune::{Color, El, col, el, row, text};

pub const TEMPLATES: [&str; 2] = ["Empty project", "Cedar Station demo"];

#[derive(Clone, Debug)]
pub struct Wizard {
    /// False: the list of projects. True: the "new project" form.
    pub creating: bool,
    pub name: rune::TextEdit,
    pub template: usize,
    pub tempo_bpm: f32,
    pub key: PitchClass,
    pub scale: ScaleKind,
}

impl Wizard {
    pub fn start() -> Self {
        Wizard {
            creating: false,
            name: rune::TextEdit::new("New piece"),
            template: 0,
            tempo_bpm: 120.0,
            key: PitchClass(2),
            scale: ScaleKind::NaturalMinor,
        }
    }

    pub fn new_project() -> Self {
        Wizard {
            creating: true,
            ..Wizard::start()
        }
    }

    /// The project this form describes.
    pub fn build(&self) -> aa_core::Project {
        let name = match self.name.text.trim() {
            "" => "New piece",
            n => n,
        };
        if self.template == 1 {
            let mut p = aa_core::demo::cedar_station();
            p.name = name.to_string();
            return p;
        }
        let mut p = aa_core::demo::blank_station(name);
        p.tempo_bpm = self.tempo_bpm;
        p.key = self.key;
        p.scale = self.scale;
        p
    }
}

pub fn view(s: &App, w: &Wizard, accent: Color) -> El {
    let page = if w.creating {
        create(s, w, accent)
    } else {
        choose(s, accent)
    };
    let card = col()
        .key("wizard")
        .role("dialog")
        .label("Choose a project")
        .w(620.0)
        .max_h(640.0)
        .padding(28.0)
        .gap(18.0)
        .radius(16.0)
        .fill(theme::card())
        .border(1.0, theme::line_strong())
        .shadow(18.0, 60.0, theme::shadow())
        .transition(rune::Transition::offset(0.0, 14.0))
        .child(page);
    el().key("wizard-backdrop")
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .fill(theme::bg().alpha(0.94))
        .on_press(|_: &mut App| {})
        .child(col().w_full().h_full().center().child(card))
}

fn choose(s: &App, accent: Color) -> El {
    let mut c = col()
        .gap(18.0)
        .child(
            col()
                .gap(4.0)
                .child(text("Adaptaudio").font(font::CAPS).color(accent))
                .child(
                    text("Which project?")
                        .font(font::HERO)
                        .color(theme::text()),
                )
                .child(body(
                    "Open a project you worked on, or start a new one. You can come back here with File → New.",
                )),
        );
    let recent: Vec<_> = s
        .settings
        .recent_projects
        .iter()
        .filter(|p| p.exists())
        .take(5)
        .collect();
    if !recent.is_empty() {
        let mut list = col().gap(4.0).child(caps("Recent"));
        for (i, path) in recent.into_iter().enumerate() {
            let name = path
                .file_stem()
                .map_or_else(|| "Project".into(), |n| n.to_string_lossy().to_string());
            let p = path.clone();
            list = list.child(
                row()
                    .key(format!("wizard-recent-{i}"))
                    .role("button")
                    .label(format!("Open {name}"))
                    .h(44.0)
                    .px(12.0)
                    .gap(10.0)
                    .items_center()
                    .radius(10.0)
                    .fill(theme::panel())
                    .hover(theme::hover())
                    .press_scale(0.98)
                    .on_press(move |s: &mut App| {
                        actions::open(s, &p);
                    })
                    .child(text("▦").font(font::HEAD).color(accent))
                    .child(text(name).font(font::HEAD).color(theme::text()))
                    .child(
                        text(path.display().to_string())
                            .font(font::SMALL)
                            .color(theme::faint())
                            .truncate()
                            .grow(1.0)
                            .min_w(0.0),
                    ),
            );
        }
        c = c.child(list);
    }
    c.child(
        row()
            .gap(8.0)
            .child(
                button("wizard-new", "＋ New project", accent, true)
                    .on_press(|s: &mut App| s.wizard = Some(Wizard::new_project())),
            )
            .child(
                button("wizard-open", "Open from disk…", accent, false)
                    .on_press_tx(actions::open_dialog),
            )
            .child(rune::spacer())
            .child(
                button("wizard-demo", "Look at the demo", accent, false)
                    .help("Cedar Station — Night Shift: a finished example to explore")
                    .on_press(|s: &mut App| {
                        actions::new_demo(s);
                        s.wizard = None;
                    }),
            ),
    )
}

fn create(s: &App, w: &Wizard, accent: Color) -> El {
    let keys: Vec<&str> = (0..12).map(|i| PitchClass(i).name()).collect();
    let scales: Vec<&str> = ScaleKind::ALL.iter().map(|k| k.name()).collect();
    let demo = w.template == 1;
    let mut form = col()
        .gap(12.0)
        .child(field_row(
            "Name",
            text_field("wizard-name", &w.name, "New piece", 320.0, |s: &mut App| {
                &mut s.wizard.get_or_insert_with(Wizard::new_project).name
            }),
        ))
        .child(field_row(
            "Start from",
            segmented(
                "wizard-template",
                &TEMPLATES,
                w.template,
                accent,
                |s: &mut App, i| {
                    if let Some(w) = s.wizard.as_mut() {
                        w.template = i
                    }
                },
            ),
        ))
        .child(body(if demo {
            "A copy of the finished demo: every track, clip and rule is there to take apart."
        } else {
            "The game's modes, sections and events are ready; tracks and notes are yours to add."
        }));
    if !demo {
        form = form
            .child(field_row(
                "Tempo",
                segmented(
                    "wizard-tempo",
                    &["70", "90", "110", "120", "140", "160"],
                    [70.0, 90.0, 110.0, 120.0, 140.0, 160.0]
                        .iter()
                        .position(|b: &f32| (*b - w.tempo_bpm).abs() < 0.5)
                        .unwrap_or(3),
                    accent,
                    |s: &mut App, i| {
                        if let Some(w) = s.wizard.as_mut() {
                            w.tempo_bpm = [70.0, 90.0, 110.0, 120.0, 140.0, 160.0][i]
                        }
                    },
                ),
            ))
            .child(field_row(
                "Key",
                dropdown(
                    s,
                    "wizard-key",
                    &keys,
                    Some(usize::from(w.key.0)),
                    120.0,
                    |s: &mut App, i, _| {
                        if let Some(w) = s.wizard.as_mut() {
                            w.key = PitchClass::new(i as i32)
                        }
                    },
                ),
            ))
            .child(field_row(
                "Scale",
                dropdown(
                    s,
                    "wizard-scale",
                    &scales,
                    ScaleKind::ALL.iter().position(|k| *k == w.scale),
                    220.0,
                    |s: &mut App, i, _| {
                        if let Some(w) = s.wizard.as_mut() {
                            w.scale = ScaleKind::ALL[i]
                        }
                    },
                ),
            ))
            .child(
                text(format!("{} — {}", w.scale.name(), w.scale.mood()))
                    .font(font::SMALL)
                    .color(theme::faint()),
            );
    }
    col()
        .gap(18.0)
        .child(
            col()
                .gap(4.0)
                .child(text("New project").font(font::HERO).color(theme::text()))
                .child(body(
                    "Everything here can be changed later. After Create, choose where to keep the project.",
                )),
        )
        .child(form)
        .child(
            row()
                .gap(8.0)
                .child(
                    button("wizard-back", "Back", accent, false)
                        .on_press(|s: &mut App| s.wizard = Some(Wizard::start())),
                )
                .child(rune::spacer())
                .child(
                    button("wizard-create", "Create", accent, true)
                        .on_press_tx(actions::create_from_wizard),
                ),
        )
}
