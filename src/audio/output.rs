//! Where the samples go: the system's sound device, or nowhere at the pace of a real one.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::Consumer;

/// Which output a player writes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioOut {
    /// The system's default sound device.
    Device,
    /// No device: the samples are taken at the pace a device would take them and thrown away, so
    /// a test runs the whole player without a sound on the machine.
    Null,
    /// Like [`Null`](Self::Null), until the device goes away after this long of a track, the way
    /// headphones are pulled out.
    #[cfg(test)]
    Vanishing(Duration),
}

/// What the output and the player share: how far the sound has got, whether it is held and how
/// loud it is.
#[derive(Debug)]
pub(crate) struct Meter {
    /// Stereo frames that have reached the output since the track began.
    pub(crate) played: AtomicU64,
    /// Whether the output writes silence instead of taking samples.
    pub(crate) paused: AtomicBool,
    /// Why the output stopped for good, once the device has gone away.
    lost: Mutex<Option<String>>,
    /// The last of the sound taken, for the visualizer.
    pub(crate) scope: Scope,
    /// What every sample is multiplied by on its way out, as the bits of an `f32`.
    gain: AtomicU32,
    /// Samples in the ring from before a seek, to be thrown away unheard.
    pub(crate) stale: AtomicUsize,
}

impl Default for Meter {
    fn default() -> Self {
        Self {
            played: AtomicU64::new(0),
            paused: AtomicBool::new(false),
            lost: Mutex::new(None),
            scope: Scope::default(),
            gain: AtomicU32::new(1.0_f32.to_bits()),
            stale: AtomicUsize::new(0),
        }
    }
}

impl Meter {
    /// Sets what every sample is multiplied by on its way out.
    pub(crate) fn set_gain(&self, gain: f32) {
        self.gain.store(gain.to_bits(), Ordering::Relaxed);
    }

