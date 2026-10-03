//! The MPRIS server: qmus as a media player on the session bus, so that the desktop's media keys,
//! `playerctl` and a "now playing" corner can drive it.
//!
//! The server holds no state of its own. Every call that arrives becomes one [`Request`] that
//! qmus answers, and qmus says what is true with [`Server::update`] and [`Server::seeked`]. Where
//! there is no session bus — over SSH, in a container — [`Server::start`] answers `None` without a
//! word and qmus carries on as if nothing had been asked of it.

mod now;
mod player;
mod request;
mod root;
mod uri;

#[cfg(test)]
pub(crate) mod tests;

use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use std::os::unix::net::UnixStream;

use zbus::Guid;
use zbus::blocking::Connection;
use zbus::blocking::connection::Builder;
use zbus::fdo::{Properties, RequestNameFlags, RequestNameReply};
use zbus::object_server::Interface;
use zbus::zvariant::OwnedValue;

pub use now::{Loop, Now, Playback};
pub use request::Request;

use player::Player;
use root::Root;

/// The object path MPRIS asks every player to keep its two interfaces at.
const PATH: &str = "/org/mpris/MediaPlayer2";

/// The bus name qmus takes. A second qmus on the same bus takes the same name with its own
/// process number on the end of it, so that both can be found and driven.
const NAME: &str = "org.mpris.MediaPlayer2.qmus";

/// The MPRIS server: two interfaces on the bus, and the way to tell the clients on it what qmus
/// is doing.
///
/// The screen keeps one of these for as long as it runs. Nothing in here changes the sound: a call
/// that arrives is a [`Request`] for qmus, and what is true is what qmus last said with
/// [`Server::update`].
pub struct Server {
    /// What the interfaces answer from, and the way to ask qmus for a change.
    session: Session,
    /// Where the news goes, to be sent by the thread below.
    outbox: Option<Sender<News>>,
    /// The thread that puts the news on the bus. It is a thread of its own because the screen
    /// calls [`Server::update`] from its update loop, and a bus that takes its time over it must
    /// not hold up the drawing of a frame.
    announcer: Option<JoinHandle<()>>,
}

impl Server {
    /// Takes the MPRIS server on the session bus, giving every call that arrives to `requests`.
    ///
    /// There is no session bus over SSH or inside a container, and there is nothing to say about
    /// that: the answer is `None`, nothing is printed, and qmus plays on.
    #[must_use]
    pub fn start(requests: impl Fn(Request) + Send + Sync + 'static) -> Option<Self> {
        let builder = Builder::session().ok()?;
        let (connection, server) = Self::serve(builder, Session::new(requests)).ok()?;
        take_name(&connection).ok()?;
        Some(server)
    }

    /// Takes the MPRIS server on one end of a socket pair, giving every call that arrives to
    /// `requests`. A test speaks to qmus over the other end this way, and never anywhere near the
    /// person's own session bus. The greeting waits for the other end to greet back.
    ///
    /// # Errors
    ///
    /// Returns the bus's error when the two ends cannot greet each other.
    pub fn on(socket: UnixStream, requests: impl Fn(Request) + Send + Sync + 'static) -> zbus::Result<Self> {
        let builder = Builder::async_io_unix_stream(socket).server(Guid::generate())?.p2p();
        Self::serve(builder, Session::new(requests)).map(|(_, server)| server)
    }

    /// Says what is true now. Every property that says something else than it did at the last call
    /// is announced to the clients on the bus, all in one signal, and the ones that stayed the
    /// same are left quiet.
    pub fn update(&self, now: &Now) {
        let changed = self.session.say(now);
        if changed.is_empty() {
            return;
        }
        if let Some(news) = self.outbox.as_ref() {
            let _ = news.send(News::Changed(changed));
        }
    }

    /// Says that the sound jumped to `position`, for the clients that follow the track by signal
    /// rather than by asking again where it is.
    pub fn seeked(&self, position: Duration) {
        if let Some(news) = self.outbox.as_ref() {
            let _ = news.send(News::Seeked(now::micros(position)));
        }
    }

