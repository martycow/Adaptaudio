//! Sending MIDI at exact moments from a dedicated thread.
//!
//! Any thread (the audio engine, the UI, a clock) gets a [`ScheduleProducer`] and pushes
//! `(when, message)` pairs into its own lock-free ring buffer. The scheduler thread collects
//! them, sorts them by time and sends each one when it is due: it sleeps in ~1 ms steps and
//! spins for the last millisecond. After start nothing on this path allocates.

use crate::message::MidiMessage;
use crate::ports::{MidiError, MidiOut, MidiSink, all_notes_off_messages};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering as AtomicOrdering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How many events one producer can queue, and how many the thread keeps waiting at once.
pub const DEFAULT_CAPACITY: usize = 4096;

/// Below this much waiting time the thread spins instead of sleeping.
const SPIN_WINDOW: Duration = Duration::from_micros(1500);
const SLEEP_STEP: Duration = Duration::from_millis(1);

type Event = (Instant, MidiMessage);

/// The queue was full; the message was not scheduled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduleFull(pub MidiMessage);

/// A handle for queueing messages. It is `Send`, so move it to the thread that needs it;
/// call [`MidiScheduler::producer`] again for another thread. Pushing never blocks or allocates,
/// so it is safe on the audio thread.
pub struct ScheduleProducer {
    ring: rtrb::Producer<Event>,
}

impl ScheduleProducer {
    /// Queues `msg` to be sent at `at`. A time in the past means "as soon as possible".
    pub fn schedule(&mut self, at: Instant, msg: MidiMessage) -> Result<(), ScheduleFull> {
        self.ring.push((at, msg)).map_err(|_| ScheduleFull(msg))
    }

    /// Queues `msg` to be sent right away.
    pub fn send_now(&mut self, msg: MidiMessage) -> Result<(), ScheduleFull> {
        self.schedule(Instant::now(), msg)
    }

    /// How many more messages fit before the scheduler picks some up.
    pub fn free_slots(&self) -> usize {
        self.ring.slots()
    }
}

/// Anything messages can be queued on with a time stamp. [`ScheduleProducer`] is the real one;
/// a `Vec` collects them, which is handy in tests.
pub trait Schedule {
    /// Returns false when the message could not be queued.
    fn schedule_at(&mut self, at: Instant, msg: MidiMessage) -> bool;
}

impl Schedule for ScheduleProducer {
    fn schedule_at(&mut self, at: Instant, msg: MidiMessage) -> bool {
        self.schedule(at, msg).is_ok()
    }
}

impl Schedule for Vec<(Instant, MidiMessage)> {
    fn schedule_at(&mut self, at: Instant, msg: MidiMessage) -> bool {
        self.push((at, msg));
        true
    }
}

/// An event waiting in the thread's heap. Ordered so the earliest (then first queued) is on top.
struct Pending {
    at: Instant,
    seq: u64,
    msg: MidiMessage,
}

impl PartialEq for Pending {
    fn eq(&self, other: &Self) -> bool {
        self.at == other.at && self.seq == other.seq
    }
}

impl Eq for Pending {}

impl PartialOrd for Pending {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Pending {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap: reverse so the earliest event comes out first.
        (other.at, other.seq).cmp(&(self.at, self.seq))
    }
}

struct Shared {
    stop: AtomicBool,
    panic: AtomicBool,
    send_errors: AtomicU32,
}

/// Owns a MIDI output and the thread that sends queued messages on time.
///
/// Dropping it (or calling [`MidiScheduler::stop`]) sends All Notes Off and ends the thread.
pub struct MidiScheduler {
    shared: Arc<Shared>,
    register: mpsc::Sender<rtrb::Consumer<Event>>,
    thread: Option<JoinHandle<()>>,
    capacity: usize,
}

impl MidiScheduler {
    /// Starts the sending thread for a real MIDI output.
    pub fn start(out: MidiOut) -> Result<MidiScheduler, MidiError> {
        MidiScheduler::with_sink(out, DEFAULT_CAPACITY)
    }