    /// What every sample is multiplied by on its way out.
    pub(crate) fn gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Ordering::Relaxed))
    }

    /// Records that the device has gone away, and why.
    pub(crate) fn lose(&self, reason: String) {
        *self.lost.lock().unwrap_or_else(PoisonError::into_inner) = Some(reason);
    }

    /// Why the device went away, once, when it has.
    pub(crate) fn take_lost(&self) -> Option<String> {
        self.lost.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

/// How many of the last frames taken the output keeps for the visualizer: enough for one window of
/// it and a frame's worth of sound beside it.
pub const SCOPE: usize = 4096;

/// The last [`SCOPE`] frames the output took, folded to one channel, which the visualizer reads
/// while the output keeps writing. The output never waits for a reader: each sample is a slot of
/// its own, so a reader that comes mid-write sees a few samples of the next moment, which a
/// picture of the sound cannot show.
#[derive(Debug)]
pub(crate) struct Scope {
    /// Each sample's bits, so a slot is written and read without a lock.
    slots: Box<[AtomicU32]>,
    /// How many frames have been written since the start; the next one goes to this slot.
    written: AtomicUsize,
}

impl Default for Scope {
    fn default() -> Self {
        Self { slots: (0..SCOPE).map(|_| AtomicU32::new(0)).collect(), written: AtomicUsize::new(0) }
    }
}

impl Scope {
    /// Keeps the frames of interleaved stereo `samples`, each as the mean of its two sides.
    pub(crate) fn push(&self, samples: &[f32]) {
        let mut at = self.written.load(Ordering::Relaxed);
        for frame in samples.chunks_exact(2) {
            let mono = f32::midpoint(frame[0], frame[1]);
            self.slots[at % SCOPE].store(mono.to_bits(), Ordering::Relaxed);
            at = at.wrapping_add(1);
        }
        self.written.store(at, Ordering::Release);
    }

    /// Fills `out` with the last `out.len()` frames, the oldest first; at most [`SCOPE`] are kept,
    /// and frames not written yet are silence.
    pub(crate) fn latest(&self, out: &mut [f32]) {
        let written = self.written.load(Ordering::Acquire);
        let count = out.len().min(SCOPE);
        let start = out.len() - count;
        out[..start].fill(0.0);
        for (index, sample) in out[start..].iter_mut().enumerate() {
            // Slot of the frame `count - index` back from the newest.
            let back = count - index;
            *sample = if back > written {
                0.0
            } else {
                f32::from_bits(self.slots[(written - back) % SCOPE].load(Ordering::Relaxed))
            };
        }
    }
}

/// An open output, taking samples from the ring while it lives.
#[expect(dead_code, reason = "each output is held only for its drop, which closes it")]
pub(crate) enum Output {
    /// The sound device's stream; dropping it closes the device.
    Device(cpal::Stream),
    /// The thread that takes samples without playing them.
    Null(NullOutput),
}

impl Output {
    /// Opens `kind` at `rate` frames a second, taking interleaved stereo from `ring`.
    pub(crate) fn open(kind: AudioOut, rate: u32, ring: Consumer<f32>, meter: Arc<Meter>) -> Result<Self, String> {
        match kind {
            AudioOut::Device => device(rate, ring, meter).map(Self::Device),
            AudioOut::Null => Ok(Self::Null(NullOutput::start(rate, ring, meter, None))),
            #[cfg(test)]
            AudioOut::Vanishing(after) => Ok(Self::Null(NullOutput::start(rate, ring, meter, Some(after)))),
        }
    }
}

/// Opens the default device for two channels of `f32` at `rate`.
fn device(rate: u32, mut ring: Consumer<f32>, meter: Arc<Meter>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or_else(|| "no sound device".to_owned())?;
    let config = cpal::StreamConfig { channels: 2, sample_rate: rate, buffer_size: cpal::BufferSize::Default };
    let told = Arc::clone(&meter);
    let mut fade = Fade::new(rate, &meter);
    let stream = device
        .build_output_stream(
            config,
            move |out: &mut [f32], _: &cpal::OutputCallbackInfo| fill(out, &mut ring, &meter, &mut fade),
            // A late buffer or a route that followed the default device is heard, not shown; a
            // device that is gone stops the track, and the screen says so.
            move |error: cpal::Error| {
                if gone(error.kind()) {
                    told.lose(error.to_string());
                }
            },
            None,
        )
        .map_err(|error| error.to_string())?;
    stream.play().map_err(|error| error.to_string())?;
    Ok(stream)
}

/// Whether a stream error means the sound can no longer go out through this stream.
fn gone(kind: cpal::ErrorKind) -> bool {
    matches!(
        kind,
        cpal::ErrorKind::DeviceNotAvailable | cpal::ErrorKind::HostUnavailable | cpal::ErrorKind::StreamInvalidated
    )
}

/// How long the sound takes to fade out when it is held and to fade in when it goes on: long
/// enough that the cut is not heard as a click, short enough that it is not heard as a fade.
const RAMP: Duration = Duration::from_millis(5);

/// How loud the output lets the sound through, between silent (held) and whole (heard), moving
/// across [`RAMP`] whenever the sound is held or goes on.
pub(crate) struct Fade {
    /// Where the level stands, in frames from silent.
    at: u32,
    /// How many frames the fade takes.
    frames: u32,
}

impl Fade {
    /// A fade for an output of `rate` frames a second, standing where `meter` says: silent when
    /// the sound is held, whole when it is heard.
    pub(crate) fn new(rate: u32, meter: &Meter) -> Self {
        let frames = ((RAMP.as_secs_f64() * f64::from(rate)) as u32).max(1);
        let at = if meter.paused.load(Ordering::Relaxed) { 0 } else { frames };
        Self { at, frames }
    }

    /// Moves one frame towards silence (`down`) or towards whole, and gives the level it stands at.
    fn step(&mut self, down: bool) -> f32 {
        self.at = if down { self.at.saturating_sub(1) } else { (self.at + 1).min(self.frames) };
        self.at as f32 / self.frames as f32
    }
}

/// Fills `out` from `ring`, with silence where the ring has run dry or the sound is held. A sound
/// that is held fades out over [`RAMP`] before the silence, and fades back in when it goes on.
fn fill(out: &mut [f32], ring: &mut Consumer<f32>, meter: &Meter, fade: &mut Fade) {
    let paused = meter.paused.load(Ordering::Relaxed);
    if paused && fade.at == 0 {
        out.fill(0.0);
        return;
    }
    let stale = meter.stale.swap(0, Ordering::Relaxed).min(ring.slots()) & !1;
    if let Ok(chunk) = ring.read_chunk(stale) {
        chunk.commit_all();
    }
    // Whole frames only, so the left and right never swap; a held sound takes only what it fades
    // out over.
    let mut wanted = out.len();
    if paused {
        wanted = wanted.min(fade.at as usize * 2);
    }
    let taken = ring.slots().min(wanted) & !1;
    if let Ok(chunk) = ring.read_chunk(taken) {
        let (first, second) = chunk.as_slices();
        out[..first.len()].copy_from_slice(first);
        out[first.len()..taken].copy_from_slice(second);
        chunk.commit_all();
        // The visualizer sees the sound before the volume, so a quiet track still moves it.
        meter.scope.push(&out[..taken]);
        let gain = meter.gain();
        for frame in out[..taken].chunks_exact_mut(2) {
            let scale = gain * fade.step(paused);
            frame.iter_mut().for_each(|sample| *sample *= scale);
        }
    }
    out[taken..].fill(0.0);
    meter.played.fetch_add(taken as u64 / 2, Ordering::Relaxed);
}

/// The output of [`AudioOut::Null`]: a thread taking a hundredth of a second of samples every
/// hundredth of a second.
pub(crate) struct NullOutput {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// How often the null output takes its share.
const NULL_STEP: Duration = Duration::from_millis(10);

impl NullOutput {
    /// Takes samples at `rate` until dropped or, when `vanish` is given, until that long has passed.
    fn start(rate: u32, mut ring: Consumer<f32>, meter: Arc<Meter>, vanish: Option<Duration>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let step = (rate as usize / 100).max(1) * 2;
        let thread = std::thread::spawn(move || {
            let mut out = vec![0.0; step];
            let mut fade = Fade::new(rate, &meter);
            let started = std::time::Instant::now();
            while !stopped.load(Ordering::Relaxed) {
                if vanish.is_some_and(|after| started.elapsed() >= after) {
                    meter.lose("the device was disconnected".to_owned());
                    return;
                }
                fill(&mut out, &mut ring, &meter, &mut fade);
                std::thread::sleep(NULL_STEP);
            }
        });
        Self { stop, thread: Some(thread) }
    }
}

impl Drop for NullOutput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fade that moves in one frame, for the tests of everything but the fade.
    fn instant(meter: &Meter) -> Fade {
        Fade::new(100, meter)
    }

    /// An output's fade at 48 kHz: 240 frames each way.
    fn real(meter: &Meter) -> Fade {
        Fade::new(48_000, meter)
    }

    #[test]
    fn a_held_sound_fades_out_over_five_milliseconds_and_comes_back_in_over_as_many() {
        let (mut producer, mut consumer) = rtrb::RingBuffer::new(4096);
        for _ in 0..2000 {
            producer.push(0.5).expect("room");
        }
        let meter = Meter::default();
        let mut fade = real(&meter);
        let mut out = [0.0; 200];
        fill(&mut out, &mut consumer, &meter, &mut fade);
        assert!(out.iter().all(|sample| *sample == 0.5), "heard whole");
        meter.paused.store(true, Ordering::Relaxed);
        let mut out = [1.0; 1000];
        fill(&mut out, &mut consumer, &meter, &mut fade);
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        assert!(left[0] > 0.49 && left[0] < 0.5, "the fade starts where the sound was: {}", left[0]);
        assert!(left.windows(2).take(239).all(|pair| pair[1] < pair[0]), "it only falls");
        assert!(left[239..].iter().all(|sample| *sample == 0.0), "silent after 240 frames");
        assert_eq!(meter.played.load(Ordering::Relaxed), 100 + 240, "only the fade is taken from the ring");
        meter.paused.store(false, Ordering::Relaxed);
        let mut out = [0.0; 1000];
        fill(&mut out, &mut consumer, &meter, &mut fade);
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        assert!(left[0] > 0.0 && left[0] < 0.01, "back from silence: {}", left[0]);
        assert!(left.windows(2).take(239).all(|pair| pair[1] > pair[0]), "it only rises");
        assert!(left[240..].iter().all(|sample| *sample == 0.5), "whole again after 240 frames");
    }

    #[test]
    fn a_held_output_writes_silence_and_takes_nothing() {
        let (mut producer, mut consumer) = rtrb::RingBuffer::new(8);
        for sample in [0.5, 0.5, 0.5, 0.5] {
            producer.push(sample).expect("room");
        }
        let meter = Meter::default();
        meter.paused.store(true, Ordering::Relaxed);
        let mut out = [1.0; 4];
        let mut fade = instant(&meter);
        fill(&mut out, &mut consumer, &meter, &mut fade);
        assert_eq!(out, [0.0; 4]);
        assert_eq!(consumer.slots(), 4, "the samples wait for the sound to go on");
        meter.paused.store(false, Ordering::Relaxed);
        fill(&mut out, &mut consumer, &meter, &mut fade);
        assert_eq!(out, [0.5; 4]);
        assert_eq!(meter.played.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn the_scope_keeps_the_last_frames_taken_as_one_channel_oldest_first() {
        let scope = Scope::default();
        let mut out = [9.0; 3];
        scope.latest(&mut out);
        assert_eq!(out, [0.0; 3], "nothing taken yet is silence");
        scope.push(&[0.2, 0.4, 1.0, 0.0]);
        scope.latest(&mut out);
        assert_eq!(out, [0.0, 0.3, 0.5]);
        let many: Vec<f32> = (0..SCOPE + 2).flat_map(|frame| [frame as f32, frame as f32]).collect();
        scope.push(&many);
        scope.latest(&mut out);
        assert_eq!(out, [(SCOPE - 1) as f32, SCOPE as f32, (SCOPE + 1) as f32], "the oldest give way");
    }

    #[test]
    fn what_the_output_takes_reaches_the_scope_and_a_held_output_adds_nothing() {
        let (mut producer, mut consumer) = rtrb::RingBuffer::new(8);
        for sample in [0.5, 0.1, 0.5, 0.1] {
            producer.push(sample).expect("room");
        }
        let meter = Meter::default();
        meter.paused.store(true, Ordering::Relaxed);
        let mut out = [0.0; 4];
        let mut fade = instant(&meter);
        fill(&mut out, &mut consumer, &meter, &mut fade);
        let mut seen = [9.0; 2];
        meter.scope.latest(&mut seen);
        assert_eq!(seen, [0.0; 2], "held, nothing goes out");
        meter.paused.store(false, Ordering::Relaxed);
        fill(&mut out, &mut consumer, &meter, &mut fade);
        meter.scope.latest(&mut seen);
        assert!(seen.iter().all(|sample| (sample - 0.3).abs() < 1e-6), "{seen:?}");
    }

    #[test]
    fn the_volume_reaches_the_sound_but_not_the_visualizer() {
        let (mut producer, mut consumer) = rtrb::RingBuffer::new(8);
        for sample in [0.5, 0.5] {
            producer.push(sample).expect("room");
        }
        let meter = Meter::default();
        meter.set_gain(0.25);
        let mut out = [0.0; 2];
        let mut fade = instant(&meter);
        fill(&mut out, &mut consumer, &meter, &mut fade);
        assert_eq!(out, [0.125; 2]);
        let mut seen = [0.0; 1];
        meter.scope.latest(&mut seen);
        assert_eq!(seen, [0.5]);
    }

    #[test]
    fn samples_from_before_a_seek_are_thrown_away_unheard_and_uncounted() {
        let (mut producer, mut consumer) = rtrb::RingBuffer::new(8);
        for sample in [0.9, 0.9, 0.9, 0.9, 0.1, 0.1] {
            producer.push(sample).expect("room");
        }
        let meter = Meter::default();
        meter.stale.store(4, Ordering::Relaxed);
        let mut out = [1.0; 4];
        let mut fade = instant(&meter);
        fill(&mut out, &mut consumer, &meter, &mut fade);
        assert_eq!(out, [0.1, 0.1, 0.0, 0.0]);
        assert_eq!(meter.played.load(Ordering::Relaxed), 1, "only the frame heard counts");
    }

    #[test]
    fn a_dry_ring_fills_the_rest_with_silence_and_counts_whole_frames() {
        let (mut producer, mut consumer) = rtrb::RingBuffer::new(8);
        for sample in [0.25, 0.25, 0.25] {
            producer.push(sample).expect("room");
        }
        let meter = Meter::default();
        let mut out = [1.0; 6];
        let mut fade = instant(&meter);
        fill(&mut out, &mut consumer, &meter, &mut fade);
        assert_eq!(out, [0.25, 0.25, 0.0, 0.0, 0.0, 0.0], "half a frame waits for its other half");
        assert_eq!(meter.played.load(Ordering::Relaxed), 1);
    }
}
