//! Learn: the glossary and mood presets, for someone new to music.

use crate::state::App;
use crate::theme::{self, font};
use crate::widgets::{body, button, caps, card, screen_header};
use aa_theory::Mood;
use rune::{Color, El, col, row, text, text_input};

pub fn view(s: &App, accent: Color) -> El {
    let query = s.glossary_query.text.clone();
    let field = text_input(&s.glossary_query, font::BODY)
        .key("glossary-field")
        .single_line()
        .placeholder("Search: bpm, сайдчейн, filter…", theme::faint())
        .w(360.0)
        .px(12.0)
        .py(9.0)
        .radius(10.0)
        .fill(theme::card())
        .border(1.0, theme::line_strong())
        .on_edit(|s: &mut App| &mut s.glossary_query);
    let entries = crate::glossary::search(&query);
    let mut list = col().gap(8.0);
    let mut section = String::new();
    for e in entries.iter().take(140) {
        if e.section != section {
            section = e.section.clone();
            list = list.child(caps(section.clone()).mt(8.0));
        }
        list = list.child(
            col()
                .gap(3.0)
                .padding(10.0)
                .radius(10.0)
                .fill(theme::card())
                .border(1.0, theme::line())
                .child(text(e.term.clone()).font(font::HEAD).color(theme::text()))
                .child(
                    text(e.text.clone())
                        .font(font::BODY)
                        .color(theme::muted())
                        .wrap(),
                ),
        );
    }
    if entries.is_empty() {
        list = list.child(body("Nothing found."));
    }
    let mut moods = col().gap(10.0).child(caps("Start from a feeling"));
    for mood in Mood::ALL {
        let preset = aa_theory::mood_preset(mood);
        moods = moods.child(
            card()
                .child(
                    row()
                        .gap(8.0)
                        .items_center()
                        .child(text(mood.name()).font(font::TITLE).color(theme::text()))
                        .child(rune::spacer())
                        .child(
                            button(format!("mood-{}", mood.name()), "Use", accent, true).on_press(
                                move |s: &mut App| {
                                    let p = aa_theory::mood_preset(mood);
                                    let bpm = ((p.tempo_bpm[0] + p.tempo_bpm[1]) * 0.5).round();
                                    s.edit(|pr| {
                                        pr.tempo_bpm = bpm;
                                        pr.scale = p.scale;
                                    });
                                    s.toast(&format!(
                                        "{}: {bpm:.0} BPM, {}",
                                        mood.name(),
                                        p.scale.name()
                                    ));
                                },
                            ),
                        ),
                )
                .child(body(preset.description.to_string()))
                .child(
                    text(format!(
                        "{:.0}–{:.0} BPM · {} · {}",
                        preset.tempo_bpm[0],
                        preset.tempo_bpm[1],
                        preset.scale.name(),
                        preset
                            .roles
                            .iter()
                            .map(|r| r.name())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                    .font(font::SMALL)
                    .color(theme::faint())
                    .wrap(),
                )
                .child(
                    text(preset.synth_character.to_string())
                        .font(font::SMALL)
                        .color(theme::muted())
                        .wrap(),
                ),
        );
    }
    col()
        .key("learn")
        .w_full()
        .h_full()
        .padding(22.0)
        .gap(16.0)
        .min_h(0.0)
        .child(screen_header(
            "Learn",
            "Every music word in the app, in plain language. Hover a word anywhere to see it.",
            accent,
        ))
        .child(field)
        .child(
            row()
                .grow(1.0)
                .min_h(0.0)
                .gap(18.0)
                .items_start()
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .h_full()
                        .scroll(rune::el::ScrollSpec {
                            stick_bottom: false,
                        })
                        .child(list),
                )
                .child(
                    col()
                        .w(360.0)
                        .h_full()
                        .scroll(rune::el::ScrollSpec {
                            stick_bottom: false,
                        })
                        .child(moods),
                ),
        )
}
