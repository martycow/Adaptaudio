//! Animation values fed from the audio engine at 60 Hz without rebuilding the view.
//!
//! A background thread copies the engine's telemetry into Rune `Live` values (meters, playheads,
//! the beat pulse, the XY orb). Rune redraws only what is bound to them. When a discrete thing
//! changes (mode, section, fired events) the thread wakes the UI so `poll` rebuilds the view.

use aa_engine::{Controller, MAX_TRACKS, Telemetry};
use rune::{Live, LiveText, Waker};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// Side of the XY pad in the side panel and on the Simulator screen, in points.
pub const PAD_SMALL: f32 = 252.0;
pub const PAD_BIG: f32 = 420.0;
pub const MAX_PARAMS: usize = aa_engine::telemetry::MAX_PARAMS;

#[derive(Clone)]
pub struct LiveState {
    telemetry: Arc<RwLock<Arc<Telemetry>>>,
    waker: Arc<Mutex<Option<Waker>>>,
    pub running: Arc<AtomicBool>,
    /// Which parameters drive the pad's X and Y.
    pub x_param: Arc<AtomicUsize>,
    pub y_param: Arc<AtomicUsize>,
    /// Clip lengths in steps per track (for playheads), set by the UI on every project change.
    pub clip_len: Arc<Mutex<[f32; MAX_TRACKS]>>,
    pub beat: Live<f32>,
    /// 1 at rest, swelling on each beat.
    pub beat_scale: Live<f32>,
    pub bar: Live<f32>,
    pub glow: Live<f32>,
    pub master: [Live<[f32; 2]>; 2],
    pub track_level: Vec<Live<[f32; 2]>>,
    pub track_hit: Vec<Live<f32>>,
    pub track_progress: Vec<Live<[f32; 2]>>,
    pub track_playhead: Vec<Live<[f32; 2]>>,
    pub params: Vec<Live<[f32; 2]>>,
    pub targets: Vec<Live<[f32; 2]>>,
    pub orb_small: Live<[f32; 2]>,
    pub orb_big: Live<[f32; 2]>,
    pub target_small: Live<[f32; 2]>,
    pub target_big: Live<[f32; 2]>,
    pub sidechain: Live<[f32; 2]>,
    pub deck_gain: [Live<[f32; 2]>; 2],
    pub position: LiveText,
    pub load: LiveText,
    pub scenario: Live<[f32; 2]>,
}

fn trims(n: usize) -> Vec<Live<[f32; 2]>> {
    (0..n).map(|_| Live::new([0.0, 0.0])).collect()
}

impl LiveState {
    pub fn new(controller: &Controller) -> Self {
        LiveState {
            telemetry: Arc::new(RwLock::new(Arc::clone(&controller.telemetry))),
            waker: Arc::new(Mutex::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            x_param: Arc::new(AtomicUsize::new(0)),
            y_param: Arc::new(AtomicUsize::new(1)),
            clip_len: Arc::new(Mutex::new([16.0; MAX_TRACKS])),
            beat: Live::new(0.0),
            beat_scale: Live::new(1.0),
            bar: Live::new(0.0),
            glow: Live::new(0.0),
            master: [Live::new([0.0, 0.0]), Live::new([0.0, 0.0])],
            track_level: trims(MAX_TRACKS),
            track_hit: (0..MAX_TRACKS).map(|_| Live::new(0.0)).collect(),
            track_progress: trims(MAX_TRACKS),
            track_playhead: trims(MAX_TRACKS),
            params: trims(MAX_PARAMS),
            targets: trims(MAX_PARAMS),
            orb_small: Live::new([0.0, 0.0]),
            orb_big: Live::new([0.0, 0.0]),
            target_small: Live::new([0.0, 0.0]),
            target_big: Live::new([0.0, 0.0]),
            sidechain: Live::new([0.0, 0.0]),
            deck_gain: [Live::new([0.0, 0.0]), Live::new([0.0, 0.0])],
            position: LiveText::new("1.1.1"),
            load: LiveText::new("0%"),
            scenario: Live::new([0.0, 0.0]),
        }
    }

    /// Points the publisher at a new engine (after the audio device changed).
    pub fn rebind(&self, controller: &Controller) {
        if let Ok(mut t) = self.telemetry.write() {
            *t = Arc::clone(&controller.telemetry);
        }
    }

    pub fn set_waker(&self, waker: Waker) {
        if let Ok(mut w) = self.waker.lock() {
            *w = Some(waker);
        }
    }

    /// Shows a target the UI just set before the engine reports it.
    pub fn set_param_target(&self, index: usize, value: f32) {
        if let Some(l) = self.targets.get(index) {
            l.set([0.0, value]);
        }
    }

    /// Starts the 60 Hz publisher thread (once).
    pub fn start(&self) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        std::thread::Builder::new()
            .name("adaptaudio-live".into())
            .spawn(move || {
                let mut last_fingerprint = 0u64;
                while me.running.load(Ordering::Relaxed) {
                    let fp = me.tick();
                    if fp != last_fingerprint {
                        last_fingerprint = fp;
                        if let Ok(w) = me.waker.lock()
                            && let Some(w) = w.as_ref()
                        {
                            w();
                        }
                    }
                    std::thread::sleep(Duration::from_millis(16));
                }
            })
            .ok();
    }

