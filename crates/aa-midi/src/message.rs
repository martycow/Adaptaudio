//! MIDI messages and their wire format.

use std::ops::Deref;

/// A MIDI message Adaptaudio sends or understands.
///
/// Channels are 1..=16, the way synths show them on screen (the wire format uses 0..=15).
/// Data values (note, velocity, controller, value, program) are 0..=127.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MidiMessage {
    NoteOn {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
    ProgramChange {
        channel: u8,
        program: u8,
    },
    /// Pitch bend, -8192..=8191, where 0 means "no bend".
    PitchBend {
        channel: u8,
        value: i16,
    },
    /// One MIDI Clock tick. There are 24 per quarter note.
    Clock,
    Start,
    Stop,
    Continue,
}

/// Controller number of "All Notes Off".
pub const CC_ALL_NOTES_OFF: u8 = 123;
/// Controller number of the sustain pedal.
pub const CC_SUSTAIN: u8 = 64;

const NOTE_OFF: u8 = 0x80;
const NOTE_ON: u8 = 0x90;
const CONTROL_CHANGE: u8 = 0xB0;
const PROGRAM_CHANGE: u8 = 0xC0;
const PITCH_BEND: u8 = 0xE0;
const CLOCK: u8 = 0xF8;
const START: u8 = 0xFA;
const CONTINUE: u8 = 0xFB;
const STOP: u8 = 0xFC;

/// The bytes of one message: up to three, kept on the stack so sending never allocates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MidiBytes {
    buf: [u8; 3],
    len: u8,
}

impl MidiBytes {
    fn new(bytes: &[u8]) -> Self {
        let mut buf = [0; 3];
        let len = bytes.len().min(3);
        buf[..len].copy_from_slice(&bytes[..len]);
        MidiBytes {
            buf,
            len: len as u8,
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..usize::from(self.len)]
    }
}

impl Deref for MidiBytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

/// Turns a 1..=16 channel into the 0..=15 nibble. Out-of-range channels are clamped.
fn channel_nibble(channel: u8) -> u8 {
    channel.clamp(1, 16) - 1
}

impl MidiMessage {
    /// The message as it goes over the wire. Out-of-range values are clamped to fit.
    pub fn to_bytes(&self) -> MidiBytes {
        match *self {
            MidiMessage::NoteOn {
                channel,
                note,
                velocity,
            } => MidiBytes::new(&[
                NOTE_ON | channel_nibble(channel),
                note.min(127),
                velocity.min(127),
            ]),
            MidiMessage::NoteOff {
                channel,
                note,
                velocity,
            } => MidiBytes::new(&[
                NOTE_OFF | channel_nibble(channel),
                note.min(127),
                velocity.min(127),
            ]),
            MidiMessage::ControlChange {
                channel,
                controller,
                value,
            } => MidiBytes::new(&[
                CONTROL_CHANGE | channel_nibble(channel),
                controller.min(127),
                value.min(127),
            ]),
            MidiMessage::ProgramChange { channel, program } => {
                MidiBytes::new(&[PROGRAM_CHANGE | channel_nibble(channel), program.min(127)])
            }
            MidiMessage::PitchBend { channel, value } => {
                let raw = (i32::from(value).clamp(-8192, 8191) + 8192) as u16;
                MidiBytes::new(&[
                    PITCH_BEND | channel_nibble(channel),
                    (raw & 0x7F) as u8,
                    (raw >> 7) as u8,
                ])
            }
            MidiMessage::Clock => MidiBytes::new(&[CLOCK]),
            MidiMessage::Start => MidiBytes::new(&[START]),
            MidiMessage::Stop => MidiBytes::new(&[STOP]),
            MidiMessage::Continue => MidiBytes::new(&[CONTINUE]),
        }
    }

    /// Reads one complete message. Returns `None` for messages Adaptaudio does not use
    /// (aftertouch, SysEx, active sensing...) and for malformed bytes.
    ///
    /// A Note On with velocity 0 is read as a Note Off, as the MIDI standard says.
    pub fn parse(bytes: &[u8]) -> Option<MidiMessage> {
        let status = *bytes.first()?;
        if status < 0x80 {
            return None;
        }
        let data = |i: usize| bytes.get(i).copied().filter(|b| *b < 0x80);
        let channel = (status & 0x0F) + 1;
        match status & 0xF0 {
            NOTE_OFF => Some(MidiMessage::NoteOff {
                channel,
                note: data(1)?,
                velocity: data(2)?,
            }),
            NOTE_ON => {
                let (note, velocity) = (data(1)?, data(2)?);
                if velocity == 0 {
                    Some(MidiMessage::NoteOff {
                        channel,
                        note,
                        velocity: 0,
                    })
                } else {
                    Some(MidiMessage::NoteOn {
                        channel,
                        note,
                        velocity,
                    })
                }
            }
            CONTROL_CHANGE => Some(MidiMessage::ControlChange {
                channel,
                controller: data(1)?,
                value: data(2)?,
            }),
            PROGRAM_CHANGE => Some(MidiMessage::ProgramChange {
                channel,
                program: data(1)?,
            }),
            PITCH_BEND => {
                let raw = u16::from(data(1)?) | (u16::from(data(2)?) << 7);
                Some(MidiMessage::PitchBend {
                    channel,
                    value: raw as i16 - 8192,
                })
            }
            _ => match status {
                CLOCK => Some(MidiMessage::Clock),
                START => Some(MidiMessage::Start),
                STOP => Some(MidiMessage::Stop),
                CONTINUE => Some(MidiMessage::Continue),
                _ => None,
            },
        }
    }

