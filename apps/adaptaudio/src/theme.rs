//! Colours, type and the small visual vocabulary every screen shares.
//!
//! Quiet dark graphite: flat surfaces, thin lines, one accent. The accent follows the game mode
//! that is playing, so the window changes mood with the music.

use aa_core::TrackRole;
use rune::{Color, TextStyle, oklch, oklcha};

pub fn bg() -> Color {
    oklch(0.155, 0.006, 265.0)
}
pub fn panel() -> Color {
    oklch(0.175, 0.007, 265.0)
}
pub fn card() -> Color {
    oklch(0.205, 0.008, 265.0)
}
pub fn card_hi() -> Color {
    oklch(0.245, 0.009, 265.0)
}
pub fn line() -> Color {
    oklcha(1.0, 0.0, 0.0, 0.06)
}
pub fn line_strong() -> Color {
    oklcha(1.0, 0.0, 0.0, 0.11)
}
pub fn text() -> Color {
    oklch(0.95, 0.004, 265.0)
}
pub fn muted() -> Color {
    oklch(0.73, 0.008, 265.0)
}
pub fn faint() -> Color {
    oklch(0.56, 0.008, 265.0)
}
pub fn hover() -> Color {
    oklcha(1.0, 0.0, 0.0, 0.035)
}
pub fn press() -> Color {
    oklcha(1.0, 0.0, 0.0, 0.07)
}
pub fn shadow() -> Color {
    oklcha(0.0, 0.0, 0.0, 0.45)
}
pub fn moss() -> Color {
    oklch(0.72, 0.13, 135.0)
}
pub fn cedar() -> Color {
    oklch(0.62, 0.11, 55.0)
}
pub fn danger() -> Color {
    oklch(0.66, 0.2, 25.0)
}
pub fn ok() -> Color {
    oklch(0.76, 0.16, 150.0)
}
pub fn warn() -> Color {
    oklch(0.8, 0.15, 80.0)
}

/// Rune's component kit (dropdowns, scroll bars) painted in this theme.
pub fn kit(accent: Color) -> rune_kit::Kit {
    let mut k = rune_kit::Kit::neutral(true);
    let c = &mut k.c;
    c.page = bg();
    c.bg_base = card();
    c.bg_deep = bg();
    c.layer_01 = card_hi();
    c.layer_02 = card();
    c.surface_base = card();
    c.surface_raised = card_hi();
    c.surface_raised_base = card_hi();
    c.surface_float = card_hi();
    c.bg_button_neutral = card_hi();
    c.text_base = text();
    c.text_strong = text();
    c.text_muted = muted();
    c.text_weak = muted();
    c.text_faint = faint();
    c.icon_base = text();
    c.icon_muted = muted();
    c.border_muted = line();
    c.border_weak = line();
    c.border_base = line_strong();
    c.border_strong = oklcha(1.0, 0.0, 0.0, 0.28);
    c.border_hover = oklcha(1.0, 0.0, 0.0, 0.2);
    c.border_focus = accent;
    c.border_active = accent;
    c.bg_accent = accent;
    c.text_accent = accent;
    c.icon_accent = accent;
    c.hover = hover();
    c.pressed = press();
    k
}

/// The accent for a game mode colour string, with a fallback.
pub fn mode_color(hex: &str) -> Color {
    rune::color::hex(hex).unwrap_or_else(default_accent)
}

pub fn default_accent() -> Color {
    oklch(0.72, 0.17, 300.0)
}

pub fn role_color(role: TrackRole) -> Color {
    let h = match role {
        TrackRole::Kick => 25.0,
        TrackRole::Snare => 80.0,
        TrackRole::Hats => 190.0,
        TrackRole::Perc => 50.0,
        TrackRole::Bass => 295.0,
        TrackRole::Pad => 245.0,
        TrackRole::Lead => 340.0,
        TrackRole::Arp => 210.0,
        TrackRole::Ambience => 140.0,
        TrackRole::Fx => 95.0,
        TrackRole::Custom => 270.0,
    };
    oklch(0.74, 0.15, h)
}

pub mod font {
    use super::TextStyle;
    pub const HERO: TextStyle = TextStyle::new(20.0, 650.0);
    pub const TITLE: TextStyle = TextStyle::new(15.0, 650.0);
    pub const HEAD: TextStyle = TextStyle::new(13.0, 620.0);
    pub const BODY: TextStyle = TextStyle::new(12.5, 450.0);
    pub const LABEL: TextStyle = TextStyle::new(11.5, 550.0);
    pub const SMALL: TextStyle = TextStyle::new(10.5, 500.0);
    pub const CAPS: TextStyle = TextStyle::new(10.0, 650.0).tracking(0.6);
    pub const NUM: TextStyle = TextStyle::new(12.0, 600.0).mono();
    pub const NUM_BIG: TextStyle = TextStyle::new(18.0, 650.0).mono();
}
