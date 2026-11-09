//! Reusable controls: buttons, pills, sliders, knobs, meters, cards. All drawn with Rune.

use crate::state::App;
use crate::theme::{self, font};
use rune::{Color, Drag, El, Live, Phase, Shape, Tx, canvas, col, el, row, text};
use rune_kit::{ScrollBar, ScrollBarStyle, Select, SelectAppearance};
use std::cell::RefCell;

thread_local! {
    /// Floating pieces (open dropdown lists) collected while the view is built; the shell mounts
    /// them above everything else.
    static LAYER: RefCell<Vec<El>> = const { RefCell::new(Vec::new()) };
}

/// Takes the floating pieces built since the last call.
pub fn take_layer() -> Vec<El> {
    LAYER.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

/// A dropdown list. `on_choose` gets the index of the picked option.
pub fn dropdown<T: AsRef<str>>(
    s: &App,
    key: &str,
    options: &[T],
    current: Option<usize>,
    width: f32,
    on_choose: impl Fn(&mut App, usize, &mut Tx) + Clone + 'static,
) -> El {
    let kit = theme::kit(s.accent());
    let names: Vec<String> = options.iter().map(|o| o.as_ref().to_string()).collect();
    let open = s.open_menu.as_deref() == Some(key);
    let (k1, lookup) = (key.to_string(), names.clone());
    let select = Select::new(key, &names)
        .appearance(SelectAppearance::Field)
        .width(width)
        .placeholder("Choose…")
        .current(current.and_then(|i| names.get(i)).map(String::as_str))
        .open(open)
        .on_toggle(move |s: &mut App| {
            s.open_menu = if s.open_menu.as_deref() == Some(k1.as_str()) {
                None
            } else {
                Some(k1.clone())
            }
        })
        .on_close(|s: &mut App| s.open_menu = None)
        .on_choose(move |s: &mut App, picked: &str, tx: &mut Tx| {
            s.open_menu = None;
            if let Some(i) = lookup.iter().position(|n| n == picked) {
                on_choose(s, i, tx);
            }
        });
    LAYER.with(|l| select.build(&kit, &mut l.borrow_mut()))
}

/// A name on the left, a control on the right.
pub fn field_row(name: &str, control: El) -> El {
    row()
        .gap(10.0)
        .items_center()
        .child(term(name, font::LABEL, theme::muted()).w(120.0))
        .child(control)
}

fn bar_style() -> ScrollBarStyle {
    ScrollBarStyle::neutral(true)
        .thumb(theme::line_strong())
        .thumb_hover(theme::faint())
}

/// Fills the space it is given and scrolls its content down, with a scroll bar when needed.
/// Rune tells scroll bars apart by where they are made, hence `track_caller` on these helpers.
#[track_caller]
pub fn vscroll(content: El) -> El {
    col()
        .grow(1.0)
        .min_h(0.0)
        .min_w(0.0)
        .scroll(rune::el::ScrollSpec {
            stick_bottom: false,
        })
        .child(content)
        .scroll_bar_styled(bar_style())
}

/// Scrolls its content sideways, with a scroll bar when needed.
#[track_caller]
pub fn hscroll(content: El) -> El {
    row()
        .min_w(0.0)
        .scroll_x()
        .child(content)
        .scroll_bar_styled(bar_style().horizontal())
}

/// Scrolls both ways (grids); the scroll bar shows the vertical position.
#[track_caller]
pub fn xyscroll(content: El) -> El {
    col()
        .grow(1.0)
        .min_h(0.0)
        .min_w(0.0)
        .scroll_xy()
        .child(content)
        .scroll_bar_styled(bar_style())
}

/// A one-line text field. Its key ends in "-field" so typing never triggers shortcuts.
pub fn text_field(
    name: &str,
    edit: &rune::TextEdit,
    placeholder: &str,
    width: f32,
    access: impl for<'a> Fn(&'a mut App) -> &'a mut rune::TextEdit + 'static,
) -> El {
    rune::text_input(edit, font::BODY)
        .key(format!("{name}-field"))
        .single_line()
        .placeholder(placeholder.to_string(), theme::faint())
        .w(width)
        .px(10.0)
        .py(7.0)
        .radius(8.0)
        .fill(theme::panel())
        .border(1.0, theme::line_strong())
        .on_edit(access)
}

