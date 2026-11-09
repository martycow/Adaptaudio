//! Running a capture on real hardware: play the plan over MIDI and record the audio input.
//!
//! The input callback only converts samples to mono and pushes them into a lock-free ring
//! buffer; the session thread moves them into a buffer allocated up front, and sends the plan's
//! MIDI at the right moments. Everything fails with a [`CaptureError`] when there is no input.

use crate::error::CaptureError;
use crate::latency::measure_latency;
use crate::plan::{CapturePlan, TimedMidi, ms_to_samples};
use aa_midi::{CC_ALL_NOTES_OFF, MidiMessage, MidiOut, find_port_index};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Where the session sends the plan's notes. A [`MidiOut`] works directly, and so does any
/// `FnMut(MidiMessage)` closure (for example one that pushes to a `MidiScheduler` producer).
pub trait NoteSender {
    fn send_midi(&mut self, msg: MidiMessage) -> Result<(), CaptureError>;
}

impl NoteSender for MidiOut {
    fn send_midi(&mut self, msg: MidiMessage) -> Result<(), CaptureError> {
        Ok(self.send(&msg)?)
    }
}

impl<F: FnMut(MidiMessage)> NoteSender for F {
    fn send_midi(&mut self, msg: MidiMessage) -> Result<(), CaptureError> {
        self(msg);
        Ok(())
    }
}

/// How long to wait for the audio system to list its devices.
const DEVICE_LIST_TIMEOUT: Duration = Duration::from_secs(5);

/// Names of all audio inputs, e.g. "H4essential".
///
/// Asking macOS about inputs can block while it waits for microphone permission. This gives
/// up after a few seconds with [`CaptureError::Audio`] instead of freezing the caller (the
/// helper thread is left to finish on its own).
pub fn list_input_devices() -> Result<Vec<String>, CaptureError> {
    let (reply, answer) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("aa-capture-devices".into())
        .spawn(move || {
            let _ = reply.send(list_input_devices_blocking());
        })
        .map_err(|e| CaptureError::Audio(e.to_string()))?;
    answer.recv_timeout(DEVICE_LIST_TIMEOUT).unwrap_or_else(|_| {
        Err(CaptureError::Audio(
            "the audio system did not answer; check microphone access in System Settings > Privacy & Security"
                .to_string(),
        ))
    })
}

fn list_input_devices_blocking() -> Result<Vec<String>, CaptureError> {
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|e| CaptureError::Audio(e.to_string()))?;
    Ok(devices.filter_map(|d| d.name().ok()).collect())
}

/// Which input channel to record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InputChannel {
    /// Average all channels into mono.
    #[default]
    Mix,
    /// One channel, counting from 0 (left = 0, right = 1).
    Only(usize),
}

/// Live progress of a running session, readable from the UI thread.
#[derive(Debug, Default)]
pub struct CaptureProgress {
    takes_started: AtomicU32,
    takes_total: AtomicU32,
    samples_recorded: AtomicU64,
    samples_total: AtomicU64,
    overruns: AtomicU32,
    cancel: AtomicBool,
}

impl CaptureProgress {
    /// How many takes have had their key pressed so far.
    pub fn takes_started(&self) -> u32 {
        self.takes_started.load(Ordering::Relaxed)
    }

    pub fn takes_total(&self) -> u32 {
        self.takes_total.load(Ordering::Relaxed)
    }

    /// 0..1 of the recording done.
    pub fn fraction(&self) -> f32 {
        let total = self.samples_total.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        (self.samples_recorded.load(Ordering::Relaxed) as f64 / total as f64).min(1.0) as f32
    }

    /// Samples the input produced that did not fit in the ring buffer. Anything above zero
    /// means the recording has gaps and the slices may be off.
    pub fn overruns(&self) -> u32 {
        self.overruns.load(Ordering::Relaxed)
    }

