//! Talking to real MIDI ports through midir (CoreMIDI on macOS).
//!
//! Everything here fails with a [`MidiError`] when no device is plugged in; nothing panics.

use crate::message::{CC_ALL_NOTES_OFF, CC_SUSTAIN, MidiMessage};
use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use std::fmt;

const CLIENT_NAME: &str = "Adaptaudio";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiError {
    /// The system MIDI service could not be opened.
    Init(String),
    /// No port name contains the text we looked for.
    PortNotFound {
        wanted: String,
        available: Vec<String>,
    },
    /// The port exists but opening it failed.
    Connect(String),
    /// Sending bytes to an open port failed (for example, the cable was pulled).
    Send(String),
    /// The scheduler thread could not be started.
    Thread(String),
}

impl fmt::Display for MidiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MidiError::Init(e) => write!(f, "could not open the MIDI system: {e}"),
            MidiError::PortNotFound { wanted, available } if available.is_empty() => {
                write!(
                    f,
                    "no MIDI port matches \"{wanted}\": no MIDI devices are connected"
                )
            }
            MidiError::PortNotFound { wanted, available } => {
                write!(
                    f,
                    "no MIDI port matches \"{wanted}\" (found: {})",
                    available.join(", ")
                )
            }
            MidiError::Connect(e) => write!(f, "could not open the MIDI port: {e}"),
            MidiError::Send(e) => write!(f, "could not send MIDI: {e}"),
            MidiError::Thread(e) => write!(f, "could not start the MIDI thread: {e}"),
        }
    }
}

impl std::error::Error for MidiError {}

/// Index of the first name containing `wanted`, ignoring case. An empty `wanted` matches the
/// first port.
pub fn find_port_index(names: &[String], wanted: &str) -> Option<usize> {
    let wanted = wanted.to_lowercase();
    names
        .iter()
        .position(|name| name.to_lowercase().contains(&wanted))
}

/// Names of all MIDI outputs (ports Adaptaudio can play), e.g. "Hydrasynth Explorer".
pub fn list_outputs() -> Result<Vec<String>, MidiError> {
    let output = MidiOutput::new(CLIENT_NAME).map_err(|e| MidiError::Init(e.to_string()))?;
    Ok(output
        .ports()
        .iter()
        .filter_map(|p| output.port_name(p).ok())
        .collect())
}

/// Names of all MIDI inputs (ports Adaptaudio can listen to).
pub fn list_inputs() -> Result<Vec<String>, MidiError> {
    let input = MidiInput::new(CLIENT_NAME).map_err(|e| MidiError::Init(e.to_string()))?;
    Ok(input
        .ports()
        .iter()
        .filter_map(|p| input.port_name(p).ok())
        .collect())
}

/// Something that accepts MIDI messages right now. [`MidiOut`] is the real one; tests use fakes.
pub trait MidiSink: Send + 'static {
    fn send(&mut self, msg: &MidiMessage) -> Result<(), MidiError>;
}

/// An open connection to one MIDI output port.
pub struct MidiOut {
    port_name: String,
    connection: MidiOutputConnection,
}

impl fmt::Debug for MidiOut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MidiOut")
            .field("port_name", &self.port_name)
            .finish()
    }
}

impl MidiOut {
    /// Opens the first output whose name contains `port_name_substring` (ignoring case).
    pub fn connect(port_name_substring: &str) -> Result<MidiOut, MidiError> {
        let output = MidiOutput::new(CLIENT_NAME).map_err(|e| MidiError::Init(e.to_string()))?;
        let ports = output.ports();
        let names: Vec<String> = ports
            .iter()
            .map(|p| output.port_name(p).unwrap_or_default())
            .collect();
        let index = find_port_index(&names, port_name_substring).ok_or_else(|| {
            MidiError::PortNotFound {
                wanted: port_name_substring.to_string(),
                available: names.clone(),
            }
        })?;
        let connection = output
            .connect(&ports[index], "Adaptaudio out")
            .map_err(|e| MidiError::Connect(e.to_string()))?;
        Ok(MidiOut {
            port_name: names[index].clone(),
            connection,
        })
    }

    /// The full name of the connected port.
    pub fn port_name(&self) -> &str {
        &self.port_name
    }

    pub fn send(&mut self, msg: &MidiMessage) -> Result<(), MidiError> {
        self.connection
            .send(&msg.to_bytes())
            .map_err(|e| MidiError::Send(e.to_string()))
    }