    /// Starts the sending thread for any [`MidiSink`]. `capacity` is the queue size of each
    /// producer and the number of events the thread holds at once.
    pub fn with_sink(sink: impl MidiSink, capacity: usize) -> Result<MidiScheduler, MidiError> {
        let capacity = capacity.max(16);
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            panic: AtomicBool::new(false),
            send_errors: AtomicU32::new(0),
        });
        let (register, registrations) = mpsc::channel();
        let thread_shared = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("aa-midi-scheduler".into())
            .spawn(move || run(sink, registrations, thread_shared, capacity))
            .map_err(|e| MidiError::Thread(e.to_string()))?;
        Ok(MidiScheduler {
            shared,
            register,
            thread: Some(thread),
            capacity,
        })
    }

    /// A new handle for queueing messages from another thread. Creating one allocates its ring
    /// buffer, so make producers up front, not on the audio thread.
    pub fn producer(&self) -> ScheduleProducer {
        let (ring, consumer) = rtrb::RingBuffer::new(self.capacity);
        // If the thread is gone the producer still works; its messages are just never sent.
        let _ = self.register.send(consumer);
        ScheduleProducer { ring }
    }

    /// Drops everything still queued and sends All Notes Off on every channel. The scheduler
    /// keeps running. Use it when the transport stops.
    pub fn all_notes_off(&self) {
        self.shared.panic.store(true, AtomicOrdering::Release);
    }

    /// How many messages the output refused so far (for example, after the cable was pulled).
    pub fn send_errors(&self) -> u32 {
        self.shared.send_errors.load(AtomicOrdering::Relaxed)
    }

    /// Sends All Notes Off and stops the thread.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.shared.stop.store(true, AtomicOrdering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for MidiScheduler {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn send_panic(sink: &mut impl MidiSink, shared: &Shared) {
    for msg in all_notes_off_messages() {
        if sink.send(&msg).is_err() {
            shared.send_errors.fetch_add(1, AtomicOrdering::Relaxed);
        }
    }
}