    /// Asks the running session to stop. It releases all notes and returns
    /// [`CaptureError::Cancelled`].
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn reset(&self, takes_total: usize, samples_total: usize) {
        self.takes_started.store(0, Ordering::Relaxed);
        self.takes_total
            .store(takes_total as u32, Ordering::Relaxed);
        self.samples_recorded.store(0, Ordering::Relaxed);
        self.samples_total
            .store(samples_total as u64, Ordering::Relaxed);
        self.overruns.store(0, Ordering::Relaxed);
        self.cancel.store(false, Ordering::Relaxed);
    }
}

/// A finished recording.
#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    /// Mono samples. Index 0 is the first sample the input delivered; the plan's offsets count
    /// from there.
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub device_name: String,
    /// Samples lost because the session thread fell behind. Normally 0.
    pub overruns: u32,
}

/// Recorded after the last take, so a late release tail still fits.
const TAIL_MS: u64 = 1000;
/// How long to wait for the first audio before giving up.
const FIRST_AUDIO_TIMEOUT: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(1);
const SPIN_WINDOW: Duration = Duration::from_micros(1500);

/// Plays a [`CapturePlan`] on a synth and records the whole take from an audio input.
///
/// It blocks the calling thread until done, so run it on its own thread and watch
/// [`CaptureSession::progress`] from the UI.
#[derive(Debug, Default)]
pub struct CaptureSession {
    device: Option<String>,
    input_channel: InputChannel,
    progress: Arc<CaptureProgress>,
}

impl CaptureSession {
    /// A session on the first input whose name contains `device_name_substring` (ignoring
    /// case), or the system default input for `None`.
    pub fn new(device_name_substring: Option<&str>) -> Self {
        CaptureSession {
            device: device_name_substring.map(str::to_string),
            ..Default::default()
        }
    }

    pub fn with_input_channel(mut self, channel: InputChannel) -> Self {
        self.input_channel = channel;
        self
    }

    /// Shared progress; clone the `Arc` into the UI.
    pub fn progress(&self) -> Arc<CaptureProgress> {
        Arc::clone(&self.progress)
    }

    /// Plays every take of `plan` through `sender` and returns the continuous recording,
    /// ready for [`crate::slice_recording`].
    pub fn run(
        &self,
        plan: &CapturePlan,
        sender: &mut dyn NoteSender,
    ) -> Result<Recording, CaptureError> {
        let total_ms = plan.total_duration().as_millis() as u64;
        self.record(
            &plan.midi_events(),
            plan.take_count(),
            total_ms + TAIL_MS,
            sender,
        )
    }

    /// Plays one note and measures, in milliseconds, how long it takes to come back on the
    /// input. Store the result in `ExternalInstrument::latency_ms`.
    pub fn measure_midi_latency(
        &self,
        sender: &mut dyn NoteSender,
        channel: u8,
        note: u8,
        velocity: u8,
    ) -> Result<f32, CaptureError> {
        let mut plan = CapturePlan::every_nth_semitone(note, note, 1, &[velocity]);
        plan.channel = channel;
        plan.hold_ms = 300;
        plan.release_ms = 400;
        plan.gap_ms = 0;
        plan.pre_roll_ms = 500;
        let recording = self.run(&plan, sender)?;
        let sent_at = ms_to_samples(plan.take_start_ms(0), recording.sample_rate);
        measure_latency(sent_at, &recording.samples, recording.sample_rate)
            .ok_or(CaptureError::NothingCaptured)
    }

    fn find_device(&self) -> Result<cpal::Device, CaptureError> {
        // Fails fast (instead of hanging) when the audio system does not respond.
        let available = list_input_devices()?;
        if let Some(wanted) = &self.device
            && find_port_index(&available, wanted).is_none()
        {
            return Err(CaptureError::NoInputDevice {
                wanted: Some(wanted.clone()),
                available,
            });
        }
        let host = cpal::default_host();
        match &self.device {
            None => host
                .default_input_device()
                .ok_or(CaptureError::NoInputDevice {
                    wanted: None,
                    available: Vec::new(),
                }),
            Some(wanted) => {
                let devices: Vec<cpal::Device> = host
                    .input_devices()
                    .map_err(|e| CaptureError::Audio(e.to_string()))?
                    .collect();
                let names: Vec<String> = devices
                    .iter()
                    .map(|d| d.name().unwrap_or_default())
                    .collect();
                let index =
                    find_port_index(&names, wanted).ok_or_else(|| CaptureError::NoInputDevice {
                        wanted: Some(wanted.clone()),
                        available: names,
                    })?;
                devices
                    .into_iter()
                    .nth(index)
                    .ok_or(CaptureError::NoInputDevice {
                        wanted: Some(wanted.clone()),
                        available: Vec::new(),
                    })
            }
        }
    }

