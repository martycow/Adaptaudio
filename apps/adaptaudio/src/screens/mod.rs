//! One module per screen.

pub mod adaptive;
pub mod assistant;
pub mod capture;
pub mod common;
pub mod editor;
pub mod learn;
pub mod mixer;
pub mod playtest;
pub mod session;
pub mod settings;
pub mod sound;
pub mod wizard;

use crate::state::{App, Screen};
use rune::{Color, Cx, El};

pub fn view(s: &App, _cx: &mut Cx<App>, accent: Color) -> El {
    match s.screen {
        Screen::Session => session::view(s, accent),
        Screen::Sound => sound::view(s, accent),
        Screen::Capture => capture::view(s, accent),
        Screen::Mixer => mixer::view(s, accent),
        Screen::Adaptive => adaptive::view(s, accent),
        Screen::Playtest => playtest::view(s, accent),
        Screen::Learn => learn::view(s, accent),
        Screen::Settings => settings::view(s, accent),
    }
}
