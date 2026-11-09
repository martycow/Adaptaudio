use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum CaptureError {
    Io(std::io::Error),
    Wav(hound::Error),
    /// A sample file is already there. Adaptaudio never overwrites recordings; pick another
    /// prefix (see [`crate::unused_prefix`]).
    FileExists(PathBuf),
    /// Two captured notes have the same note and velocity, so they would share a file.
    DuplicateTake {
        note: u8,
        velocity: u8,
    },
    /// Nothing usable came out of the recording.
    NothingCaptured,
    /// No audio input matches the name we looked for.
    NoInputDevice {
        wanted: Option<String>,
        available: Vec<String>,
    },
    /// The audio system refused to open or run the input.
    Audio(String),
    /// The input opened but no audio arrived.
    NoAudioArriving,
    Midi(aa_midi::MidiError),
    /// The session was stopped from outside.
    Cancelled,
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaptureError::Io(e) => write!(f, "file error: {e}"),
            CaptureError::Wav(e) => write!(f, "WAV error: {e}"),
            CaptureError::FileExists(path) => {
                write!(f, "{} already exists; not overwriting it", path.display())
            }
            CaptureError::DuplicateTake { note, velocity } => {
                write!(
                    f,
                    "note {} at velocity {velocity} was captured twice",
                    aa_core::music::note_name(*note)
                )
            }
            CaptureError::NothingCaptured => {
                write!(f, "nothing was captured: the recording is silent")
            }
            CaptureError::NoInputDevice { wanted: None, .. } => {
                write!(f, "there is no audio input device")
            }
            CaptureError::NoInputDevice {
                wanted: Some(wanted),
                available,
            } if available.is_empty() => {
                write!(
                    f,
                    "no audio input matches \"{wanted}\": no inputs are connected"
                )
            }
            CaptureError::NoInputDevice {
                wanted: Some(wanted),
                available,
            } => {
                write!(
                    f,
                    "no audio input matches \"{wanted}\" (found: {})",
                    available.join(", ")
                )
            }
            CaptureError::Audio(e) => write!(f, "audio input error: {e}"),
            CaptureError::NoAudioArriving => {
                write!(f, "the audio input is open but sends no audio")
            }
            CaptureError::Midi(e) => write!(f, "{e}"),
            CaptureError::Cancelled => write!(f, "capture cancelled"),
        }
    }
}

impl std::error::Error for CaptureError {}

impl From<std::io::Error> for CaptureError {
    fn from(e: std::io::Error) -> Self {
        CaptureError::Io(e)
    }
}

impl From<hound::Error> for CaptureError {
    fn from(e: hound::Error) -> Self {
        CaptureError::Wav(e)
    }
}

impl From<aa_midi::MidiError> for CaptureError {
    fn from(e: aa_midi::MidiError) -> Self {
        CaptureError::Midi(e)
    }
}