    fn record(
        &self,
        events: &[TimedMidi],
        take_total: usize,
        length_ms: u64,
        sender: &mut dyn NoteSender,
    ) -> Result<Recording, CaptureError> {
        let device = self.find_device()?;
        let device_name = device.name().unwrap_or_default();
        let supported = device
            .default_input_config()
            .map_err(|e| CaptureError::Audio(e.to_string()))?;
        let config = supported.config();
        let sample_rate = config.sample_rate.0;

        let target = ms_to_samples(length_ms, sample_rate);
        let mut samples: Vec<f32> = Vec::with_capacity(target);
        self.progress.reset(take_total, target);
        let (producer, mut consumer) = rtrb::RingBuffer::new(sample_rate as usize);
        let timing = Arc::new(InputTiming::new());
        let stream_error = Arc::new(Mutex::new(None::<String>));
        let stream = build_stream(
            supported.sample_format(),
            (
                &device,
                &config,
                self.input_channel,
                producer,
                Arc::clone(&self.progress),
                Arc::clone(&timing),
                Arc::clone(&stream_error),
            ),
        )?;
        stream
            .play()
            .map_err(|e| CaptureError::Audio(e.to_string()))?;

        let opened = Instant::now();
        let mut held = [false; 128];
        let mut next_event = 0;
        let result = loop {
            if self.progress.is_cancelled() {
                break Err(CaptureError::Cancelled);
            }
            if let Ok(guard) = stream_error.lock()
                && let Some(message) = guard.as_ref()
            {
                break Err(CaptureError::Audio(message.clone()));
            }

            let available = consumer.slots().min(target - samples.len());
            if let Ok(chunk) = consumer.read_chunk(available) {
                let (a, b) = chunk.as_slices();
                samples.extend_from_slice(a);
                samples.extend_from_slice(b);
                chunk.commit_all();
            }
            // Keep the ring empty even after the target is reached.
            if samples.len() >= target {
                while consumer.pop().is_ok() {}
            }
            self.progress
                .samples_recorded
                .store(samples.len() as u64, Ordering::Relaxed);

            let Some(anchor) = timing.anchor() else {
                if opened.elapsed() > FIRST_AUDIO_TIMEOUT {
                    break Err(CaptureError::NoAudioArriving);
                }
                std::thread::sleep(POLL);
                continue;
            };

            let now = Instant::now();
            let mut failed = None;
            while let Some(event) = events.get(next_event) {
                let due =
                    anchor.instant_of(ms_to_samples(event.offset_ms, sample_rate), sample_rate);
                if due > now {
                    break;
                }
                if let Err(e) = sender.send_midi(event.msg) {
                    failed = Some(e);
                    break;
                }
                match event.msg {
                    MidiMessage::NoteOn { note, .. } => {
                        held[usize::from(note.min(127))] = true;
                        self.progress
                            .takes_started
                            .store(event.take as u32 + 1, Ordering::Relaxed);
                    }
                    MidiMessage::NoteOff { note, .. } => held[usize::from(note.min(127))] = false,
                    _ => {}
                }
                next_event += 1;
            }
            if let Some(e) = failed {
                break Err(e);
            }
            if samples.len() >= target && next_event >= events.len() {
                break Ok(());
            }

            let next_due = events
                .get(next_event)
                .map(|e| anchor.instant_of(ms_to_samples(e.offset_ms, sample_rate), sample_rate));
            match next_due {
                Some(due) if due.saturating_duration_since(now) <= SPIN_WINDOW => {
                    while Instant::now() < due {
                        std::hint::spin_loop();
                    }
                }
                _ => std::thread::sleep(POLL),
            }
        };
        drop(stream);

        // Never leave a note hanging, whatever happened.
        let channel = events.iter().find_map(|e| e.msg.channel()).unwrap_or(1);
        for (note, _) in held.iter().enumerate().filter(|(_, h)| **h) {
            let _ = sender.send_midi(MidiMessage::NoteOff {
                channel,
                note: note as u8,
                velocity: 0,
            });
        }
        let _ = sender.send_midi(MidiMessage::ControlChange {
            channel,
            controller: CC_ALL_NOTES_OFF,
            value: 0,
        });

        result?;
        Ok(Recording {
            samples,
            sample_rate,
            device_name,
            overruns: self.progress.overruns(),
        })
    }
}

