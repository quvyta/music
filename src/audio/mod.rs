//! The player: a thread that decodes one track at a time into a ring of samples, and an output
//! that takes them from the ring as the sound goes out.
//!
//! The screen talks to the player through [`Player`]: it sends orders and reads a [`Status`]. The
//! position comes from what the output has taken, not from what has been decoded, so the time on
//! screen is the time heard.

mod convert;
mod decode;
mod output;

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use rtrb::{Producer, RingBuffer};

use convert::Converter;
pub use decode::Decoder;
pub use output::{AudioOut, SCOPE};
use output::{Meter, Output};

/// How long the ring between the decoder and the output lasts.
const RING: Duration = Duration::from_millis(500);

/// How long the decoder waits for room in a full ring before looking again.
const WAIT: Duration = Duration::from_millis(10);

/// Where the player stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    /// Nothing is loaded.
    #[default]
    Stopped,
    /// The track is heard.
    Playing,
    /// The track is held where it is.
    Paused,
    /// The track has played to its end.
    Ended,
}

/// What went wrong with the track loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// The file could not be opened or read to its end: why. The next track may well play.
    Track(String),
    /// The sound device could not be opened, or went away while the track played: why. No track
    /// would be heard until it is back.
    Output(String),
}

/// What the screen reads from the player.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Status {
    /// The track loaded, while one is.
    pub track: Option<PathBuf>,
    /// Where the player stands.
    pub state: State,
    /// How far into the track the sound has got.
    pub position: Duration,
    /// What went wrong with the track loaded, when something did.
    pub problem: Option<Problem>,
}

/// What the screen asks of the player. Each track asked for carries its own number, so the
/// player never mistakes the end of the track before for the end of the one just chosen.
enum Order {
    Play(PathBuf, u64),
    Seek(Duration, u64),
    Follow(Option<PathBuf>, u64),
    Quit,
}

/// The part of the status the player's thread writes.
#[derive(Default)]
struct Loaded {
    track: Option<PathBuf>,
    /// The number of the track last asked for.
    generation: u64,
    state: State,
    rate: u32,
    problem: Option<Problem>,
    /// Where a cued track was left, said as its position until the thread has opened it.
    cued_at: Duration,
}

/// What the screen and the player's thread share.
#[derive(Default)]
struct Shared {
    meter: Arc<Meter>,
    /// How many tracks the screen has asked to be played from their start.
    #[cfg(test)]
    plays: std::sync::atomic::AtomicUsize,
    loaded: Mutex<Loaded>,
}

