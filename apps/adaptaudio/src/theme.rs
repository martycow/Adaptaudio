//! Colours, type and the small visual vocabulary every screen shares.
//!
//! Dark graphite with neon accents. The accent follows the game mode that is playing, so the
//! whole window changes mood with the music.

use aa_core::TrackRole;
use rune::{Color, TextStyle, oklch, oklcha};

pub fn bg() -> Color {
    oklch(0.145, 0.012, 270.0)
}
pub fn panel() -> Color {
    oklch(0.178, 0.014, 270.0)
}
pub fn card() -> Color {
    oklch(0.215, 0.016, 270.0)
}
pub fn card_hi() -> Color {
    oklch(0.255, 0.018, 270.0)
}
pub fn line() -> Color {
    oklcha(1.0, 0.0, 0.0, 0.065)
}
pub fn line_strong() -> Color {
    oklcha(1.0, 0.0, 0.0, 0.12)
}
pub fn text() -> Color {
    oklch(0.96, 0.005, 270.0)
}
pub fn muted() -> Color {
    oklch(0.72, 0.012, 270.0)
}
pub fn faint() -> Color {
    oklch(0.52, 0.012, 270.0)
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
    pub const HERO: TextStyle = TextStyle::new(22.0, 700.0);
    pub const TITLE: TextStyle = TextStyle::new(15.0, 650.0);
    pub const HEAD: TextStyle = TextStyle::new(13.0, 620.0);
    pub const BODY: TextStyle = TextStyle::new(12.5, 450.0);
    pub const LABEL: TextStyle = TextStyle::new(11.5, 550.0);
    pub const SMALL: TextStyle = TextStyle::new(10.5, 500.0);
    pub const CAPS: TextStyle = TextStyle::new(10.0, 650.0).tracking(0.6);
    pub const NUM: TextStyle = TextStyle::new(12.0, 600.0).mono();
    pub const NUM_BIG: TextStyle = TextStyle::new(18.0, 650.0).mono();
}