/// When the first recorded sample was captured, as written by the input callback.
#[derive(Debug)]
struct InputTiming {
    base: Instant,
    /// Nanoseconds after `base` at which sample 0 was captured.
    first_sample_nanos: AtomicU64,
    ready: AtomicBool,
}

impl InputTiming {
    fn new() -> Self {
        InputTiming {
            base: Instant::now(),
            first_sample_nanos: AtomicU64::new(0),
            ready: AtomicBool::new(false),
        }
    }

    /// Called on the audio thread for the first buffer: `age` is how long ago its first
    /// sample was captured, as the driver reports it.
    fn mark_first_buffer(&self, age: Duration) {
        if self.ready.load(Ordering::Relaxed) {
            return;
        }
        let captured = Instant::now()
            .checked_sub(age)
            .unwrap_or(self.base)
            .max(self.base);
        self.first_sample_nanos.store(
            captured.duration_since(self.base).as_nanos() as u64,
            Ordering::Relaxed,
        );
        self.ready.store(true, Ordering::Release);
    }

    fn anchor(&self) -> Option<Anchor> {
        if !self.ready.load(Ordering::Acquire) {
            return None;
        }
        Some(Anchor {
            first_sample: self.base
                + Duration::from_nanos(self.first_sample_nanos.load(Ordering::Relaxed)),
        })
    }
}

/// Maps a sample position of the recording to a wall-clock moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Anchor {
    first_sample: Instant,
}

impl Anchor {
    fn instant_of(&self, sample: usize, sample_rate: u32) -> Instant {
        let nanos = sample as u128 * 1_000_000_000 / u128::from(sample_rate.max(1));
        self.first_sample + Duration::from_nanos(nanos as u64)
    }
}

/// Turns one input frame into a mono sample.
fn frame_to_mono<T: Copy>(frame: &[T], channel: InputChannel, convert: impl Fn(T) -> f32) -> f32 {
    match channel {
        InputChannel::Mix if !frame.is_empty() => {
            frame.iter().map(|s| convert(*s)).sum::<f32>() / frame.len() as f32
        }
        InputChannel::Mix => 0.0,
        InputChannel::Only(c) => frame.get(c).or(frame.first()).map_or(0.0, |s| convert(*s)),
    }
}

fn build_stream(
    format: cpal::SampleFormat,
    args: StreamArgs<'_>,
) -> Result<cpal::Stream, CaptureError> {
    use cpal::SampleFormat as F;
    match format {
        F::F32 => build_typed::<f32>(args),
        F::F64 => build_typed::<f64>(args),
        F::I8 => build_typed::<i8>(args),
        F::I16 => build_typed::<i16>(args),
        F::I32 => build_typed::<i32>(args),
        F::U8 => build_typed::<u8>(args),
        F::U16 => build_typed::<u16>(args),
        F::U32 => build_typed::<u32>(args),
        other => Err(CaptureError::Audio(format!(
            "unsupported input sample format {other}"
        ))),
    }
}

type StreamArgs<'a> = (
    &'a cpal::Device,
    &'a cpal::StreamConfig,
    InputChannel,
    rtrb::Producer<f32>,
    Arc<CaptureProgress>,
    Arc<InputTiming>,
    Arc<Mutex<Option<String>>>,
);