impl Shared {
    fn loaded(&self) -> std::sync::MutexGuard<'_, Loaded> {
        // The thread writes plain values; a panic halfway leaves nothing half-written to fear.
        self.loaded.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The player, running on its own thread for as long as this lives.
pub struct Player {
    orders: Sender<Order>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Player {
    /// Starts a player that writes to `out`.
    #[must_use]
    pub fn start(out: AudioOut) -> Self {
        let (orders, inbox) = mpsc::channel();
        let shared = Arc::new(Shared::default());
        let engine = Engine { out, shared: Arc::clone(&shared), inbox, playing: None, generation: 0, following: None };
        let thread = std::thread::Builder::new().name("qmus-player".to_owned()).spawn(move || engine.run()).ok();
        Self { orders, shared, thread }
    }

    /// A player whose thread could not start, as on a machine out of threads.
    #[cfg(test)]
    fn without_thread() -> Self {
        let (orders, _) = mpsc::channel();
        Self { orders, shared: Arc::new(Shared::default()), thread: None }
    }

    /// Plays the file at `path` from its start, in place of whatever played.
    pub fn play(&self, path: PathBuf) {
        #[cfg(test)]
        self.shared.plays.fetch_add(1, Ordering::Relaxed);
        // What went wrong belongs to the track loaded; a new track starts clean, and heard: a
        // track chosen while the one before was held is not held itself.
        let generation = {
            let mut loaded = self.shared.loaded();
            loaded.generation += 1;
            loaded.track = Some(path.clone());
            loaded.state = State::Playing;
            loaded.problem = None;
            loaded.cued_at = Duration::ZERO;
            loaded.generation
        };
        self.shared.meter.paused.store(false, Ordering::Relaxed);
        self.shared.meter.played.store(0, Ordering::Relaxed);
        if self.orders.send(Order::Play(path, generation)).is_err() {
            // No thread takes the order: nothing would ever be heard, and the screen says so.
            let mut loaded = self.shared.loaded();
            loaded.state = State::Stopped;
            loaded.problem = Some(Problem::Output("the player could not start".to_owned()));
        }
    }

    /// Names the track to go on with once the one loaded ends, or none. When its rate is the same,
    /// its first sample follows the last of this one with nothing between; the status names it
    /// from the moment it is heard.
    pub fn follow_with(&self, path: Option<PathBuf>) {
        let generation = self.shared.loaded().generation;
        let _ = self.orders.send(Order::Follow(path, generation));
    }

    /// Loads the file at `path` held at `at`, as a track left off where it was: nothing is heard
    /// until [`resume`](Self::resume).
    pub fn cue(&self, path: PathBuf, at: Duration) {
        let generation = {
            let mut loaded = self.shared.loaded();
            loaded.generation += 1;
            loaded.track = Some(path.clone());
            loaded.state = State::Paused;
            loaded.problem = None;
            loaded.rate = 0;
            loaded.cued_at = at;
            loaded.generation
        };
        self.shared.meter.paused.store(true, Ordering::Relaxed);
        self.shared.meter.played.store(0, Ordering::Relaxed);
        let _ = self.orders.send(Order::Play(path, generation));
        if !at.is_zero() {
            let _ = self.orders.send(Order::Seek(at, generation));
        }
    }

    /// Holds the sound where it is.
    pub fn pause(&self) {
        // The status says so at once: a read that comes before the player's thread has looked
        // must not hand the screen back the state it just left.
        self.shared.meter.paused.store(true, Ordering::Relaxed);
        self.set_state_if(State::Playing, State::Paused);
    }

    /// Goes on from where the sound was held.
    pub fn resume(&self) {
        self.shared.meter.paused.store(false, Ordering::Relaxed);
        self.set_state_if(State::Paused, State::Playing);
    }

    fn set_state_if(&self, from: State, to: State) {
        let mut loaded = self.shared.loaded();
        if loaded.state == from {
            loaded.state = to;
        }
    }

    /// Moves the track loaded to `at` from its start; the position says so at once, and the
    /// sound follows as soon as the file has been entered there. Past the end, the track ends.
    pub fn seek(&self, at: Duration) {
        let (rate, generation) = {
            let loaded = self.shared.loaded();
            (loaded.rate, loaded.generation)
        };
        if rate == 0 {
            return;
        }
        self.shared.meter.played.store((at.as_secs_f64() * f64::from(rate)) as u64, Ordering::Relaxed);
        let _ = self.orders.send(Order::Seek(at, generation));
    }

    /// Sets how loud the sound goes out, from 0 (silent) to 100 (as the file has it), on a cubic
    /// curve, so each step sounds like the same step to the ear. The system's own volume is left
    /// alone.
    pub fn set_volume(&self, level: u8) {
        let share = f32::from(level.min(100)) / 100.0;
        self.shared.meter.set_gain(share * share * share);
    }

    /// How many tracks the screen has asked to be played from their start.
    #[cfg(test)]
    pub(crate) fn plays(&self) -> usize {
        self.shared.plays.load(Ordering::Relaxed)
    }

    /// What every sample is multiplied by on its way out.
    #[cfg(test)]
    pub(crate) fn gain(&self) -> f32 {
        self.shared.meter.gain()
    }

    /// Fills `out` with the last of the sound that went out, folded to one channel, the oldest
    /// first (at most [`SCOPE`] frames), and gives the rate it went out at: zero while nothing has
    /// been loaded.
    pub fn heard(&self, out: &mut [f32]) -> u32 {
        self.shared.meter.scope.latest(out);
        self.shared.loaded().rate
    }

    /// Where the player stands now.
    #[must_use]
    pub fn status(&self) -> Status {
        let loaded = self.shared.loaded();
        let played = self.shared.meter.played.load(Ordering::Relaxed);
        let position = if loaded.rate == 0 {
            loaded.cued_at
        } else {
            Duration::from_secs_f64(played as f64 / f64::from(loaded.rate))
        };
        Status { track: loaded.track.clone(), state: loaded.state, position, problem: loaded.problem.clone() }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.orders.send(Order::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A track on its way out: its decoder, the ring it fills and the output that empties it.
struct Playing {
    decoder: Option<Decoder>,
    ring: Producer<f32>,
    /// Decoded samples that did not fit into the ring yet.
    waiting: Vec<f32>,
    /// Frames put into the ring, counted the way the output counts what it took.
    pushed: u64,
    /// The track whose samples follow in the ring, and the frame it begins at.
    chained: Option<(PathBuf, u64)>,
    /// Whether the screen named another track to follow after this one was chained: the chained
    /// samples are thrown away unheard at the boundary, and the one named goes on instead.
    refollow: bool,
    /// Frames a second of the output; a track of another rate that follows is converted to it.
    rate: u32,
    /// The rate change of a following track of another rate, while it is decoded.
    convert: Option<Converter>,
    /// A packet of such a track, before it is converted.
    unconverted: Vec<f32>,
    /// Whether the track could not be read to its end; what follows it waits for the screen.
    failed: bool,
    /// Dropped last, after the ring's other end stops being read.
    _output: Output,
}

/// How a track of `from` frames a second follows in an output of `to`: as it is (`Some(None)`),
/// converted (`Some(Some(_))`), or not at all when the two cannot be converted.
fn conversion(from: u32, to: u32) -> Option<Option<Converter>> {
    if from == to { Some(None) } else { Converter::new(from, to).map(Some) }
}

/// The player's thread.
struct Engine {
    out: AudioOut,
    shared: Arc<Shared>,
    inbox: Receiver<Order>,
    playing: Option<Playing>,
    /// The number of the track this thread loaded last.
    generation: u64,
    /// The track to go on with once the one loaded ends.
    following: Option<PathBuf>,
}

impl Engine {
    fn run(mut self) {
        loop {
            // Idle, the thread sleeps until it is asked for something; busy, it looks between
            // packets.
            let order = if self.busy() {
                match self.inbox.recv_timeout(WAIT) {
                    Ok(order) => Some(order),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            } else {
                match self.inbox.recv() {
                    Ok(order) => Some(order),
                    Err(_) => return,
                }
            };
            if !self.step(order) {
                return;
            }
        }
    }

    /// Carries out `order`, when there is one, and feeds the ring; `false` once asked to quit.
    fn step(&mut self, order: Option<Order>) -> bool {
        match order {
            Some(Order::Play(path, generation)) => self.load(&path, generation),
            Some(Order::Seek(at, generation)) => self.seek(at, generation),
            Some(Order::Follow(path, generation)) if generation == self.generation => self.follow(path),
            Some(Order::Quit) => return false,
            Some(Order::Follow(..)) | None => {}
        }
        self.feed();
        true
    }

    /// Whether a track is loaded that still has samples to decode or to hear.
    fn busy(&self) -> bool {
        self.playing.is_some()
    }

    /// The status, when it still belongs to the track this thread loaded last: once the screen
    /// has asked for another, what happens to this one is no longer news.
    fn own(&self) -> Option<std::sync::MutexGuard<'_, Loaded>> {
        let loaded = self.shared.loaded();
        (loaded.generation == self.generation).then_some(loaded)
    }

    /// Opens `path` and an output at its rate, as track number `generation`.
    fn load(&mut self, path: &Path, generation: u64) {
        // The old output stops before the new one opens, so two tracks are never heard at once.
        self.playing = None;
        self.generation = generation;
        self.following = None;
        let meter = &self.shared.meter;
        meter.take_lost();
        let opened = Decoder::open(path).map_err(Problem::Track).and_then(|decoder| {
            let rate = decoder.rate();
            let samples = (u64::from(rate) * 2 * RING.as_millis() as u64 / 1000) as usize;
            let (ring, taken) = RingBuffer::new(samples.max(2));
            meter.played.store(0, Ordering::Relaxed);
            let output = Output::open(self.out, rate, taken, Arc::clone(meter)).map_err(Problem::Output)?;
            Ok((
                rate,
                Playing {
                    decoder: Some(decoder),
                    ring,
                    waiting: Vec::new(),
                    pushed: 0,
                    chained: None,
                    refollow: false,
                    rate,
                    convert: None,
                    unconverted: Vec::new(),
                    failed: false,
                    _output: output,
                },
            ))
        });
        let Some(mut loaded) = self.own() else { return };
        match opened {
            Ok((rate, playing)) => {
                loaded.rate = rate;
                loaded.problem = None;
                drop(loaded);
                self.playing = Some(playing);
            }
            Err(problem) => {
                loaded.state = State::Stopped;
                loaded.problem = Some(problem);
            }
        }
    }

    /// Enters the track loaded at `at`, when it is still track number `generation`; what the ring
    /// holds from before is thrown away unheard.
    fn seek(&mut self, at: Duration, generation: u64) {
        if generation != self.generation {
            return;
        }
        let Some(playing) = &mut self.playing else { return };
        if playing.chained.is_some() {
            // The decoder is already on the next track; the last moments of this one are heard
            // out as they are.
            return;
        }
        let Some(decoder) = &mut playing.decoder else { return };
        playing.waiting.clear();
        if let Some(convert) = &mut playing.convert {
            convert.reset();
        }
        let held = playing.ring.buffer().capacity() - playing.ring.slots();
        let meter = &self.shared.meter;
        meter.stale.store(held, Ordering::Relaxed);
        if decoder.seek(at).is_err() {
            // Past the end, or a file that cannot be entered: the rest is not heard.
            playing.decoder = None;
        }
        let rate = self.shared.loaded().rate;
        let frames = (at.as_secs_f64() * f64::from(rate)) as u64;
        playing.pushed = frames;
        meter.played.store(frames, Ordering::Relaxed);
    }

    /// Takes `path` as the track to go on with. One already chained into the ring stays only
    /// while it is still the one named; otherwise it is dropped at the boundary.
    fn follow(&mut self, path: Option<PathBuf>) {
        if let Some(playing) = &mut self.playing
            && let Some((chained, _)) = &playing.chained
        {
            playing.refollow = path.as_ref() != Some(chained);
            if !playing.refollow {
                // The one named is already on its way; naming it again must not chain it twice.
                self.following = None;
                return;
            }
        }
        self.following = path;
    }

    /// Names the chained track as the one heard once the output has reached its first frame,
    /// counting from there; a chained track the screen no longer wants is thrown away there.
    fn promote(&mut self) {
        let Some(playing) = &mut self.playing else { return };
        let Some((_, at)) = &playing.chained else { return };
        let at = *at;
        let meter = &self.shared.meter;
        let played = meter.played.load(Ordering::Relaxed);
        if played < at {
            return;
        }
        if playing.refollow {
            let held = playing.ring.buffer().capacity() - playing.ring.slots();
            meter.stale.store(held, Ordering::Relaxed);
            playing.waiting.clear();
            playing.decoder = None;
            playing.convert = None;
            playing.chained = None;
            playing.refollow = false;
            // What is still in the ring goes unheard, so the next track begins where the output is.
            playing.pushed = played;
            return;
        }
        meter.played.fetch_sub(at, Ordering::Relaxed);
        playing.pushed -= at;
        let Some((path, _)) = playing.chained.take() else { return };
        if let Some(mut loaded) = self.own() {
            loaded.track = Some(path);
        }
    }

    /// Decodes into the ring until it is full, and notices when the track has been heard to its end.
    fn feed(&mut self) {
        if self.playing.is_none() {
            return;
        }
        if let Some(reason) = self.shared.meter.take_lost() {
            // The ring would never empty again; the track stops where the sound stopped.
            self.playing = None;
            if let Some(mut loaded) = self.own() {
                loaded.state = State::Stopped;
                loaded.problem = Some(Problem::Output(reason));
            }
            return;
        }
        self.promote();
        let generation = self.generation;
        let Some(playing) = &mut self.playing else { return };
        loop {
            if playing.waiting.is_empty() {
                // Once this track is decoded, the next goes on in the same ring; one of another
                // rate is converted to the output's on the way.
                if playing.decoder.is_none()
                    && playing.chained.is_none()
                    && !playing.failed
                    && let Some(path) = self.following.take()
                    && let Ok(next) = Decoder::open(&path)
                    && let Some(convert) = conversion(next.rate(), playing.rate)
                {
                    playing.decoder = Some(next);
                    playing.convert = convert;
                    playing.chained = Some((path, playing.pushed));
                }
                let Some(decoder) = &mut playing.decoder else { break };
                let decoded = match &mut playing.convert {
                    None => decoder.next_into(&mut playing.waiting),
                    Some(convert) => {
                        playing.unconverted.clear();
                        let decoded = decoder.next_into(&mut playing.unconverted);
                        convert.push(&playing.unconverted, &mut playing.waiting);
                        if !matches!(decoded, Ok(true)) {
                            convert.finish(&mut playing.waiting);
                        }
                        decoded
                    }
                };
                match decoded {
                    Ok(true) => {}
                    Ok(false) => playing.decoder = None,
                    Err(error) => {
                        // What was decoded is still heard; the screen says why the rest is not.
                        playing.failed = true;
                        playing.decoder = None;
                        let mut loaded = self.shared.loaded();
                        if loaded.generation == generation {
                            loaded.problem = Some(Problem::Track(error));
                        }
                    }
                }
                continue;
            }
            let room = playing.ring.slots().min(playing.waiting.len()) & !1;
            if room == 0 {
                return;
            }
            if let Ok(mut chunk) = playing.ring.write_chunk(room) {
                let (first, second) = chunk.as_mut_slices();
                let split = first.len();
                first.copy_from_slice(&playing.waiting[..split]);
                second.copy_from_slice(&playing.waiting[split..room]);
                chunk.commit_all();
                playing.pushed += room as u64 / 2;
            }
            playing.waiting.drain(..room);
        }
        // Everything is decoded; the track has ended once the output has taken the last sample.
        if playing.ring.slots() == playing.ring.buffer().capacity() {
            self.playing = None;
            if let Some(mut loaded) = self.own()
                && loaded.state == State::Playing
            {
                loaded.state = State::Ended;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::testing::{Scratch, sine_wav};

    /// Long enough for any of these short tracks to be heard on a loaded machine.
    const GENEROUS: Duration = Duration::from_secs(20);

    fn wait_for(player: &Player, what: impl Fn(&Status) -> bool) -> Status {
        let start = Instant::now();
        loop {
            let status = player.status();
            if what(&status) {
                return status;
            }
            assert!(start.elapsed() < GENEROUS, "the player never got there: {status:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_track_plays_to_its_end_at_the_pace_of_real_sound() {
        let scratch = Scratch::new("player-end");
        let path = scratch.path("tone.wav");
        sine_wav(&path, 8_000, 2, 0.3, 440.0);
        let player = Player::start(AudioOut::Null);
        let started = Instant::now();
        player.play(path.clone());
        let status = wait_for(&player, |status| status.state == State::Ended);
        assert!(started.elapsed() >= Duration::from_millis(250), "0.3 s of sound took {:?}", started.elapsed());
        assert_eq!(status.track, Some(path));
        assert_eq!(status.position, Duration::from_millis(300), "every frame was heard");
    }

    #[test]
    fn a_held_track_stays_where_it_is_and_goes_on_from_there() {
        let scratch = Scratch::new("player-pause");
        let path = scratch.path("long.wav");
        sine_wav(&path, 8_000, 1, 3.0, 220.0);
        let player = Player::start(AudioOut::Null);
        player.play(path);
        wait_for(&player, |status| status.position >= Duration::from_millis(100));
        player.pause();
        wait_for(&player, |status| status.state == State::Paused);
        // The sound fades out over its last few milliseconds; after that nothing more is heard.
        std::thread::sleep(Duration::from_millis(100));
        let held = player.status().position;
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(player.status().position, held, "nothing is heard while held");
        player.resume();
        wait_for(&player, |status| status.state == State::Playing && status.position > held);
    }

    #[test]
    fn holding_and_going_on_show_in_the_very_next_status() {
        let scratch = Scratch::new("player-at-once");
        let path = scratch.path("tone.wav");
        sine_wav(&path, 8_000, 1, 3.0, 440.0);
        let player = Player::start(AudioOut::Null);
        player.play(path);
        wait_for(&player, |status| status.position > Duration::ZERO);
        for _ in 0..50 {
            player.pause();
            assert_eq!(player.status().state, State::Paused);
            player.resume();
            assert_eq!(player.status().state, State::Playing);
        }
    }

    #[test]
    fn a_seek_moves_the_position_at_once_and_the_sound_soon_after() {
        let scratch = Scratch::new("player-seek");
        let path = scratch.path("tone.wav");
        sine_wav(&path, 8_000, 1, 6.0, 440.0);
        let player = Player::start(AudioOut::Null);
        player.play(path);
        wait_for(&player, |status| status.position > Duration::ZERO);
        player.seek(Duration::from_secs(5));
        assert!(player.status().position >= Duration::from_secs(5), "{:?}", player.status());
        let asked = Instant::now();
        wait_for(&player, |status| status.state == State::Ended);
        // The last second is heard, not the five before it.
        assert!(asked.elapsed() < Duration::from_secs(4), "took {:?}", asked.elapsed());
    }

    #[test]
    fn a_seek_past_the_end_ends_the_track() {
        let scratch = Scratch::new("player-seek-end");
        let path = scratch.path("tone.wav");
        sine_wav(&path, 8_000, 1, 6.0, 440.0);
        let player = Player::start(AudioOut::Null);
        player.play(path);
        wait_for(&player, |status| status.position > Duration::ZERO);
        let asked = Instant::now();
        player.seek(Duration::from_secs(60));
        wait_for(&player, |status| status.state == State::Ended);
        assert!(asked.elapsed() < Duration::from_secs(4), "took {:?}", asked.elapsed());
    }

    #[test]
    fn the_volume_follows_a_curve_the_ear_hears_as_even() {
        let player = Player::start(AudioOut::Null);
        player.set_volume(50);
        assert!((player.gain() - 0.125).abs() < 1e-6);
        player.set_volume(0);
        assert_eq!(player.gain(), 0.0);
        player.set_volume(250);
        assert_eq!(player.gain(), 1.0);
    }

    #[test]
    fn a_player_whose_thread_could_not_start_says_so_when_asked_to_play() {
        let scratch = Scratch::new("player-no-thread");
        let path = scratch.path("tone.wav");
        sine_wav(&path, 8_000, 1, 1.0, 440.0);
        let player = Player::without_thread();
        player.play(path);
        let status = player.status();
        assert_eq!(status.state, State::Stopped);
        assert!(matches!(status.problem, Some(Problem::Output(_))), "{status:?}");
    }

    #[test]
    fn a_track_named_after_the_next_was_chained_goes_on_instead_of_it() {
        let scratch = Scratch::new("player-refollow");
        let (first, unwanted, wanted) = (scratch.path("a.wav"), scratch.path("b.wav"), scratch.path("c.wav"));
        sine_wav(&first, 8_000, 1, 0.3, 440.0);
        sine_wav(&unwanted, 8_000, 1, 1.0, 550.0);
        sine_wav(&wanted, 8_000, 1, 0.4, 660.0);
        let player = Player::start(AudioOut::Null);
        player.play(first);
        player.follow_with(Some(unwanted.clone()));
        // The first track fits the ring whole, so the unwanted one is chained into it at once.
        std::thread::sleep(Duration::from_millis(100));
        player.follow_with(Some(wanted.clone()));
        let start = Instant::now();
        let status = loop {
            let status = player.status();
            assert_ne!(status.track.as_ref(), Some(&unwanted), "the track no longer wanted is never heard");
            if status.state == State::Ended {
                break status;
            }
            assert!(start.elapsed() < GENEROUS, "the tracks never ended: {status:?}");
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(status.track, Some(wanted), "the one named last went on");
        assert_eq!(status.position, Duration::from_millis(400), "heard whole, from its start: {status:?}");
    }

    #[test]
    fn a_followed_track_of_the_same_rate_goes_on_with_no_stop_between() {
        let scratch = Scratch::new("player-gapless");
        let (first, second) = (scratch.path("a.wav"), scratch.path("b.wav"));
        sine_wav(&first, 8_000, 1, 0.5, 440.0);
        sine_wav(&second, 8_000, 1, 0.5, 660.0);
        let player = Player::start(AudioOut::Null);
        player.play(first.clone());
        player.follow_with(Some(second.clone()));
        let start = Instant::now();
        loop {
            let status = player.status();
            assert_eq!(status.state, State::Playing, "the sound never stops between the two: {status:?}");
            if status.track.as_ref() == Some(&second) {
                assert!(status.position < Duration::from_millis(300), "counted from the second's start: {status:?}");
                break;
            }
            assert_eq!(status.track.as_ref(), Some(&first));
            assert!(start.elapsed() < GENEROUS, "the second track never came: {status:?}");
            std::thread::sleep(Duration::from_millis(1));
        }
        let status = wait_for(&player, |status| status.state == State::Ended);
        assert_eq!(status.track, Some(second), "the second is the one that ended");
    }

    #[test]
    fn a_followed_track_of_another_rate_goes_on_with_no_stop_and_lasts_as_long_as_it_does() {
        let scratch = Scratch::new("player-other-rate");
        let (first, second) = (scratch.path("a.wav"), scratch.path("b.wav"));
        sine_wav(&first, 8_000, 1, 0.4, 440.0);
        sine_wav(&second, 16_000, 1, 0.5, 660.0);
        let player = Player::start(AudioOut::Null);
        player.play(first.clone());
        player.follow_with(Some(second.clone()));
        let start = Instant::now();
        loop {
            let status = player.status();
            assert_eq!(status.state, State::Playing, "the sound never stops between the two: {status:?}");
            if status.track.as_ref() == Some(&second) {
                break;
            }
            assert!(start.elapsed() < GENEROUS, "the second track never came: {status:?}");
            std::thread::sleep(Duration::from_millis(1));
        }
        let status = wait_for(&player, |status| status.state == State::Ended);
        assert_eq!(status.track, Some(second), "the second is the one that ended");
        // Heard at the first track's rate, the second still lasts its own half second.
        assert_eq!(status.position, Duration::from_millis(500), "{status:?}");
    }

    #[test]
    fn a_file_that_cannot_be_played_stops_the_player_and_says_why() {
        let scratch = Scratch::new("player-broken");
        let path = scratch.path("broken.flac");
        std::fs::write(&path, "not music").expect("file");
        let player = Player::start(AudioOut::Null);
        player.play(path);
        let status = wait_for(&player, |status| status.problem.is_some());
        assert_eq!(status.state, State::Stopped);
        assert!(matches!(status.problem, Some(Problem::Track(_))), "the file is to blame: {status:?}");
    }

    #[test]
    fn a_new_track_starts_from_its_beginning() {
        let scratch = Scratch::new("player-next");
        let (first, second) = (scratch.path("first.wav"), scratch.path("second.wav"));
        sine_wav(&first, 8_000, 1, 3.0, 220.0);
        sine_wav(&second, 8_000, 1, 3.0, 330.0);
        let player = Player::start(AudioOut::Null);
        player.play(first);
        wait_for(&player, |status| status.position >= Duration::from_millis(500));
        player.play(second.clone());
        let status = player.status();
        assert_eq!(status.track, Some(second));
        assert!(status.position < Duration::from_millis(500), "the second track starts over: {status:?}");
    }

    #[test]
    fn a_track_ending_just_as_another_is_chosen_leaves_the_new_one_playing() {
        let scratch = Scratch::new("player-race");
        let (first, second) = (scratch.path("first.wav"), scratch.path("second.wav"));
        sine_wav(&first, 8_000, 1, 0.05, 220.0);
        sine_wav(&second, 8_000, 1, 1.0, 330.0);
        // The player's thread is driven by hand here, so the moment between the screen choosing a
        // track and the thread hearing about it can be held open.
        let (orders, inbox) = mpsc::channel();
        let shared = Arc::new(Shared::default());
        let player = Player { orders, shared: Arc::clone(&shared), thread: None };
        let mut engine = Engine { out: AudioOut::Null, shared, inbox, playing: None, generation: 0, following: None };
        player.play(first);
        assert!(engine.step(engine.inbox.try_recv().ok()));
        let start = Instant::now();
        while engine.shared.meter.played.load(Ordering::Relaxed) < 400 {
            assert!(start.elapsed() < GENEROUS, "the first track was never heard to its end");
            std::thread::sleep(Duration::from_millis(10));
        }
        player.play(second.clone());
        assert!(engine.step(None), "the thread sees the first track end before it hears of the second");
        assert_eq!(player.status().state, State::Playing, "the track just chosen has not ended");
        assert!(engine.step(engine.inbox.try_recv().ok()));
        let status = player.status();
        assert_eq!((status.track, status.state), (Some(second), State::Playing));
    }

    #[test]
    fn a_device_that_goes_away_stops_the_track_and_says_why() {
        let scratch = Scratch::new("player-gone");
        let path = scratch.path("long.wav");
        sine_wav(&path, 8_000, 1, 3.0, 220.0);
        let player = Player::start(AudioOut::Vanishing(Duration::from_millis(200)));
        player.play(path);
        let status = wait_for(&player, |status| status.problem.is_some());
        assert_eq!(status.state, State::Stopped);
        assert!(matches!(status.problem, Some(Problem::Output(_))), "the device is to blame: {status:?}");
    }
}