/// A toolbar button with a glyph and a word, so nobody has to guess.
pub fn tool_button(
    key: impl Into<String>,
    glyph: &str,
    label: &str,
    help: &str,
    active: bool,
    accent: Color,
) -> El {
    let ink = if active { accent } else { theme::text() };
    row()
        .key(key)
        .role("button")
        .label(label.to_string())
        .help(help.to_string())
        .focusable(true)
        .h(32.0)
        .px(10.0)
        .gap(6.0)
        .center()
        .radius(8.0)
        .fill(if active {
            accent.alpha(0.16)
        } else {
            theme::panel()
        })
        .hover(theme::hover())
        .press(theme::press())
        .press_scale(0.96)
        .motion(rune::tok::motion::SNAPPY)
        .child(text(glyph).font(font::HEAD).color(ink))
        .child(text(label.to_string()).font(font::LABEL).color(ink))
}

pub fn label(s: impl Into<String>) -> El {
    text(s).font(font::LABEL).color(theme::muted())
}

pub fn caps(s: impl Into<String>) -> El {
    text(s.into().to_uppercase())
        .font(font::CAPS)
        .color(theme::faint())
}

pub fn body(s: impl Into<String>) -> El {
    text(s).font(font::BODY).color(theme::muted()).wrap()
}

/// A term with its glossary explanation as a tooltip.
pub fn term(s: &str, style: rune::TextStyle, color: Color) -> El {
    let t = text(s).font(style).color(color);
    match crate::glossary::lookup(s) {
        Some(explain) => t.help(explain),
        None => t,
    }
}

/// A rounded panel.
pub fn card() -> El {
    col()
        .fill(theme::card())
        .radius(12.0)
        .padding(16.0)
        .gap(12.0)
}

/// A text button. `primary` fills it with the accent.
pub fn button(
    key: impl Into<String>,
    label: impl Into<String>,
    accent: Color,
    primary: bool,
) -> El {
    let label: String = label.into();
    let (fill, ink, border) = if primary {
        (accent, theme::bg(), accent)
    } else {
        (theme::card_hi(), theme::text(), theme::line_strong())
    };
    row()
        .key(key)
        .role("button")
        .label(label.clone())
        .focusable(true)
        .self_start()
        .h(30.0)
        .px(12.0)
        .center()
        .radius(8.0)
        .fill(fill)
        .border(1.0, border)
        .hover(theme::hover())
        .press(theme::press())
        .press_scale(0.96)
        .child(text(label).font(font::LABEL).color(ink))
}

/// A rounded tag. `active` lights it in its colour.
pub fn pill(key: impl Into<String>, label: impl Into<String>, color: Color, active: bool) -> El {
    let label: String = label.into();
    let (fill, ink, border) = if active {
        (color.alpha(0.24), color, color.alpha(0.85))
    } else {
        (theme::card(), theme::muted(), theme::line_strong())
    };
    let mut p = row()
        .key(key)
        .role("button")
        .label(label.clone())
        .h(26.0)
        .px(11.0)
        .gap(6.0)
        .center()
        .radius(13.0)
        .fill(fill)
        .border(1.0, border)
        .hover(theme::hover())
        .press_scale(0.95)
        .motion(rune::tok::motion::SNAPPY);
    p = p.child(el().size(7.0, 7.0).radius(3.5).fill(color));
    let p = p.child(text(label).font(font::LABEL).color(ink));
    if active {
        p.shadow(0.0, 14.0, color.alpha(0.45))
    } else {
        p
    }
}