    /// Opens the connection `builder` makes with the two interfaces already served on it, and
    /// starts the thread that announces news. The interfaces go in before the connection opens:
    /// one served afterwards misses a call that arrives while it is being put in place, and the
    /// client that made it waits for an answer forever.
    fn serve(builder: Builder<'_>, session: Session) -> zbus::Result<(Connection, Self)> {
        let connection = builder
            .serve_at(PATH, Root { session: session.clone() })?
            .serve_at(PATH, Player { session: session.clone() })?
            .build()?;
        let (outbox, inbox) = mpsc::channel();
        let announcing = connection.clone();
        let announcer =
            std::thread::Builder::new().name("qmus-mpris".to_owned()).spawn(move || announce(&announcing, &inbox)).ok();
        Ok((connection, Self { session, outbox: Some(outbox), announcer }))
    }
}

impl Debug for Server {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Server").field("said", &*self.session.now()).finish_non_exhaustive()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // Closing the outbox is what ends the thread. The sender is put away first because a field
        // is dropped only after this runs, and a sender still being held would keep the thread
        // waiting for news that will never come.
        drop(self.outbox.take());
        if let Some(announcer) = self.announcer.take() {
            let _ = announcer.join();
        }
    }
}

/// Something that has to be said on the bus.
enum News {
    /// These properties now say something else than they did.
    Changed(HashMap<&'static str, OwnedValue>),
    /// The sound jumped to this place, in microseconds.
    Seeked(i64),
}

/// The thread that puts news on the bus. It holds the connection, because the connection is what
/// the news goes out on, and the news goes out one signal at a time as it arrives.
fn announce(connection: &Connection, inbox: &Receiver<News>) {
    while let Ok(news) = inbox.recv() {
        let sent = match news {
            News::Changed(changed) => connection.emit_signal(
                None::<&str>,
                PATH,
                Properties::name(),
                "PropertiesChanged",
                &(Player::name(), changed, Vec::<String>::new()),
            ),
            News::Seeked(position) => connection.emit_signal(None::<&str>, PATH, Player::name(), "Seeked", &position),
        };
        if sent.is_err() {
            // The connection is gone, and so is everyone who was listening on it.
            return;
        }
    }
}

/// Takes the bus name qmus answers to, or that same name with this process's number on the end of
/// it when another qmus is already answering to it.
fn take_name(connection: &Connection) -> zbus::Result<()> {
    let asked = connection.request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into());
    if matches!(asked, Ok(RequestNameReply::PrimaryOwner)) {
        return Ok(());
    }
    connection.request_name(format!("{NAME}.instance{}", std::process::id()))?;
    Ok(())
}

/// What the two interfaces answer from, and the way they ask qmus for a change. Both interfaces
/// hold one of these, and so does the server that remembers what they said last.
#[derive(Debug, Clone)]
pub(super) struct Session {
    /// What qmus last said was true.
    said: Arc<Mutex<Now>>,
    /// Where a call that asks for something goes.
    requests: Ask,
}

impl Session {
    /// A session that hands every call to `requests`, with nothing playing yet.
    pub(super) fn new(requests: impl Fn(Request) + Send + Sync + 'static) -> Self {
        Self { said: Arc::new(Mutex::new(Now::default())), requests: Ask(Arc::new(requests)) }
    }

    /// What is true, as the properties answer it.
    pub(super) fn now(&self) -> MutexGuard<'_, Now> {
        // The screen writes whole values; a panic halfway through leaves nothing half-written.
        self.said.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Hands `request` to qmus.
    pub(super) fn ask(&self, request: Request) {
        (self.requests.0)(request);
    }

    /// Says what is true now, and answers with the properties that changed to say it.
    fn say(&self, now: &Now) -> HashMap<&'static str, OwnedValue> {
        let mut said = self.now();
        let changed = now.changed_since(&said);
        *said = now.clone();
        changed
    }
}

/// The way the interfaces ask qmus for a change. What sits behind it is qmus's own business, and
/// it is of no use to anyone reading about the server.
#[derive(Clone)]
struct Ask(Arc<dyn Fn(Request) + Send + Sync>);

impl Debug for Ask {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("the way qmus is asked")
    }
}
