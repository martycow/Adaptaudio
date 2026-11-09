//! MIDI Clock: Adaptaudio leads, the hardware follows its tempo.
//!
//! MIDI Clock is 24 ticks per quarter note ("PPQN"). [`ClockGenerator`] is the pure timing
//! math; [`MidiClockSender`] uses it to queue Start, Clock and Stop on a scheduler.

use crate::message::MidiMessage;
use crate::scheduler::{Schedule, ScheduleProducer};
use std::ops::Range;
use std::time::{Duration, Instant};

/// MIDI Clock ticks per quarter note.
pub const PPQN: u32 = 24;

const MIN_BPM: f64 = 1.0;
const MAX_BPM: f64 = 999.0;

/// When each clock tick happens, for a tempo and a start time.
///
/// Tick 0 is at the start. A tempo change takes effect from the next tick, so the tick count
/// stays continuous. Tick times are computed with the current tempo, so they are exact for
/// ticks after the last tempo change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClockGenerator {
    bpm: f64,
    anchor: Instant,
    anchor_tick: u64,
}

impl ClockGenerator {
    /// A clock at `bpm` (clamped to 1..=999) whose tick 0 is at `start`.
    pub fn new(bpm: f64, start: Instant) -> Self {
        ClockGenerator {
            bpm: clamp_bpm(bpm),
            anchor: start,
            anchor_tick: 0,
        }
    }

    pub fn bpm(&self) -> f64 {
        self.bpm
    }

    /// Time between two ticks.
    pub fn tick_interval(&self) -> Duration {
        Duration::from_secs_f64(self.seconds_per_tick())
    }

    fn seconds_per_tick(&self) -> f64 {
        60.0 / (self.bpm * f64::from(PPQN))
    }

    /// When tick `n` happens.
    pub fn tick_time(&self, n: u64) -> Instant {
        let spt = self.seconds_per_tick();
        if n >= self.anchor_tick {
            self.anchor + Duration::from_secs_f64((n - self.anchor_tick) as f64 * spt)
        } else {
            let back = Duration::from_secs_f64((self.anchor_tick - n) as f64 * spt);
            self.anchor.checked_sub(back).unwrap_or(self.anchor)
        }
    }

    /// The first tick that happens at or after `at`.
    pub fn first_tick_at_or_after(&self, at: Instant) -> u64 {
        let elapsed = if at >= self.anchor {
            at.duration_since(self.anchor).as_secs_f64()
        } else {
            -self.anchor.duration_since(at).as_secs_f64()
        };
        let estimate = self.anchor_tick as f64 + elapsed / self.seconds_per_tick();
        let mut n = estimate.ceil().max(0.0) as u64;
        // Float rounding can be off by one either way; settle it with exact tick times.
        while n > 0 && self.tick_time(n - 1) >= at {
            n -= 1;
        }
        while self.tick_time(n) < at {
            n += 1;
        }
        n
    }

    /// Ticks that happen in `from..to` (start included, end excluded).
    pub fn ticks_between(&self, from: Instant, to: Instant) -> Range<u64> {
        let first = self.first_tick_at_or_after(from);
        let end = self.first_tick_at_or_after(to).max(first);
        first..end
    }

    /// Changes the tempo from the first tick at or after `at`.
    pub fn set_bpm(&mut self, bpm: f64, at: Instant) {
        let tick = self.first_tick_at_or_after(at);
        self.anchor = self.tick_time(tick);
        self.anchor_tick = tick;
        self.bpm = clamp_bpm(bpm);
    }
}

fn clamp_bpm(bpm: f64) -> f64 {
    if bpm.is_finite() {
        bpm.clamp(MIN_BPM, MAX_BPM)
    } else {
        120.0
    }
}

/// How far ahead the clock sender queues ticks by default.
pub const DEFAULT_LOOKAHEAD: Duration = Duration::from_millis(50);

/// If [`MidiClockSender::pump`] was not called for this long, skip the missed ticks instead
/// of sending a burst of them.
const MAX_CATCH_UP: Duration = Duration::from_secs(1);

