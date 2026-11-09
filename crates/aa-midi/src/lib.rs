//! aa-midi: MIDI between Adaptaudio and the studio's hardware synths.
//!
//! - [`MidiMessage`]: the messages we use, with their wire format.
//! - [`MidiOut`] / [`MidiIn`]: open real ports by part of their name.
//! - [`MidiScheduler`]: a thread that sends queued messages at exact times.
//! - [`ClockGenerator`] / [`MidiClockSender`]: MIDI Clock so hardware follows our tempo.
//! - [`KNOWN_DEVICES`] / [`guess_device`]: recognise the Hydrasynth, MiniFreak, EP-1320, Scarlett Solo, H4essential.
//!
//! Functions that touch devices return a [`MidiError`] when nothing is connected.

pub mod clock;
pub mod devices;
pub mod message;
pub mod ports;
pub mod scheduler;

pub use clock::{ClockGenerator, DEFAULT_LOOKAHEAD, MidiClockSender, PPQN};
pub use devices::{KNOWN_DEVICES, KnownDevice, external_instrument_for, guess_device};
pub use message::{CC_ALL_NOTES_OFF, CC_SUSTAIN, MidiBytes, MidiMessage};
pub use ports::{
    MidiError, MidiIn, MidiOut, MidiSink, all_notes_off_messages, find_port_index, list_inputs,
    list_outputs,
};
pub use scheduler::{DEFAULT_CAPACITY, MidiScheduler, Schedule, ScheduleFull, ScheduleProducer};