    /// The channel (1..=16) of a channel message, `None` for clock and transport messages.
    pub fn channel(&self) -> Option<u8> {
        match *self {
            MidiMessage::NoteOn { channel, .. }
            | MidiMessage::NoteOff { channel, .. }
            | MidiMessage::ControlChange { channel, .. }
            | MidiMessage::ProgramChange { channel, .. }
            | MidiMessage::PitchBend { channel, .. } => Some(channel),
            MidiMessage::Clock | MidiMessage::Start | MidiMessage::Stop | MidiMessage::Continue => {
                None
            }
        }
    }

    /// True for Note On and Note Off: the messages a hardware keyboard sends when you play.
    pub fn is_note(&self) -> bool {
        matches!(
            self,
            MidiMessage::NoteOn { .. } | MidiMessage::NoteOff { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_on_encodes_with_zero_based_channel() {
        let msg = MidiMessage::NoteOn {
            channel: 1,
            note: 60,
            velocity: 100,
        };
        assert_eq!(msg.to_bytes().as_slice(), &[0x90, 60, 100]);
        let msg = MidiMessage::NoteOff {
            channel: 16,
            note: 61,
            velocity: 0,
        };
        assert_eq!(&*msg.to_bytes(), &[0x8F, 61, 0]);
    }

    #[test]
    fn control_program_and_realtime_encode() {
        let cc = MidiMessage::ControlChange {
            channel: 3,
            controller: 74,
            value: 127,
        };
        assert_eq!(&*cc.to_bytes(), &[0xB2, 74, 127]);
        let pc = MidiMessage::ProgramChange {
            channel: 10,
            program: 5,
        };
        assert_eq!(&*pc.to_bytes(), &[0xC9, 5]);
        assert_eq!(&*MidiMessage::Clock.to_bytes(), &[0xF8]);
        assert_eq!(&*MidiMessage::Start.to_bytes(), &[0xFA]);
        assert_eq!(&*MidiMessage::Continue.to_bytes(), &[0xFB]);
        assert_eq!(&*MidiMessage::Stop.to_bytes(), &[0xFC]);
    }

    #[test]
    fn pitch_bend_encodes_center_and_extremes() {
        let bend = |value| MidiMessage::PitchBend { channel: 1, value }.to_bytes();
        assert_eq!(&*bend(0), &[0xE0, 0x00, 0x40]);
        assert_eq!(&*bend(-8192), &[0xE0, 0x00, 0x00]);
        assert_eq!(&*bend(8191), &[0xE0, 0x7F, 0x7F]);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let msg = MidiMessage::NoteOn {
            channel: 0,
            note: 200,
            velocity: 200,
        };
        assert_eq!(&*msg.to_bytes(), &[0x90, 127, 127]);
        let msg = MidiMessage::NoteOn {
            channel: 40,
            note: 1,
            velocity: 1,
        };
        assert_eq!(msg.to_bytes()[0], 0x9F);
    }

    #[test]
    fn every_message_round_trips() {
        let messages = [
            MidiMessage::NoteOn {
                channel: 1,
                note: 0,
                velocity: 1,
            },
            MidiMessage::NoteOn {
                channel: 16,
                note: 127,
                velocity: 127,
            },
            MidiMessage::NoteOff {
                channel: 5,
                note: 64,
                velocity: 64,
            },
            MidiMessage::ControlChange {
                channel: 2,
                controller: 1,
                value: 0,
            },
            MidiMessage::ProgramChange {
                channel: 7,
                program: 127,
            },
            MidiMessage::PitchBend {
                channel: 9,
                value: -8192,
            },
            MidiMessage::PitchBend {
                channel: 9,
                value: 0,
            },
            MidiMessage::PitchBend {
                channel: 9,
                value: 1234,
            },
            MidiMessage::PitchBend {
                channel: 9,
                value: 8191,
            },
            MidiMessage::Clock,
            MidiMessage::Start,
            MidiMessage::Stop,
            MidiMessage::Continue,
        ];
        for msg in messages {
            assert_eq!(MidiMessage::parse(&msg.to_bytes()), Some(msg), "{msg:?}");
        }
    }

    #[test]
    fn note_on_with_zero_velocity_reads_as_note_off() {
        assert_eq!(
            MidiMessage::parse(&[0x93, 60, 0]),
            Some(MidiMessage::NoteOff {
                channel: 4,
                note: 60,
                velocity: 0
            })
        );
    }

    #[test]
    fn junk_and_unsupported_messages_are_rejected() {
        assert_eq!(MidiMessage::parse(&[]), None);
        assert_eq!(
            MidiMessage::parse(&[60, 100]),
            None,
            "data byte without status"
        );
        assert_eq!(MidiMessage::parse(&[0x90, 60]), None, "truncated");
        assert_eq!(
            MidiMessage::parse(&[0x90, 0x80, 1]),
            None,
            "data byte with the high bit set"
        );
        assert_eq!(MidiMessage::parse(&[0xD0, 10]), None, "channel aftertouch");
        assert_eq!(MidiMessage::parse(&[0xF0, 0x7E, 0xF7]), None, "sysex");
        assert_eq!(MidiMessage::parse(&[0xFE]), None, "active sensing");
    }

    #[test]
    fn channel_and_note_helpers() {
        assert_eq!(
            MidiMessage::NoteOn {
                channel: 3,
                note: 1,
                velocity: 1
            }
            .channel(),
            Some(3)
        );
        assert_eq!(MidiMessage::Clock.channel(), None);
        assert!(
            MidiMessage::NoteOff {
                channel: 1,
                note: 1,
                velocity: 0
            }
            .is_note()
        );
        assert!(
            !MidiMessage::ControlChange {
                channel: 1,
                controller: 1,
                value: 0
            }
            .is_note()
        );
    }
}