/// Sends MIDI Start, Clock and Stop through a scheduler so hardware (Hydrasynth arpeggiator,
/// EP-1320 sequencer...) follows Adaptaudio's tempo.
///
/// It has no thread of its own: call [`MidiClockSender::pump`] often (every UI frame or audio
/// block is plenty). Each call queues the ticks of the next [`DEFAULT_LOOKAHEAD`].
pub struct MidiClockSender<S: Schedule = ScheduleProducer> {
    out: S,
    clock: ClockGenerator,
    next_tick: u64,
    running: bool,
    lookahead: Duration,
}

impl<S: Schedule> MidiClockSender<S> {
    pub fn new(out: S, bpm: f64) -> Self {
        MidiClockSender {
            out,
            clock: ClockGenerator::new(bpm, Instant::now()),
            next_tick: 0,
            running: false,
            lookahead: DEFAULT_LOOKAHEAD,
        }
    }

    /// How far ahead ticks are queued. Longer survives UI hiccups; shorter makes tempo
    /// changes reach the hardware sooner.
    pub fn set_lookahead(&mut self, lookahead: Duration) {
        self.lookahead = lookahead;
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn bpm(&self) -> f64 {
        self.clock.bpm()
    }

    /// The clock math in use, for lining up other things with the ticks.
    pub fn clock(&self) -> &ClockGenerator {
        &self.clock
    }

    /// Sends Start at `at` and begins ticking from there (the first tick is the downbeat).
    pub fn start(&mut self, at: Instant) {
        self.clock = ClockGenerator::new(self.clock.bpm(), at);
        self.next_tick = 0;
        self.running = self.out.schedule_at(at, MidiMessage::Start);
    }

    /// Sends Continue at `at` and resumes ticking from there.
    pub fn resume(&mut self, at: Instant) {
        self.clock = ClockGenerator::new(self.clock.bpm(), at);
        self.next_tick = 0;
        self.running = self.out.schedule_at(at, MidiMessage::Continue);
    }

    /// Sends Stop at `at`. Ticks already queued before `at` still go out.
    pub fn stop(&mut self, at: Instant) {
        self.running = false;
        let _ = self.out.schedule_at(at, MidiMessage::Stop);
    }

    /// Changes the tempo. Ticks already queued keep the old tempo, so the change reaches the
    /// hardware within one lookahead.
    pub fn set_bpm(&mut self, bpm: f64) {
        let from = self.clock.tick_time(self.next_tick);
        self.clock.set_bpm(bpm, from);
    }

    /// Queues every tick due before `now + lookahead`. Returns how many were queued.
    pub fn pump(&mut self, now: Instant) -> usize {
        if !self.running {
            return 0;
        }
        if let Some(behind) = now.checked_sub(MAX_CATCH_UP)
            && self.clock.tick_time(self.next_tick) < behind
        {
            self.next_tick = self.clock.first_tick_at_or_after(now);
        }
        let horizon = now + self.lookahead;
        let mut queued = 0;
        loop {
            let at = self.clock.tick_time(self.next_tick);
            if at >= horizon || !self.out.schedule_at(at, MidiMessage::Clock) {
                break;
            }
            self.next_tick += 1;
            queued += 1;
        }
        queued
    }

    /// The scheduler handle back, for example to send notes on the same port.
    pub fn output(&mut self) -> &mut S {
        &mut self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn ticks_are_24_per_beat() {
        let start = Instant::now();
        let clock = ClockGenerator::new(120.0, start);
        // 120 bpm: a beat is 500 ms, a tick is 500/24 ms.
        assert!((clock.tick_interval().as_secs_f64() - 0.5 / 24.0).abs() < 1e-9);
        assert_eq!(clock.tick_time(0), start);
        let beat = clock.tick_time(24) - start;
        assert!((beat.as_secs_f64() - 0.5).abs() < 1e-6);
        let bar = clock.tick_time(96) - start;
        assert!((bar.as_secs_f64() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn ticks_between_includes_start_and_excludes_end() {
        let start = Instant::now();
        let clock = ClockGenerator::new(120.0, start);
        // One beat holds exactly ticks 0..24.
        assert_eq!(clock.ticks_between(start, clock.tick_time(24)), 0..24);
        assert_eq!(
            clock.ticks_between(clock.tick_time(5), clock.tick_time(7)),
            5..7
        );
        // Just after a tick, that tick is no longer included.
        assert_eq!(
            clock.ticks_between(
                clock.tick_time(5) + Duration::from_nanos(1),
                clock.tick_time(7)
            ),
            6..7
        );
        // Empty and reversed windows give nothing.
        assert!(
            clock
                .ticks_between(clock.tick_time(3), clock.tick_time(3))
                .is_empty()
        );
        assert!(
            clock
                .ticks_between(clock.tick_time(9), clock.tick_time(3))
                .is_empty()
        );
        // Before the start, the first tick is 0.
        if let Some(before) = start.checked_sub(ms(100)) {
            assert_eq!(clock.ticks_between(before, start + ms(1)), 0..1);
        }
    }

    #[test]
    fn tempo_change_keeps_counting_from_the_next_tick() {
        let start = Instant::now();
        let mut clock = ClockGenerator::new(120.0, start);
        let t10 = clock.tick_time(10);
        clock.set_bpm(60.0, t10 - Duration::from_micros(10));
        assert_eq!(clock.tick_time(10), t10, "the anchor tick does not move");
        let gap = clock.tick_time(11) - clock.tick_time(10);
        assert!(
            (gap.as_secs_f64() - 1.0 / 24.0).abs() < 1e-6,
            "60 bpm from here on"
        );
        assert_eq!(
            clock.ticks_between(t10, t10 + ms(1000) + Duration::from_micros(1)),
            10..35
        );
    }

    #[test]
    fn bpm_is_kept_in_a_sane_range() {
        let now = Instant::now();
        assert_eq!(ClockGenerator::new(0.0, now).bpm(), 1.0);
        assert_eq!(ClockGenerator::new(5000.0, now).bpm(), 999.0);
        assert_eq!(ClockGenerator::new(f64::NAN, now).bpm(), 120.0);
    }

    #[test]
    fn sender_queues_start_then_ticks_within_the_lookahead() {
        let start = Instant::now();
        let mut sender = MidiClockSender::new(Vec::new(), 120.0);
        sender.start(start);
        // 50 ms lookahead at 120 bpm (20.83 ms per tick): ticks 0, 1, 2.
        assert_eq!(sender.pump(start), 3);
        // Pumping again at the same moment queues nothing new.
        assert_eq!(sender.pump(start), 0);
        sender.pump(start + ms(500));
        sender.stop(start + ms(600));
        let events = sender.output().clone();
        assert_eq!(events[0], (start, MidiMessage::Start));
        assert_eq!(events[1], (start, MidiMessage::Clock));
        let clocks = events.iter().filter(|e| e.1 == MidiMessage::Clock).count();
        // Ticks before 550 ms: 0..=26.
        assert_eq!(clocks, 27);
        assert!(
            events.windows(2).all(|w| w[0].0 <= w[1].0),
            "events are in time order"
        );
        assert_eq!(events.last().map(|e| e.1), Some(MidiMessage::Stop));
        assert_eq!(sender.pump(start + ms(700)), 0, "stopped clocks stay quiet");
    }

    #[test]
    fn sender_skips_ticks_after_a_long_stall() {
        let start = Instant::now();
        let mut sender = MidiClockSender::new(Vec::new(), 120.0);
        sender.start(start);
        sender.pump(start);
        let queued = sender.pump(start + Duration::from_secs(10));
        assert!(queued <= 3, "no burst of hundreds of ticks, got {queued}");
    }

    #[test]
    fn sender_tempo_change_applies_after_queued_ticks() {
        let start = Instant::now();
        let mut sender = MidiClockSender::new(Vec::new(), 120.0);
        sender.start(start);
        sender.pump(start); // ticks 0..3 at 120 bpm
        sender.set_bpm(60.0);
        sender.pump(start + ms(200));
        let ticks: Vec<Instant> = sender
            .output()
            .iter()
            .filter(|e| e.1 == MidiMessage::Clock)
            .map(|e| e.0)
            .collect();
        let old_gap = (ticks[2] - ticks[1]).as_secs_f64();
        let new_gap = (ticks[4] - ticks[3]).as_secs_f64();
        assert!((old_gap - 0.5 / 24.0).abs() < 1e-6);
        assert!((new_gap - 1.0 / 24.0).abs() < 1e-6);
    }
}
