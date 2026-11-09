//! What to play during a capture: which notes, how hard, and for how long.

use aa_midi::MidiMessage;
use std::time::Duration;

/// A capture take list. Every note is played at every velocity, note by note:
/// C2 soft, C2 medium, C2 hard, D#2 soft...
///
/// Each take lasts `hold_ms` (key down) + `release_ms` (key up, the tail rings out) +
/// `gap_ms` (silence before the next take). The take starts after `pre_roll_ms` of silence,
/// which also gives the noise floor for analysis.
#[derive(Clone, Debug, PartialEq)]
pub struct CapturePlan {
    pub notes: Vec<u8>,
    pub velocities: Vec<u8>,
    pub hold_ms: u32,
    pub release_ms: u32,
    pub gap_ms: u32,
    /// MIDI channel 1..=16.
    pub channel: u8,
    pub pre_roll_ms: u32,
}

/// One note at one velocity, at its place in the take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlannedTake {
    pub index: usize,
    pub note: u8,
    pub velocity: u8,
    /// When the key goes down, from the start of the recording.
    pub start_ms: u64,
}

/// A MIDI message at a time measured from the start of the recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimedMidi {
    pub offset_ms: u64,
    pub msg: MidiMessage,
    /// Which take this message belongs to.
    pub take: usize,
}

impl Default for CapturePlan {
    /// Every 3rd semitone from C2 to C6 (four octaves) at three velocities: 51 takes, about
    /// 3.6 minutes.
    fn default() -> Self {
        CapturePlan::every_nth_semitone(36, 84, 3, &[45, 90, 127])
    }
}

impl CapturePlan {
    /// Notes from `low` to `high` (both included) every `step` semitones, at each velocity.
    /// Velocities are sorted, de-duplicated and kept in 1..=127; none means a single 100.
    pub fn every_nth_semitone(low: u8, high: u8, step: u8, velocities: &[u8]) -> Self {
        let (low, high) = (low.min(high).min(127), high.max(low).min(127));
        let notes = (low..=high).step_by(usize::from(step.max(1))).collect();
        let mut velocities: Vec<u8> = velocities.iter().map(|v| (*v).clamp(1, 127)).collect();
        velocities.sort_unstable();
        velocities.dedup();
        if velocities.is_empty() {
            velocities.push(100);
        }
        CapturePlan {
            notes,
            velocities,
            hold_ms: 2500,
            release_ms: 1500,
            gap_ms: 250,
            channel: 1,
            pre_roll_ms: 500,
        }
    }

    /// Time from one key-down to the next.
    pub fn slot_ms(&self) -> u64 {
        u64::from(self.hold_ms) + u64::from(self.release_ms) + u64::from(self.gap_ms)
    }

    pub fn take_count(&self) -> usize {
        self.notes.len() * self.velocities.len()
    }

    /// When take `index` starts, from the start of the recording.
    pub fn take_start_ms(&self, index: usize) -> u64 {
        u64::from(self.pre_roll_ms) + index as u64 * self.slot_ms()
    }

    /// Every take in playing order.
    pub fn takes(&self) -> Vec<PlannedTake> {
        let pairs = self.notes.iter().flat_map(|&note| {
            self.velocities
                .iter()
                .map(move |&velocity| (note, velocity))
        });
        pairs
            .enumerate()
            .map(|(index, (note, velocity))| PlannedTake {
                index,
                note,
                velocity,
                start_ms: self.take_start_ms(index),
            })
            .collect()
    }

    /// Length of the whole recording: pre-roll plus every take.
    pub fn total_duration(&self) -> Duration {
        Duration::from_millis(self.take_start_ms(self.take_count()))
    }

    /// The Note On and Note Off of every take, in time order.
    pub fn midi_events(&self) -> Vec<TimedMidi> {
        let channel = self.channel.clamp(1, 16);
        let mut events = Vec::with_capacity(self.take_count() * 2);
        for take in self.takes() {
            let (note, velocity) = (take.note, take.velocity);
            events.push(TimedMidi {
                offset_ms: take.start_ms,
                msg: MidiMessage::NoteOn {
                    channel,
                    note,
                    velocity,
                },
                take: take.index,
            });
            events.push(TimedMidi {
                offset_ms: take.start_ms + u64::from(self.hold_ms),
                msg: MidiMessage::NoteOff {
                    channel,
                    note,
                    velocity: 0,
                },
                take: take.index,
            });
        }
        events
    }
}

