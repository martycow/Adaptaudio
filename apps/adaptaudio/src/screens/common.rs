//! Pieces several screens share.

use crate::actions;
use crate::state::App;
use crate::theme::{self, font};
use aa_core::Clip;
use rune::{Color, Drag, El, Phase, Shape, Tx, canvas, col, row, text};

/// A miniature of a clip's notes.
pub fn clip_preview(c: &Clip, w: f32, h: f32, color: Color) -> El {
    let len = f32::from(c.length_steps.max(1));
    let (lo, hi) = c.notes.iter().fold((u8::MAX, 0u8), |(lo, hi), n| {
        (lo.min(n.pitch), hi.max(n.pitch))
    });
    let span = f32::from(hi.saturating_sub(lo).max(1));
    let mut shape = Shape::new();
    for n in &c.notes {
        let x0 = f32::from(n.step) / len * w;
        let x1 = ((f32::from(n.step) + n.length_steps.max(0.4)) / len * w)
            .min(w)
            .max(x0 + 2.0);
        let y = if hi == lo {
            h * 0.5
        } else {
            h - 1.5 - f32::from(n.pitch - lo) / span * (h - 3.0)
        };
        shape = shape.line(
            [x0 + 1.0, y],
            [x1 - 1.0, y],
            2.5,
            rune::shape::LineCap::Round,
            color.alpha(0.4 + 0.6 * n.velocity),
        );
    }
    canvas(shape).size(w, h)
}

pub fn empty_state(title: &str, body: &str) -> El {
    col()
        .w_full()
        .h_full()
        .center()
        .gap(6.0)
        .radius(14.0)
        .border(1.0, theme::line())
        .border_dash(6.0, 5.0)
        .child(
            text(title.to_string())
                .font(font::TITLE)
                .color(theme::muted()),
        )
        .child(
            text(body.to_string())
                .font(font::BODY)
                .color(theme::faint()),
        )
}

/// The tempo, dragged up and down like a hardware encoder.
pub fn bpm_drag(s: &App, accent: Color) -> El {
    let bpm = s.project.tempo_bpm;
    col()
        .key("bpm")
        .role("slider")
        .label("Tempo")
        .range_value(f64::from(bpm), 40.0, 240.0)
        .help(crate::glossary::lookup("BPM").unwrap_or_default() + "\nDrag up or down.")
        .w(64.0)
        .gap(1.0)
        .cursor(rune::Cursor::Pointer)
        .on_drag(move |s: &mut App, d: &mut Drag, _: &mut Tx| {
            if d.phase == Phase::Down {
                d.data = s.project.tempo_bpm;
            }
            actions::set_tempo(s, d.data - d.dy / 3.0, d.phase == Phase::Down);
        })
        .on_step(|s: &mut App, delta, _| {
            let bpm = s.project.tempo_bpm + delta as f32;
            actions::set_tempo(s, bpm, true);
        })
        .child(text("BPM").font(font::CAPS).color(theme::faint()))
        .child(
            row()
                .gap(4.0)
                .items_center()
                .child(
                    text(format!("{bpm:.0}"))
                        .font(font::NUM_BIG)
                        .color(theme::text()),
                )
                .child(text("↕").font(font::SMALL).color(accent)),
        )
}