fn run(
    mut sink: impl MidiSink,
    registrations: mpsc::Receiver<rtrb::Consumer<Event>>,
    shared: Arc<Shared>,
    capacity: usize,
) {
    let mut consumers: Vec<rtrb::Consumer<Event>> = Vec::with_capacity(16);
    let mut pending: BinaryHeap<Pending> = BinaryHeap::with_capacity(capacity);
    let mut seq: u64 = 0;

    while !shared.stop.load(AtomicOrdering::Acquire) {
        while let Ok(consumer) = registrations.try_recv() {
            consumers.push(consumer);
        }
        consumers.retain(|c| !(c.is_abandoned() && c.is_empty()));

        if shared.panic.swap(false, AtomicOrdering::AcqRel) {
            pending.clear();
            for consumer in &mut consumers {
                while consumer.pop().is_ok() {}
            }
            send_panic(&mut sink, &shared);
        }

        for consumer in &mut consumers {
            while pending.len() < capacity {
                match consumer.pop() {
                    Ok((at, msg)) => {
                        pending.push(Pending { at, seq, msg });
                        seq += 1;
                    }
                    Err(_) => break,
                }
            }
        }

        let now = Instant::now();
        while pending.peek().is_some_and(|p| p.at <= now) {
            if let Some(event) = pending.pop()
                && sink.send(&event.msg).is_err()
            {
                shared.send_errors.fetch_add(1, AtomicOrdering::Relaxed);
            }
        }

        match pending.peek().map(|p| p.at) {
            Some(due) if due.saturating_duration_since(now) <= SPIN_WINDOW => {
                while Instant::now() < due {
                    std::hint::spin_loop();
                }
            }
            _ => std::thread::sleep(SLEEP_STEP),
        }
    }

    send_panic(&mut sink, &shared);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<(Instant, MidiMessage)>>>);

    impl MidiSink for Recorder {
        fn send(&mut self, msg: &MidiMessage) -> Result<(), MidiError> {
            self.0.lock().unwrap().push((Instant::now(), *msg));
            Ok(())
        }
    }

    fn note(n: u8) -> MidiMessage {
        MidiMessage::NoteOn {
            channel: 1,
            note: n,
            velocity: 100,
        }
    }

    #[test]
    fn messages_go_out_in_time_order_and_on_time() {
        let recorder = Recorder::default();
        let scheduler = MidiScheduler::with_sink(recorder.clone(), 64).unwrap();
        let mut producer = scheduler.producer();
        let base = Instant::now() + Duration::from_millis(20);
        producer
            .schedule(base + Duration::from_millis(20), note(3))
            .unwrap();
        producer.schedule(base, note(1)).unwrap();
        producer
            .schedule(base + Duration::from_millis(10), note(2))
            .unwrap();
        // Same instant: keep the order they were queued in.
        producer
            .schedule(
                base + Duration::from_millis(30),
                MidiMessage::NoteOff {
                    channel: 1,
                    note: 4,
                    velocity: 0,
                },
            )
            .unwrap();
        producer
            .schedule(base + Duration::from_millis(30), note(4))
            .unwrap();
        std::thread::sleep(Duration::from_millis(90));
        scheduler.stop();

        let sent = recorder.0.lock().unwrap().clone();
        let expected = [
            (note(1), 0),
            (note(2), 10),
            (note(3), 20),
            (
                MidiMessage::NoteOff {
                    channel: 1,
                    note: 4,
                    velocity: 0,
                },
                30,
            ),
            (note(4), 30),
        ];
        for (i, (msg, offset_ms)) in expected.iter().enumerate() {
            let (when, got) = sent[i];
            assert_eq!(got, *msg);
            let due = base + Duration::from_millis(*offset_ms);
            assert!(when >= due, "message {i} was early");
            // Generous bound: test machines can be busy.
            assert!(
                when - due < Duration::from_millis(15),
                "message {i} was {:?} late",
                when - due
            );
        }
        // Stopping silences everything.
        assert_eq!(sent.len(), expected.len() + 32);
    }

    #[test]
    fn producers_work_from_other_threads() {
        let recorder = Recorder::default();
        let scheduler = MidiScheduler::with_sink(recorder.clone(), 64).unwrap();
        let mut producer = scheduler.producer();
        let handle = std::thread::spawn(move || {
            producer.send_now(note(60)).unwrap();
        });
        handle.join().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(
            recorder.0.lock().unwrap().first().map(|e| e.1),
            Some(note(60))
        );
    }

    #[test]
    fn panic_drops_queued_messages() {
        let recorder = Recorder::default();
        let scheduler = MidiScheduler::with_sink(recorder.clone(), 64).unwrap();
        let mut producer = scheduler.producer();
        producer
            .schedule(Instant::now() + Duration::from_millis(50), note(1))
            .unwrap();
        std::thread::sleep(Duration::from_millis(10));
        scheduler.all_notes_off();
        std::thread::sleep(Duration::from_millis(70));
        let sent = recorder.0.lock().unwrap().clone();
        assert_eq!(sent.len(), 32, "only the panic messages");
        assert!(
            sent.iter()
                .all(|(_, m)| matches!(m, MidiMessage::ControlChange { .. }))
        );
    }

    #[test]
    fn full_queue_reports_the_message_back() {
        let recorder = Recorder::default();
        let scheduler = MidiScheduler::with_sink(recorder, 16).unwrap();
        let mut producer = scheduler.producer();
        let later = Instant::now() + Duration::from_secs(60);
        let mut refused = None;
        // The thread drains the ring into its heap (also 16), so 40 pushes must overflow.
        for i in 0..40 {
            if let Err(full) = producer.schedule(later, note(i)) {
                refused = Some(full);
                break;
            }
        }
        assert!(matches!(
            refused,
            Some(ScheduleFull(MidiMessage::NoteOn { .. }))
        ));
    }
}