fn build_typed<T>(args: StreamArgs<'_>) -> Result<cpal::Stream, CaptureError>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let (device, config, channel, mut producer, progress, timing, stream_error) = args;
    let channels = usize::from(config.channels.max(1));
    let on_data = move |data: &[T], info: &cpal::InputCallbackInfo| {
        let stamp = info.timestamp();
        timing.mark_first_buffer(
            stamp
                .callback
                .duration_since(&stamp.capture)
                .unwrap_or_default(),
        );
        for frame in data.chunks(channels) {
            let value = frame_to_mono(frame, channel, <f32 as cpal::FromSample<T>>::from_sample_);
            if producer.push(value).is_err() {
                progress.overruns.fetch_add(1, Ordering::Relaxed);
            }
        }
    };
    let on_error = move |e: cpal::StreamError| {
        if let Ok(mut slot) = stream_error.lock() {
            slot.get_or_insert_with(|| e.to_string());
        }
    };
    device
        .build_input_stream::<T, _, _>(config, on_data, on_error, None)
        .map_err(|e| CaptureError::Audio(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closures_are_note_senders() {
        let mut sent = Vec::new();
        {
            let mut sender = |m: MidiMessage| sent.push(m);
            let sender: &mut dyn NoteSender = &mut sender;
            sender.send_midi(MidiMessage::Start).unwrap();
        }
        assert_eq!(sent, vec![MidiMessage::Start]);
    }

    // Touches CoreAudio, which can wait on the macOS microphone permission prompt.
    // Run with `cargo test -p aa-capture -- --ignored` on a machine with audio access.
    #[test]
    #[ignore]
    fn missing_input_device_is_an_error_not_a_panic() {
        let session = CaptureSession::new(Some("no-such-input-7f3e1b"));
        let mut sent = Vec::new();
        let result = session.run(&CapturePlan::default(), &mut |m: MidiMessage| sent.push(m));
        assert!(matches!(
            result,
            Err(CaptureError::NoInputDevice { .. }) | Err(CaptureError::Audio(_))
        ));
        assert!(sent.is_empty(), "no notes are played without an input");
    }

    #[test]
    #[ignore]
    fn listing_inputs_does_not_panic() {
        let _ = list_input_devices();
    }

    #[test]
    fn anchor_maps_samples_to_time() {
        let start = Instant::now();
        let anchor = Anchor {
            first_sample: start,
        };
        assert_eq!(anchor.instant_of(0, 48_000), start);
        assert_eq!(
            anchor.instant_of(48_000, 48_000),
            start + Duration::from_secs(1)
        );
        assert_eq!(
            anchor.instant_of(24, 48_000),
            start + Duration::from_micros(500)
        );
    }

    #[test]
    fn input_timing_is_set_once() {
        let timing = InputTiming::new();
        assert!(timing.anchor().is_none());
        timing.mark_first_buffer(Duration::ZERO);
        let first = timing.anchor().unwrap();
        std::thread::sleep(Duration::from_millis(2));
        timing.mark_first_buffer(Duration::ZERO);
        assert_eq!(timing.anchor(), Some(first));
    }

    #[test]
    fn frames_become_mono() {
        let id = |x: f32| x;
        assert!((frame_to_mono(&[0.2, 0.4], InputChannel::Mix, id) - 0.3).abs() < 1e-6);
        assert_eq!(frame_to_mono(&[0.2, 0.4], InputChannel::Only(1), id), 0.4);
        assert_eq!(
            frame_to_mono(&[0.2], InputChannel::Only(5), id),
            0.2,
            "falls back to the first channel"
        );
        assert_eq!(frame_to_mono::<f32>(&[], InputChannel::Mix, id), 0.0);
        let from_i16 = <f32 as cpal::FromSample<i16>>::from_sample_;
        assert_eq!(
            frame_to_mono(&[i16::MIN], InputChannel::Mix, from_i16),
            -1.0
        );
    }

    #[test]
    fn progress_reports_fraction_and_cancel() {
        let progress = CaptureProgress::default();
        assert_eq!(progress.fraction(), 0.0);
        progress.reset(10, 1000);
        progress.samples_recorded.store(250, Ordering::Relaxed);
        assert_eq!(progress.fraction(), 0.25);
        assert_eq!(progress.takes_total(), 10);
        progress.cancel();
        assert!(progress.is_cancelled());
    }
}