/// A segmented choice. `on_pick` gets the chosen index.
pub fn segmented(
    key: &str,
    options: &[&str],
    current: usize,
    accent: Color,
    on_pick: impl Fn(&mut App, usize) + Clone + 'static,
) -> El {
    let mut r = row()
        .key(key.to_string())
        .role("radiogroup")
        .self_start()
        .fill(theme::panel())
        .radius(9.0)
        .padding(2.0)
        .gap(2.0)
        .border(1.0, theme::line());
    for (i, o) in options.iter().enumerate() {
        let on = i == current;
        let f = on_pick.clone();
        r = r.child(
            row()
                .key(format!("{key}-{i}"))
                .role("radio")
                .checked(on)
                .label(o.to_string())
                .h(24.0)
                .px(9.0)
                .center()
                .radius(7.0)
                .fill(if on {
                    accent.alpha(0.25)
                } else {
                    theme::panel()
                })
                .hover(theme::hover())
                .motion(rune::tok::motion::SNAPPY)
                .on_press(move |s: &mut App| f(s, i))
                .child(text(o.to_string()).font(font::SMALL).color(if on {
                    accent
                } else {
                    theme::muted()
                })),
        );
    }
    r
}

/// A horizontal fill bar drawn as a stroke, trimmed to a live [start, end].
pub fn live_bar(w: f32, h: f32, color: Color, value: &Live<[f32; 2]>) -> El {
    let shape = Shape::new().stroke(vec![[h * 0.5, h * 0.5], [w - h * 0.5, h * 0.5]], h, color);
    canvas(shape).size(w, h).live_trim(value)
}

/// A vertical level meter trimmed to a live [0, level].
pub fn live_meter(w: f32, h: f32, color: Color, value: &Live<[f32; 2]>) -> El {
    let shape = Shape::new().stroke(vec![[w * 0.5, h - w * 0.5], [w * 0.5, w * 0.5]], w, color);
    let track = el().size(w, h).radius(w * 0.5).fill(theme::line());
    el().size(w, h)
        .child(track.absolute(0.0, 0.0))
        .child(canvas(shape).size(w, h).absolute(0.0, 0.0).live_trim(value))
}

/// A slider 0..1. `on_change(app, value, starting)` runs on every move; `starting` on press.
pub fn slider(
    key: impl Into<String>,
    value: f32,
    w: f32,
    color: Color,
    on_change: impl Fn(&mut App, f32, bool) + 'static,
) -> El {
    let v = value.clamp(0.0, 1.0);
    let h = 22.0;
    let track_h = 6.0;
    let fill_w = (w - 16.0) * v + 8.0;
    row()
        .key(key)
        .role("slider")
        .range_value(f64::from(v), 0.0, 1.0)
        .size(w, h)
        .cursor(rune::Cursor::Pointer)
        .child(
            el().size(w, track_h)
                .radius(track_h * 0.5)
                .fill(theme::line_strong())
                .absolute(0.0, (h - track_h) * 0.5),
        )
        .child(
            el().size(fill_w, track_h)
                .radius(track_h * 0.5)
                .fill(color)
                .absolute(0.0, (h - track_h) * 0.5),
        )
        .child(
            el().size(16.0, 16.0)
                .radius(8.0)
                .fill(theme::text())
                .shadow(1.0, 6.0, theme::shadow())
                .absolute(fill_w - 8.0, 3.0),
        )
        .on_drag(move |s: &mut App, d: &mut Drag, _tx: &mut Tx| {
            if d.cancelled() {
                return;
            }
            let x = ((d.x - 8.0) / (d.w - 16.0).max(1.0)).clamp(0.0, 1.0);
            on_change(s, x, d.phase == Phase::Down);
        })
}

/// A slider plus its name and printed value in one row.
pub fn slider_row(
    key: &str,
    name: &str,
    value: f32,
    shown: String,
    w: f32,
    color: Color,
    on_change: impl Fn(&mut App, f32, bool) + 'static,
) -> El {
    row()
        .gap(10.0)
        .items_center()
        .child(term(name, font::LABEL, theme::muted()).w(92.0).truncate())
        .child(slider(key.to_string(), value, w, color, on_change))
        .child(text(shown).font(font::NUM).color(theme::text()).w(58.0))
}

