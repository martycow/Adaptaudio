//! Learn: the glossary by topic, and mood presets, for someone new to music.

use crate::state::App;
use crate::theme::{self, font};
use crate::widgets::{body, button, caps, card, screen_header, segmented, text_field, vscroll};
use aa_theory::Mood;
use rune::{Color, El, col, row, text};

pub const TABS: [&str; 2] = ["Glossary", "Mood presets"];

pub fn view(s: &App, accent: Color) -> El {
    let tab = s.tabs.learn.min(TABS.len() - 1);
    let page = if tab == 0 {
        glossary(s, accent)
    } else {
        moods(s, accent)
    };
    col()
        .key("learn")
        .w_full()
        .h_full()
        .padding(22.0)
        .gap(16.0)
        .min_h(0.0)
        .child(screen_header(
            "Learn",
            "Every music word in the app, in plain language. Anywhere in the app, rest the pointer on a word to see its meaning.",
            accent,
        ))
        .child(segmented("learn-tab", &TABS, tab, accent, |s: &mut App, i| {
            s.tabs.learn = i
        }))
        .child(page)
}

/// Topics on the left, the words of one topic on the right. Typing searches every topic.
fn glossary(s: &App, accent: Color) -> El {
    let query = s.glossary_query.text.trim().to_string();
    let all = crate::glossary::entries();
    let mut sections: Vec<&str> = Vec::new();
    for e in all {
        if !sections.contains(&e.section.as_str()) {
            sections.push(&e.section);
        }
    }
    let searching = !query.is_empty();
    let current = s
        .tabs
        .learn_section
        .as_deref()
        .filter(|c| sections.contains(c))
        .or(sections.first().copied())
        .unwrap_or("");

    let mut nav = col().gap(2.0).child(caps("Topics").mb(4.0));
    for (i, name) in sections.iter().enumerate() {
        let on = !searching && *name == current;
        let count = all.iter().filter(|e| e.section == *name).count();
        let pick = name.to_string();
        nav = nav.child(
            row()
                .key(format!("topic-{i}"))
                .role("tab")
                .checked(on)
                .label(name.to_string())
                .h(32.0)
                .px(10.0)
                .gap(8.0)
                .items_center()
                .radius(8.0)
                .fill(if on {
                    accent.alpha(0.14)
                } else {
                    theme::card()
                })
                .hover(theme::hover())
                .on_press(move |s: &mut App| {
                    s.tabs.learn_section = Some(pick.clone());
                    s.glossary_query = rune::TextEdit::new("");
                })
                .child(
                    text(name.to_string())
                        .font(font::LABEL)
                        .color(if on { theme::text() } else { theme::muted() })
                        .truncate()
                        .grow(1.0)
                        .min_w(0.0),
                )
                .child(
                    text(count.to_string())
                        .font(font::SMALL)
                        .color(theme::faint()),
                ),
        );
    }

    let found = crate::glossary::search(&query);
    let shown: Vec<_> = found
        .into_iter()
        .filter(|e| searching || e.section == current)
        .take(120)
        .collect();
    let mut list = col().gap(0.0).max_w(780.0);
    list = list.child(
        col().gap(4.0).pb(10.0).child(
            text(if searching {
                format!("Found {} for “{query}”", shown.len())
            } else {
                current.to_string()
            })
            .font(font::TITLE)
            .color(theme::text()),
        ),
    );
    for e in &shown {
        let mut head = row()
            .gap(10.0)
            .items_center()
            .child(text(e.term.clone()).font(font::HEAD).color(theme::text()));
        if searching {
            head = head.child(
                text(e.section.clone())
                    .font(font::SMALL)
                    .color(theme::faint()),
            );
        }
        list = list.child(
            col()
                .gap(4.0)
                .py(12.0)
                .border_sides([0.0, 1.0, 0.0, 0.0], theme::line())
                .child(head)
                .child(
                    text(e.text.clone())
                        .font(font::BODY)
                        .color(theme::muted())
                        .wrap()
                        .selectable(true),
                ),
        );
    }
    if shown.is_empty() {
        list = list.child(body(
            "Nothing found. Try a shorter word, in English or Russian.",
        ));
    }

    row()
        .grow(1.0)
        .min_h(0.0)
        .gap(18.0)
        .child(
            col()
                .w(250.0)
                .h_full()
                .min_h(0.0)
                .gap(12.0)
                .child(text_field(
                    "glossary",
                    &s.glossary_query,
                    "Search: bpm, сайдчейн…",
                    250.0,
                    |s: &mut App| &mut s.glossary_query,
                ))
                .child(
                    col()
                        .grow(1.0)
                        .min_h(0.0)
                        .radius(12.0)
                        .fill(theme::card())
                        .child(vscroll(nav.padding(10.0))),
                ),
        )
        .child(vscroll(list.pr(16.0)))
}

/// Ready-made starting points by feeling. Applying one changes only the tempo and the scale.
fn moods(s: &App, accent: Color) -> El {
    let p = &s.project;
    let mut grid = row().gap(14.0).flex_wrap().items_start();
    for mood in Mood::ALL {
        let preset = aa_theory::mood_preset(mood);
        let bpm = ((preset.tempo_bpm[0] + preset.tempo_bpm[1]) * 0.5).round();
        let applied = (p.tempo_bpm - bpm).abs() < 0.5 && p.scale == preset.scale;
        grid = grid.child(
            card()
                .w(340.0)
                .child(text(mood.name()).font(font::TITLE).color(theme::text()))
                .child(body(preset.description.to_string()))
                .child(fact(
                    "Tempo",
                    format!("{:.0}–{:.0} BPM", preset.tempo_bpm[0], preset.tempo_bpm[1]),
                ))
                .child(fact("Scale", preset.scale.name().to_string()))
                .child(fact(
                    "Tracks",
                    preset
                        .roles
                        .iter()
                        .map(|r| r.name())
                        .collect::<Vec<_>>()
                        .join(", "),
                ))
                .child(fact("Sound", preset.synth_character.to_string()))
                .child(
                    button(
                        format!("mood-{}", mood.name()),
                        if applied {
                            "✓ In use".to_string()
                        } else {
                            format!("Set {bpm:.0} BPM and {}", preset.scale.name())
                        },
                        accent,
                        !applied,
                    )
                    .disabled(applied)
                    .on_press(move |s: &mut App| {
                        let p = aa_theory::mood_preset(mood);
                        let bpm = ((p.tempo_bpm[0] + p.tempo_bpm[1]) * 0.5).round();
                        s.edit(|pr| {
                            pr.tempo_bpm = bpm;
                            pr.scale = p.scale;
                        });
                        s.toast(&format!(
                            "{}: {bpm:.0} BPM, {} (⌘Z to undo)",
                            mood.name(),
                            p.scale.name()
                        ));
                    }),
                ),
        );
    }
    col()
        .grow(1.0)
        .min_h(0.0)
        .gap(12.0)
        .child(
            body("A mood is a recipe: a tempo range, a scale, and which tracks and sounds suit it. The button changes this project's tempo and scale to match; the tracks and sounds are advice to follow by hand.")
                .max_w(860.0),
        )
        .child(vscroll(grid))
}

fn fact(name: &str, value: String) -> El {
    row()
        .gap(10.0)
        .items_start()
        .child(
            text(name.to_string())
                .font(font::SMALL)
                .color(theme::faint())
                .w(48.0),
        )
        .child(
            text(value)
                .font(font::SMALL)
                .color(theme::muted())
                .wrap()
                .grow(1.0)
                .min_w(0.0),
        )
}