    /// Releases the sustain pedal and silences every note on all 16 channels.
    /// Use it on stop and when something goes wrong, so no note hangs.
    pub fn all_notes_off(&mut self) -> Result<(), MidiError> {
        for msg in all_notes_off_messages() {
            self.send(&msg)?;
        }
        Ok(())
    }
}

impl MidiSink for MidiOut {
    fn send(&mut self, msg: &MidiMessage) -> Result<(), MidiError> {
        MidiOut::send(self, msg)
    }
}

/// The panic messages: sustain off and All Notes Off on every channel.
pub fn all_notes_off_messages() -> impl Iterator<Item = MidiMessage> {
    (1..=16u8).flat_map(|channel| {
        [
            MidiMessage::ControlChange {
                channel,
                controller: CC_SUSTAIN,
                value: 0,
            },
            MidiMessage::ControlChange {
                channel,
                controller: CC_ALL_NOTES_OFF,
                value: 0,
            },
        ]
    })
}

/// An open connection to one MIDI input port. Messages arrive on midir's thread until this
/// value is dropped.
pub struct MidiIn {
    port_name: String,
    _connection: MidiInputConnection<()>,
}

impl fmt::Debug for MidiIn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MidiIn")
            .field("port_name", &self.port_name)
            .finish()
    }
}

impl MidiIn {
    /// Opens the first input whose name contains `port_name_substring` (ignoring case) and
    /// calls `on_message` for every message [`MidiMessage::parse`] understands, including
    /// clock and transport. Filter with [`MidiMessage::is_note`] to just play the built-in
    /// synth from a hardware keyboard.
    ///
    /// The callback runs on the MIDI system's thread: keep it short and do not block.
    pub fn connect(
        port_name_substring: &str,
        mut on_message: impl FnMut(MidiMessage) + Send + 'static,
    ) -> Result<MidiIn, MidiError> {
        let mut input = MidiInput::new(CLIENT_NAME).map_err(|e| MidiError::Init(e.to_string()))?;
        input.ignore(Ignore::SysexAndActiveSense);
        let ports = input.ports();
        let names: Vec<String> = ports
            .iter()
            .map(|p| input.port_name(p).unwrap_or_default())
            .collect();
        let index = find_port_index(&names, port_name_substring).ok_or_else(|| {
            MidiError::PortNotFound {
                wanted: port_name_substring.to_string(),
                available: names.clone(),
            }
        })?;
        let connection = input
            .connect(
                &ports[index],
                "Adaptaudio in",
                move |_timestamp_us, bytes, _| {
                    if let Some(msg) = MidiMessage::parse(bytes) {
                        on_message(msg);
                    }
                },
                (),
            )
            .map_err(|e| MidiError::Connect(e.to_string()))?;
        Ok(MidiIn {
            port_name: names[index].clone(),
            _connection: connection,
        })
    }

    pub fn port_name(&self) -> &str {
        &self.port_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_lookup_ignores_case_and_takes_the_first_match() {
        let names = vec![
            "IAC Driver Bus 1".to_string(),
            "Hydrasynth Explorer".to_string(),
            "hydrasynth 2".to_string(),
        ];
        assert_eq!(find_port_index(&names, "hydrasynth"), Some(1));
        assert_eq!(find_port_index(&names, "IAC"), Some(0));
        assert_eq!(find_port_index(&names, "MiniFreak"), None);
        assert_eq!(find_port_index(&[], "anything"), None);
    }

    #[test]
    fn panic_messages_cover_every_channel() {
        let messages: Vec<_> = all_notes_off_messages().collect();
        assert_eq!(messages.len(), 32);
        assert!(messages.contains(&MidiMessage::ControlChange {
            channel: 16,
            controller: 123,
            value: 0
        }));
        assert!(messages.contains(&MidiMessage::ControlChange {
            channel: 1,
            controller: 64,
            value: 0
        }));
    }

    #[test]
    fn missing_port_is_an_error_not_a_panic() {
        // Whatever is attached to the test machine, this name will not exist.
        let result = MidiOut::connect("no-such-port-4d1c9a");
        assert!(result.is_err());
        let result = MidiIn::connect("no-such-port-4d1c9a", |_| {});
        assert!(result.is_err());
    }
}
