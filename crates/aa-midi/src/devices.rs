//! The studio's hardware, recognised by port name.

use aa_core::ExternalInstrument;

/// A piece of gear Adaptaudio knows by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KnownDevice {
    /// Text found in the device's MIDI port or audio device name.
    pub port_hint: &'static str,
    pub friendly_name: &'static str,
    /// The MIDI channel the device listens on out of the box (1..=16).
    pub default_channel: u8,
    /// True for the audio interface: it records the synths but plays no notes.
    pub is_audio_interface: bool,
    /// What the device does in Adaptaudio, in one line.
    pub role: &'static str,
}

/// The studio's gear. Matching ignores case.
pub const KNOWN_DEVICES: &[KnownDevice] = &[
    KnownDevice {
        port_hint: "Hydrasynth",
        friendly_name: "ASM Hydrasynth Explorer",
        default_channel: 1,
        is_audio_interface: false,
        role: "Main synth: pads, basses, leads. Live and Capture.",
    },
    KnownDevice {
        port_hint: "MiniFreak",
        friendly_name: "Arturia MiniFreak",
        default_channel: 1,
        is_audio_interface: false,
        role: "Second synth: arpeggios and digital textures. Live and Capture.",
    },
    KnownDevice {
        port_hint: "EP-1320",
        friendly_name: "Teenage Engineering EP-1320",
        default_channel: 1,
        is_audio_interface: false,
        role: "Sampler groovebox: drums and lo-fi textures. Follows MIDI Clock.",
    },
    KnownDevice {
        port_hint: "H4essential",
        friendly_name: "Zoom H4essential",
        default_channel: 1,
        is_audio_interface: true,
        role: "USB audio interface for Live and Capture, and field recorder.",
    },
];

/// The known device whose hint appears in `port_name`, ignoring case.
pub fn guess_device(port_name: &str) -> Option<&'static KnownDevice> {
    let name = port_name.to_lowercase();
    KNOWN_DEVICES
        .iter()
        .find(|d| name.contains(&d.port_hint.to_lowercase()))
}

/// A starting [`ExternalInstrument`] for a port: the device's default channel (1 if unknown)
/// and no latency compensation until it is measured.
pub fn external_instrument_for(port_name: &str) -> ExternalInstrument {
    let channel = guess_device(port_name).map_or(1, |d| d.default_channel);
    ExternalInstrument {
        port_name: port_name.to_string(),
        channel,
        latency_ms: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devices_are_recognised_from_real_looking_port_names() {
        assert_eq!(
            guess_device("Hydrasynth Explorer").map(|d| d.friendly_name),
            Some("ASM Hydrasynth Explorer")
        );
        assert_eq!(
            guess_device("Arturia MiniFreak MIDI").map(|d| d.port_hint),
            Some("MiniFreak")
        );
        assert_eq!(
            guess_device("ep-1320 medieval").map(|d| d.port_hint),
            Some("EP-1320")
        );
        assert!(guess_device("H4essential").is_some_and(|d| d.is_audio_interface));
        assert_eq!(guess_device("IAC Driver Bus 1"), None);
    }

    #[test]
    fn channels_are_valid() {
        assert!(
            KNOWN_DEVICES
                .iter()
                .all(|d| (1..=16).contains(&d.default_channel))
        );
    }

    #[test]
    fn external_instrument_uses_the_known_channel() {
        let inst = external_instrument_for("Hydrasynth Explorer");
        assert_eq!(inst.port_name, "Hydrasynth Explorer");
        assert_eq!(inst.channel, 1);
        assert_eq!(inst.latency_ms, 0.0);
    }
}
