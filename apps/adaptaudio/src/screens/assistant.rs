//! The AI assistant drawer: ask in any language; proposed edits wait for your Apply.

use crate::ext;
use crate::state::App;
use crate::theme::{self, font};
use crate::widgets::{button, caps};
use rune::{Color, El, col, row, text, text_input};

pub fn drawer(s: &App, accent: Color) -> El {
    let mut msgs = col().gap(10.0).padding(16.0);
    if s.ext.chat.is_empty() {
        msgs = msgs.child(
            col()
                .gap(8.0)
                .child(text("Ask anything").font(font::TITLE).color(theme::text()))
                .child(text("“Что такое сайдчейн?” · “Сделай тревожный бас для Stealth” · “Why does Chase feel flat?” · “Add a riser before the peak”").font(font::BODY).color(theme::muted()).wrap())
                .child(text("Edits come back as proposals: nothing changes until you press Apply, and every change can be undone.").font(font::SMALL).color(theme::faint()).wrap()),
        );
    }
    for (mi, m) in s.ext.chat.iter().enumerate() {
        let bubble = col()
            .key(format!("msg-{mi}"))
            .gap(8.0)
            .padding(12.0)
            .radius(14.0)
            .max_w(380.0)
            .fill(if m.from_user {
                accent.alpha(0.18)
            } else {
                theme::card()
            })
            .border(
                1.0,
                if m.error {
                    theme::danger()
                } else {
                    theme::line()
                },
            )
            .child(
                text(m.text.clone())
                    .font(font::BODY)
                    .color(if m.error {
                        theme::danger()
                    } else {
                        theme::text()
                    })
                    .wrap()
                    .selectable(true),
            );
        let mut bubble = bubble;
        for (ai, (action, applied)) in m.actions.iter().enumerate() {
            if !action.is_edit() {
                continue;
            }
            let label = action.describe();
            bubble = bubble.child(
                row()
                    .gap(8.0)
                    .items_center()
                    .padding(8.0)
                    .radius(10.0)
                    .fill(theme::panel())
                    .child(
                        text(label)
                            .font(font::SMALL)
                            .color(theme::muted())
                            .wrap()
                            .grow(1.0)
                            .min_w(0.0),
                    )
                    .child(if *applied {
                        text("✓ applied").font(font::SMALL).color(theme::ok())
                    } else {
                        button(format!("apply-{mi}-{ai}"), "Apply", accent, true)
                            .on_press(move |s: &mut App| apply(s, mi, ai))
                    }),
            );
        }
        if m.actions
            .iter()
            .filter(|(a, done)| a.is_edit() && !done)
            .count()
            > 1
        {
            bubble = bubble.child(
                button(format!("applyall-{mi}"), "Apply all", accent, false).on_press(
                    move |s: &mut App| {
                        let n = s.ext.chat.get(mi).map_or(0, |m| m.actions.len());
                        for ai in 0..n {
                            apply(s, mi, ai);
                        }
                    },
                ),
            );
        }
        for w in &m.warnings {
            bubble = bubble.child(
                text(format!("⚠ {w}"))
                    .font(font::SMALL)
                    .color(theme::warn())
                    .wrap(),
            );
        }
        let line = if m.from_user {
            row().child(rune::spacer()).child(bubble)
        } else {
            row().child(bubble)
        };
        msgs = msgs.child(line);
    }
    if s.ext.chat_rx.is_some() {
        msgs = msgs.child(
            row()
                .gap(8.0)
                .items_center()
                .child(
                    rune::el()
                        .size(8.0, 8.0)
                        .radius(4.0)
                        .fill(accent)
                        .live_opacity(&s.live.beat),
                )
                .child(text("Thinking…").font(font::LABEL).color(theme::muted())),
        );
    }
    let field = text_input(&s.ext.chat_input, font::BODY)
        .key("chat-input")
        .placeholder("Ask the assistant…", theme::faint())
        .grow(1.0)
        .min_h(38.0)
        .max_h(120.0)
        .px(12.0)
        .py(9.0)
        .radius(12.0)
        .fill(theme::panel())
        .border(1.0, theme::line_strong())
        .on_edit(|s: &mut App| &mut s.ext.chat_input)
        .on_submit(|s: &mut App, _| ext::ask(s));
    col()
        .key("assistant")
        .w(420.0)
        .h_full()
        .fill(theme::panel())
        .border_sides([1.0, 0.0, 0.0, 0.0], theme::line())
        .child(
            row()
                .padding(16.0)
                .gap(8.0)
                .items_center()
                .child(text("✧").font(font::TITLE).color(accent))
                .child(caps("Assistant"))
                .child(rune::spacer())
                .child(
                    button("chat-clear", "New chat", accent, false).on_press(|s: &mut App| {
                        s.ext.chat.clear();
                        if let Some(a) = s.ext.assistant.as_mut() {
                            a.clear();
                        }
                    }),
                ),
        )
        .child(
            col()
                .grow(1.0)
                .min_h(0.0)
                .scroll(rune::el::ScrollSpec { stick_bottom: true })
                .child(msgs),
        )
        .child(
            row().padding(14.0).gap(8.0).items_end().child(field).child(
                button("chat-send", "Send", accent, true)
                    .disabled(s.ext.chat_rx.is_some())
                    .on_press(ext::ask),
            ),
        )
}

fn apply(s: &mut App, message: usize, action: usize) {
    let Some((a, done)) = s
        .ext
        .chat
        .get(message)
        .and_then(|m| m.actions.get(action))
        .cloned()
    else {
        return;
    };
    if done || !a.is_edit() {
        return;
    }
    match ext::apply_action(s, &a) {
        Ok(()) => {
            if let Some(slot) = s
                .ext
                .chat
                .get_mut(message)
                .and_then(|m| m.actions.get_mut(action))
            {
                slot.1 = true;
            }
            s.toast(&format!("Applied: {}", a.describe()));
        }
        Err(e) => s.toast_error(&format!("Can't apply: {e}")),
    }
}
