//! The screen on the session bus: what a media key, `playerctl` or a desktop's "now playing"
//! corner asks of qmus comes in here as a [`Request`], and what qmus is doing goes out after every
//! change.

use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use qframe::prelude::*;
use qframe::runtime::Task;
use qframe::widgets::Toast;

use super::{Msg, Music};
use crate::audio::State;
use crate::mpris::{Loop, Now, Playback, Request, Server};
use crate::queue::Repeat;

/// Where qmus offers itself to the desktop's media keys and "now playing" corners.
#[derive(Debug, Clone)]
pub enum Bus {
    /// Nowhere: nothing outside qmus can drive it.
    Nowhere,
    /// The session bus, when there is one; over SSH or in a container there is none, and qmus
    /// carries on without it.
    Session,
    /// One end of a socket pair, which a test holds the other end of.
    Socket(Arc<std::os::unix::net::UnixStream>),
}

impl Music {
    /// Takes qmus's place on the bus off the drawing thread, then hands every call that comes in to
    /// the screen for as long as qmus runs.
    pub(super) fn serve_bus(&self) -> Command<Msg> {
        let bus = self.machine.bus.clone();
        if matches!(bus, Bus::Nowhere) {
            return Command::none();
        }
        Command::task(Task::new("mpris", move |cx| {
            let (asked, requests) = mpsc::channel();
            let ask = move |request| {
                let _ = asked.send(request);
            };
            let server = match bus {
                Bus::Nowhere => None,
                Bus::Session => Server::start(ask),
                Bus::Socket(socket) => socket.try_clone().ok().and_then(|socket| Server::on(socket, ask).ok()),
            };
            // No bus is nothing to tell the person about: qmus plays on as it would anyway.
            let Some(server) = server else { return Err("no session bus".to_owned()) };
            cx.send(Msg::Served(Arc::new(server)));
            while let Some(request) = cx.recv(&requests) {
                cx.send(Msg::Asked(request));
            }
            Err("the bus closed".to_owned())
        }))
    }

    /// Does what a client on the bus asked for, the way the same key or button would.
    pub(super) fn asked(&mut self, request: Request) -> Command<Msg> {
        match request {
            Request::PlayPause => return self.play_pause(),
            Request::Play if self.status.state != State::Playing => return self.play_pause(),
            Request::Pause if self.status.state == State::Playing => return self.play_pause(),
            Request::Play | Request::Pause => {}
            Request::Stop => {
                if matches!(self.status.state, State::Playing | State::Paused) {
                    self.player.pause();
                    self.player.seek(Duration::ZERO);
                    self.status = self.player.status();
                    self.announce_seek();
                }
            }
            Request::Next => return self.next(),
            Request::Previous => return self.previous(),
            Request::SeekBy(offset) => {
                let by = Duration::from_micros(offset.unsigned_abs());
                let at = if offset < 0 {
                    self.status.position.saturating_sub(by)
                } else {
                    self.status.position.saturating_add(by)
                };
                // MPRIS: a jump past the end goes on to the next track.
                if self.current().and_then(|track| track.duration).is_some_and(|total| at >= total) {
                    return self.next();
                }
                return self.seek_to(at);
            }
            Request::SetPosition { track, position } => {
                let length = self.current().and_then(|track| track.duration);
                // MPRIS: a place past the end of the track is not one to go to.
                if self.track_id() == track && length.is_none_or(|total| position <= total) {
                    return self.seek_to(position);
                }
            }
            Request::SetVolume(share) => {
                let level = (share.clamp(0.0, 1.0) * 100.0).round();
                // Between 0 and 100 after the clamp, so the cast loses nothing.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                self.set_volume(level as u8);
            }
            Request::SetShuffle(on) => {
                if on != self.queue.is_shuffled() {
                    self.shuffle();
                }
            }
            Request::SetLoop(wanted) => {
                let repeat = match wanted {
                    Loop::None => Repeat::Off,
                    Loop::Track => Repeat::One,
                    Loop::Playlist => Repeat::All,
                };
                self.queue.set_repeat(repeat);
                self.follow();
            }
            Request::Open(path) => return self.open_file(&path),
            Request::Quit => {
                self.keep_queue();
                return Command::quit();
            }
        }
        Command::none()
    }

    /// Plays the file at `path` when it is one of the tracks shown; qmus plays its own library and
    /// says so of anything else.
    fn open_file(&mut self, path: &Path) -> Command<Msg> {
        match self.places.get(path).copied() {
            Some(index) => self.play(index),
            None => Command::toast(
                Toast::warning(t!("music.bus.elsewhere.title"))
                    .body(t!("music.bus.elsewhere.text", path = path.display().to_string().as_str()))
                    .key("open"),
            ),
        }
    }

    /// Moves the track heard to `at`, and tells the bus it jumped.
    fn seek_to(&mut self, at: Duration) -> Command<Msg> {
        if !matches!(self.status.state, State::Playing | State::Paused) {
            return Command::none();
        }
        self.player.seek(at);
        self.status = self.player.status();
        self.announce_seek();
        Command::none()
    }

    /// Tells the bus the sound jumped, for the clients that count along with it.
    pub(super) fn announce_seek(&self) {
        if let Some(server) = &self.mpris {
            server.seeked(self.status.position);
        }
    }

    /// The number the bus knows the track loaded by: its row, counted from one; nothing loaded is
    /// zero.
    fn track_id(&self) -> u64 {
        self.current.map_or(0, |row| row as u64 + 1)
    }

    /// Tells the bus what qmus is doing now; only what changed goes out.
    pub(super) fn announce(&self) {
        let Some(server) = &self.mpris else { return };
        server.update(&self.now());
    }

    /// What qmus is doing, as a client on the bus sees it.
    pub(super) fn now(&self) -> Now {
        let status = match self.status.state {
            State::Playing => Playback::Playing,
            State::Paused => Playback::Paused,
            State::Stopped | State::Ended => Playback::Stopped,
        };
        let loop_status = match self.queue.repeat() {
            Repeat::Off => Loop::None,
            Repeat::One => Loop::Track,
            Repeat::All => Loop::Playlist,
        };
        let volume = if self.muted { 0.0 } else { f64::from(self.volume) / 100.0 };
        let shared = Now {
            status,
            volume,
            shuffle: self.queue.is_shuffled(),
            loop_status,
            position: self.status.position,
            ..Now::default()
        };
        let Some(track) = self.current() else { return shared };
        Now {
            id: self.track_id(),
            title: track.title.clone(),
            artists: listed(&track.artist),
            album: track.album.clone(),
            album_artists: listed(&track.artist),
            track_number: track.number,
            length: track.duration,
            art: self.art.as_ref().and_then(|art| art.file.clone()),
            path: Some(track.path.clone()),
            can_next: self.queue.peek_next().is_some(),
            can_previous: true,
            ..shared
        }
    }
}

/// A name as the bus lists it: none when the tags gave none.
fn listed(name: &str) -> Vec<String> {
    if name.is_empty() { Vec::new() } else { vec![name.to_owned()] }
}