/// A rotary knob (drag up/down). Value 0..1.
pub fn knob(
    key: impl Into<String>,
    name: &str,
    value: f32,
    shown: String,
    color: Color,
    on_change: impl Fn(&mut App, f32, bool) + 'static,
) -> El {
    let v = value.clamp(0.0, 1.0);
    let size = 44.0;
    let r = size * 0.5 - 4.0;
    let c = size * 0.5;
    let start = std::f32::consts::PI * 0.75;
    let sweep = std::f32::consts::PI * 1.5;
    let arc = |to: f32| -> Vec<[f32; 2]> {
        let n = 28;
        (0..=n)
            .map(|i| {
                let a = start + sweep * to * i as f32 / n as f32;
                [c + r * a.cos(), c + r * a.sin()]
            })
            .collect()
    };
    let bg = Shape::new().stroke(arc(1.0), 4.0, theme::line_strong());
    let fg = Shape::new().stroke(arc(v.max(0.001)), 4.0, color);
    let a = start + sweep * v;
    let dot = Shape::new().line(
        [c, c],
        [c + (r - 7.0) * a.cos(), c + (r - 7.0) * a.sin()],
        3.0,
        rune::shape::LineCap::Round,
        theme::text(),
    );
    col()
        .gap(3.0)
        .items_center()
        .child(
            el().key(key)
                .role("slider")
                .label(name.to_string())
                .range_value(f64::from(v), 0.0, 1.0)
                .size(size, size)
                .cursor(rune::Cursor::Pointer)
                .child(
                    el().size(size - 14.0, size - 14.0)
                        .radius((size - 14.0) * 0.5)
                        .fill(theme::card_hi())
                        .absolute(7.0, 7.0),
                )
                .child(canvas(bg).size(size, size).absolute(0.0, 0.0))
                .child(canvas(fg).size(size, size).absolute(0.0, 0.0))
                .child(canvas(dot).size(size, size).absolute(0.0, 0.0))
                .on_drag(move |s: &mut App, d: &mut Drag, _tx: &mut Tx| {
                    if d.cancelled() {
                        return;
                    }
                    if d.phase == Phase::Down {
                        d.data = v;
                    }
                    let next = (d.data - d.dy / 160.0).clamp(0.0, 1.0);
                    on_change(s, next, d.phase == Phase::Down);
                }),
        )
        .child(term(name, font::SMALL, theme::muted()))
        .child(text(shown).font(font::SMALL.mono()).color(theme::text()))
}

/// A toggle chip.
pub fn toggle(
    key: impl Into<String>,
    label: &str,
    on: bool,
    color: Color,
    on_toggle: impl Fn(&mut App) + 'static,
) -> El {
    row()
        .key(key)
        .role("switch")
        .checked(on)
        .label(label.to_string())
        .h(24.0)
        .px(9.0)
        .center()
        .radius(7.0)
        .fill(if on {
            color.alpha(0.28)
        } else {
            theme::card_hi()
        })
        .border(1.0, if on { color } else { theme::line_strong() })
        .hover(theme::hover())
        .press_scale(0.94)
        .on_press(on_toggle)
        .child(text(label.to_string()).font(font::SMALL).color(if on {
            color
        } else {
            theme::muted()
        }))
}

/// A heading for a screen.
pub fn screen_header(name: &str, about: &str, accent: Color) -> El {
    col()
        .gap(4.0)
        .child(
            row()
                .gap(10.0)
                .items_center()
                .child(el().size(8.0, 8.0).radius(4.0).fill(accent))
                .child(text(name).font(font::HERO).color(theme::text())),
        )
        .child(text(about).font(font::BODY).color(theme::muted()).wrap())
}

/// Linear-to-dB text.
pub fn db(v: f32) -> String {
    format!("{v:+.1} dB")
}