    /// One publish: copies telemetry into the live values. Returns a fingerprint of the discrete
    /// state so the caller can wake the UI when it changes.
    pub fn tick(&self) -> u64 {
        let Ok(t) = self.telemetry.read().map(|t| Arc::clone(&t)) else {
            return 0;
        };
        let beat = t.beat_pulse.get();
        self.beat.set(beat);
        self.beat_scale.set(1.0 + 0.45 * beat);
        self.bar.set(t.bar_pulse.get());
        let xi = self.x_param.load(Ordering::Relaxed).min(MAX_PARAMS - 1);
        let yi = self.y_param.load(Ordering::Relaxed).min(MAX_PARAMS - 1);
        let intensity = t.params[xi].get();
        self.glow.set(if t.is_playing() {
            (0.25 + 0.75 * intensity) * beat
        } else {
            0.0
        });
        for (i, m) in self.master.iter().enumerate() {
            m.set([0.0, t.master_peak[i].get().clamp(0.0, 1.0)]);
        }
        let lens = self
            .clip_len
            .lock()
            .map(|l| *l)
            .unwrap_or([16.0; MAX_TRACKS]);
        for (i, &len) in lens.iter().enumerate() {
            let tt = &t.tracks[i];
            self.track_level[i].set([0.0, (tt.level.get() * 1.4).clamp(0.0, 1.0)]);
            self.track_hit[i].set(tt.hit.get().clamp(0.0, 1.0));
            let step = tt.clip_step.get();
            let len = len.max(1.0);
            if step >= 0.0 {
                let f = (step / len).clamp(0.0, 1.0);
                self.track_progress[i].set([0.0, f]);
                let s = step.floor() / len;
                self.track_playhead[i].set([s, (s + 1.0 / len).min(1.0)]);
            } else {
                self.track_progress[i].set([0.0, 0.0]);
                self.track_playhead[i].set([0.0, 0.0]);
            }
        }
        for i in 0..MAX_PARAMS {
            self.params[i].set([0.0, t.params[i].get()]);
            self.targets[i].set([0.0, t.param_targets[i].get()]);
        }
        let (x, y) = (intensity, t.params[yi].get());
        let (tx, ty) = (t.param_targets[xi].get(), t.param_targets[yi].get());
        self.orb_small.set([x * PAD_SMALL, (1.0 - y) * PAD_SMALL]);
        self.orb_big.set([x * PAD_BIG, (1.0 - y) * PAD_BIG]);
        self.target_small
            .set([tx * PAD_SMALL, (1.0 - ty) * PAD_SMALL]);
        self.target_big.set([tx * PAD_BIG, (1.0 - ty) * PAD_BIG]);
        self.sidechain.set([0.0, t.sidechain.get().clamp(0.0, 1.0)]);
        for d in 0..2 {
            self.deck_gain[d].set([0.0, t.deck_gain[d].get().clamp(0.0, 1.0)]);
        }
        let pos = t.position.get();
        let bar = (pos / 16.0).floor() as i64 + 1;
        let beat_in_bar = ((pos % 16.0) / 4.0).floor() as i64 + 1;
        let step = (pos % 4.0).floor() as i64 + 1;
        self.position.set(format!("{bar:>3}.{beat_in_bar}.{step}"));
        self.load
            .set(format!("{:.0}%", (t.load.get() * 100.0).clamp(0.0, 999.0)));

        // Discrete fingerprint.
        let mut fp = t.mode.load(Ordering::Relaxed) as i64 as u64;
        fp = fp
            .wrapping_mul(31)
            .wrapping_add(t.pending_mode.load(Ordering::Relaxed) as i64 as u64);
        fp = fp.wrapping_mul(31).wrapping_add(u64::from(t.is_playing()));
        fp = fp
            .wrapping_mul(31)
            .wrapping_add(t.fired.load(Ordering::Relaxed));
        let active = t.active_deck.load(Ordering::Relaxed) as usize;
        fp = fp.wrapping_mul(31).wrapping_add(u64::from(
            t.deck_section[active.min(1)].load(Ordering::Relaxed),
        ));
        for tt in t.tracks.iter() {
            fp = fp
                .wrapping_mul(31)
                .wrapping_add(u64::from(tt.clip.load(Ordering::Relaxed)));
        }
        fp
    }
}
