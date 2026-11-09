//! Mixer: one strip per track plus the master bus.

use crate::state::App;
use crate::theme::{self, font};
use crate::widgets::{caps, db, knob, live_meter, screen_header, toggle};
use aa_core::{MixerSettings, TrackId};
use rune::{Color, Drag, El, Phase, Tx, col, el, row, text};

fn mix_edit(s: &mut App, tid: TrackId, start: bool, f: impl FnOnce(&mut MixerSettings)) {
    if start {
        s.begin_gesture();
    }
    s.edit_quiet(|p| {
        if let Some(t) = p.track_mut(tid) {
            f(&mut t.mixer);
        }
    });
}

fn fader(
    key: String,
    db_value: f32,
    color: Color,
    on_change: impl Fn(&mut App, f32, bool) + 'static,
) -> El {
    let h = 170.0;
    let unit = ((db_value + 60.0) / 66.0).clamp(0.0, 1.0);
    let y = (1.0 - unit) * (h - 14.0);
    el().key(key)
        .role("slider")
        .label("Volume")
        .range_value(f64::from(db_value), -60.0, 6.0)
        .size(30.0, h)
        .cursor(rune::Cursor::Pointer)
        .child(
            el().size(4.0, h)
                .radius(2.0)
                .fill(theme::line_strong())
                .absolute(13.0, 0.0),
        )
        .child(
            el().size(4.0, h - y)
                .radius(2.0)
                .fill(color.alpha(0.7))
                .absolute(13.0, y),
        )
        .child(
            el().size(30.0, 14.0)
                .radius(4.0)
                .fill(theme::text())
                .shadow(1.0, 6.0, theme::shadow())
                .absolute(0.0, y),
        )
        .on_drag(move |s: &mut App, d: &mut Drag, _: &mut Tx| {
            if d.cancelled() {
                return;
            }
            let u = 1.0 - (d.y / d.h).clamp(0.0, 1.0);
            on_change(s, (u * 66.0 - 60.0).round(), d.phase == Phase::Down);
        })
}

pub fn view(s: &App, accent: Color) -> El {
    let mut strips = row().key("strips").gap(10.0).items_start();
    for (i, t) in s.project.tracks.iter().enumerate() {
        let tid = t.id;
        let color = theme::role_color(t.role);
        let m = &t.mixer;
        let meter = s
            .live
            .track_level
            .get(i)
            .map_or_else(el, |l| live_meter(6.0, 170.0, color, l));
        strips = strips.child(
            col()
                .key(format!("strip-{tid}"))
                .w(104.0)
                .padding(10.0)
                .gap(10.0)
                .items_center()
                .radius(14.0)
                .fill(theme::card())
                .border(
                    1.0,
                    if s.selected_track == Some(tid) {
                        accent
                    } else {
                        theme::line()
                    },
                )
                .on_press(move |s: &mut App| s.selected_track = Some(tid))
                .child(el().w_full().h(4.0).radius(2.0).fill(color))
                .child(
                    text(t.name.clone())
                        .font(font::HEAD)
                        .color(theme::text())
                        .truncate(),
                )
                .child(knob(
                    format!("pan-{tid}"),
                    "Pan",
                    (m.pan + 1.0) * 0.5,
                    format!("{:+.0}", m.pan * 100.0),
                    color,
                    move |s: &mut App, v, st| {
                        mix_edit(s, tid, st, |m| m.pan = (v * 2.0 - 1.0).clamp(-1.0, 1.0))
                    },
                ))
                .child(knob(
                    format!("rev-{tid}"),
                    "Reverb",
                    m.reverb_send,
                    format!("{:.0}%", m.reverb_send * 100.0),
                    color,
                    move |s: &mut App, v, st| mix_edit(s, tid, st, |m| m.reverb_send = v),
                ))
                .child(knob(
                    format!("drv-{tid}"),
                    "Drive",
                    m.drive,
                    format!("{:.0}%", m.drive * 100.0),
                    color,
                    move |s: &mut App, v, st| mix_edit(s, tid, st, |m| m.drive = v),
                ))
                .child(knob(
                    format!("sc-{tid}"),
                    "Sidechain",
                    m.sidechain,
                    format!("{:.0}%", m.sidechain * 100.0),
                    color,
                    move |s: &mut App, v, st| mix_edit(s, tid, st, |m| m.sidechain = v),
                ))
                .child(
                    row()
                        .gap(6.0)
                        .items_end()
                        .child(fader(
                            format!("fad-{tid}"),
                            m.volume_db,
                            color,
                            move |s: &mut App, v, st| mix_edit(s, tid, st, |m| m.volume_db = v),
                        ))
                        .child(meter),
                )
                .child(
                    text(db(m.volume_db))
                        .font(font::SMALL.mono())
                        .color(theme::text()),
                )
                .child(
                    row()
                        .gap(4.0)
                        .child(toggle(
                            format!("mm-{tid}"),
                            "M",
                            m.mute,
                            theme::warn(),
                            move |s: &mut App| mix_edit(s, tid, true, |m| m.mute = !m.mute),
                        ))
                        .child(toggle(
                            format!("ms-{tid}"),
                            "S",
                            m.solo,
                            theme::ok(),
                            move |s: &mut App| mix_edit(s, tid, true, |m| m.solo = !m.solo),
                        )),
                ),
        );
    }
    let ms = &s.project.master;
    let master_edit = |s: &mut App, start: bool, f: &dyn Fn(&mut aa_core::MasterSettings)| {
        if start {
            s.begin_gesture();
        }
        s.edit_quiet(|p| f(&mut p.master));
    };
    let master = col()
        .key("master")
        .w(150.0)
        .padding(12.0)
        .gap(10.0)
        .items_center()
        .radius(14.0)
        .fill(theme::card_hi())
        .border(1.0, accent.alpha(0.6))
        .child(caps("Master"))
        .child(
            row()
                .gap(6.0)
                .child(fader(
                    "fad-master".into(),
                    ms.volume_db,
                    accent,
                    move |s: &mut App, v, st| master_edit(s, st, &|m| m.volume_db = v),
                ))
                .child(live_meter(6.0, 170.0, accent, &s.live.master[0]))
                .child(live_meter(6.0, 170.0, accent, &s.live.master[1])),
        )
        .child(
            text(db(ms.volume_db))
                .font(font::SMALL.mono())
                .color(theme::text()),
        )
        .child(knob(
            "room",
            "Reverb",
            ms.reverb_size,
            format!("{:.0}%", ms.reverb_size * 100.0),
            accent,
            move |s: &mut App, v, st| master_edit(s, st, &|m| m.reverb_size = v),
        ))
        .child(knob(
            "screl",
            "Sidechain",
            ms.sidechain_release / 0.6,
            format!("{:.0} ms", ms.sidechain_release * 1000.0),
            accent,
            move |s: &mut App, v, st| {
                master_edit(s, st, &|m| m.sidechain_release = (v * 0.6).max(0.02))
            },
        ))
        .child(toggle(
            "limiter",
            "Limiter",
            ms.limiter,
            accent,
            |s: &mut App| s.edit(|p| p.master.limiter = !p.master.limiter),
        ));
    col()
        .key("mixer")
        .w_full()
        .h_full()
        .padding(22.0)
        .gap(16.0)
        .min_h(0.0)
        .child(screen_header("Mixer", "Balance the tracks. Sidechain makes a track duck under the kick — the pumping of a big peak.", accent))
        .child(col().grow(1.0).min_h(0.0).scroll_xy().child(row().gap(16.0).items_start().child(strips).child(master)))
}