/// MIDI Start at `start_ms`, 24 clock ticks per beat at `bpm` for `length_ms`, then Stop: what
/// a groovebox needs to play its pattern in time while it is being recorded.
pub fn clock_events(bpm: f32, start_ms: u64, length_ms: u64) -> Vec<TimedMidi> {
    let tick_ms = 60_000.0 / f64::from(bpm.clamp(20.0, 400.0)) / 24.0;
    let ticks = (length_ms as f64 / tick_ms).ceil() as u64;
    let mut events = vec![TimedMidi {
        offset_ms: start_ms,
        msg: MidiMessage::Start,
        take: 0,
    }];
    events.extend((0..ticks).map(|n| TimedMidi {
        offset_ms: start_ms + (n as f64 * tick_ms).round() as u64,
        msg: MidiMessage::Clock,
        take: 0,
    }));
    events.push(TimedMidi {
        offset_ms: start_ms + length_ms,
        msg: MidiMessage::Stop,
        take: 0,
    });
    events
}

/// Converts milliseconds to a sample count without rounding drift.
pub fn ms_to_samples(ms: u64, sample_rate: u32) -> usize {
    (ms * u64::from(sample_rate) / 1000) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_plan_covers_four_octaves_every_third_semitone() {
        let plan = CapturePlan::default();
        assert_eq!(plan.notes.first(), Some(&36));
        assert_eq!(plan.notes.last(), Some(&84));
        assert_eq!(plan.notes.len(), 17);
        assert!(plan.notes.windows(2).all(|w| w[1] - w[0] == 3));
        assert_eq!(plan.velocities, vec![45, 90, 127]);
        assert_eq!(plan.take_count(), 51);
    }

    #[test]
    fn velocities_are_cleaned_up() {
        let plan = CapturePlan::every_nth_semitone(60, 60, 1, &[127, 0, 64, 64, 200]);
        assert_eq!(plan.velocities, vec![1, 64, 127]);
        let plan = CapturePlan::every_nth_semitone(60, 48, 0, &[]);
        assert_eq!(plan.velocities, vec![100]);
        assert_eq!(
            plan.notes.first(),
            Some(&48),
            "low and high are swapped back"
        );
        assert_eq!(plan.notes.len(), 13, "step 0 is treated as 1");
    }

    #[test]
    fn total_duration_adds_pre_roll_and_every_slot() {
        let mut plan = CapturePlan::every_nth_semitone(60, 66, 3, &[60, 120]);
        plan.hold_ms = 1000;
        plan.release_ms = 500;
        plan.gap_ms = 100;
        plan.pre_roll_ms = 400;
        // 3 notes x 2 velocities x 1.6 s + 0.4 s.
        assert_eq!(plan.total_duration(), Duration::from_millis(400 + 6 * 1600));
    }

    #[test]
    fn takes_go_note_by_note_and_events_pair_up() {
        let mut plan = CapturePlan::every_nth_semitone(60, 63, 3, &[50, 100]);
        plan.channel = 2;
        let takes = plan.takes();
        let order: Vec<(u8, u8)> = takes.iter().map(|t| (t.note, t.velocity)).collect();
        assert_eq!(order, vec![(60, 50), (60, 100), (63, 50), (63, 100)]);
        assert_eq!(
            takes[1].start_ms,
            u64::from(plan.pre_roll_ms) + plan.slot_ms()
        );

        let events = plan.midi_events();
        assert_eq!(events.len(), 8);
        assert!(events.windows(2).all(|w| w[0].offset_ms <= w[1].offset_ms));
        assert_eq!(
            events[0].msg,
            MidiMessage::NoteOn {
                channel: 2,
                note: 60,
                velocity: 50
            }
        );
        assert_eq!(
            events[1].msg,
            MidiMessage::NoteOff {
                channel: 2,
                note: 60,
                velocity: 0
            }
        );
        assert_eq!(
            events[1].offset_ms - events[0].offset_ms,
            u64::from(plan.hold_ms)
        );
    }

    #[test]
    fn ms_to_samples_is_exact() {
        assert_eq!(ms_to_samples(1000, 48_000), 48_000);
        assert_eq!(ms_to_samples(3_600_000, 44_100), 158_760_000);
    }

    #[test]
    fn clock_events_start_tick_and_stop() {
        // One beat at 120 BPM is 500 ms: Start, 24 ticks, Stop.
        let events = clock_events(120.0, 200, 500);
        assert_eq!(events.len(), 26);
        assert_eq!(events[0].msg, MidiMessage::Start);
        assert_eq!(events[1].offset_ms, 200);
        assert_eq!(events[24].offset_ms, 200 + 479);
        assert_eq!(events[25].msg, MidiMessage::Stop);
        assert_eq!(events[25].offset_ms, 700);
        assert!(events.windows(2).all(|w| w[0].offset_ms <= w[1].offset_ms));
    }
}
