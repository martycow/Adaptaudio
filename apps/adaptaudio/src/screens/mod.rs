//! One module per screen.

pub mod adaptive;
pub mod assistant;
pub mod capture;
pub mod common;
pub mod learn;
pub mod mixer;
pub mod pattern;
pub mod puzzle;
pub mod session;
pub mod settings;
pub mod simulator;
pub mod sound;

use crate::state::{App, Screen};
use rune::{Color, Cx, El};

pub fn view(s: &App, _cx: &mut Cx<App>, accent: Color) -> El {
    match s.screen {
        Screen::Session => session::view(s, accent),
        Screen::Pattern => pattern::view(s, accent),
        Screen::Sound => sound::view(s, accent),
        Screen::Capture => capture::view(s, accent),
        Screen::Mixer => mixer::view(s, accent),
        Screen::Adaptive => adaptive::view(s, accent),
        Screen::Simulator => simulator::view(s, accent),
        Screen::Puzzle => puzzle::view(s, accent),
        Screen::Learn => learn::view(s, accent),
        Screen::Settings => settings::view(s, accent),
    }
}
