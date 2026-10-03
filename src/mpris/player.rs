//! The `org.mpris.MediaPlayer2.Player` interface: what qmus is doing, and the calls that change
//! it.
//!
//! Every call here becomes one [`Request`], and every property is read from the last
//! [`Now`](super::Now) qmus gave. The three writable properties do not announce themselves when a
//! client sets them: qmus has not done it yet, and only qmus knows when it has. The change is
//! announced when qmus says so with [`Server::update`](super::Server::update).

use zbus::interface;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::ObjectPath;

use super::now;
use super::uri;
use super::{Loop, Now, Request, Session};

/// The `org.mpris.MediaPlayer2.Player` interface, served at the one object path MPRIS names.
#[derive(Debug)]
pub(super) struct Player {
    /// What the properties answer from, and where a call goes.
    pub(super) session: Session,
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    /// Plays the track if it is held, holds it if it is heard.
    fn play_pause(&self) {
        self.session.ask(Request::PlayPause);
    }

    /// Plays, whether the track is heard or not.
    fn play(&self) {
        self.session.ask(Request::Play);
    }

    /// Holds the track where it is.
    fn pause(&self) {
        self.session.ask(Request::Pause);
    }

    /// Stops playing.
    fn stop(&self) {
        self.session.ask(Request::Stop);
    }

    /// Goes on to the next track.
    fn next(&self) {
        self.session.ask(Request::Next);
    }

    /// Goes back to the previous track.
    fn previous(&self) {
        self.session.ask(Request::Previous);
    }

    /// Jumps so many microseconds from where the sound is; a negative offset goes back.
    fn seek(&self, offset: i64) {
        self.session.ask(Request::SeekBy(offset));
    }

    /// Goes to a place in a track. A client that names a track other than the one being heard is
    /// asking about a track that is not playing, and MPRIS says such a call is ignored.
    // zbus hands a method its arguments by value.
    #[allow(clippy::needless_pass_by_value)]
    fn set_position(&self, track: ObjectPath<'_>, position: i64) {
        let Some(track) = Now::track_of(&track) else { return };
        if self.session.now().id != track {
            return;
        }
        self.session.ask(Request::SetPosition { track, position: now::duration(position) });
    }

    /// Plays a file qmus can open. qmus plays the person's own music and nothing that would come
    /// off the network, so any other kind of URL is turned away.
    fn open_uri(&self, uri: &str) -> zbus::fdo::Result<()> {
        let Some(path) = uri::path_of(uri) else {
            return Err(zbus::fdo::Error::InvalidArgs(format!("qmus opens local files, not {uri}")));
        };
        self.session.ask(Request::Open(path));
        Ok(())
    }

    /// Where the sound stands: heard, held, or nothing at all.
    #[zbus(property)]
    fn playback_status(&self) -> &'static str {
        self.session.now().status.word()
    }

    /// How the queue repeats.
    #[zbus(property(emits_changed_signal = "false"))]
    fn loop_status(&self) -> &'static str {
        self.session.now().loop_status.word()
    }

    /// Sets how the queue repeats. The answer comes with the next [`Server::update`], once qmus
    /// has repeated that way.
    #[zbus(property)]
    fn set_loop_status(&mut self, value: &str) -> zbus::fdo::Result<()> {
        let Some(loop_status) = Loop::of(value) else {
            return Err(zbus::fdo::Error::InvalidArgs(format!("no way of repeating called {value}")));
        };
        self.session.ask(Request::SetLoop(loop_status));
        Ok(())
    }

    /// Whether the queue is shuffled.
    #[zbus(property(emits_changed_signal = "false"))]
    fn shuffle(&self) -> bool {
        self.session.now().shuffle
    }

    /// Shuffles the queue, or puts it back in order. The answer comes with the next
    /// [`Server::update`](super::Server::update), once qmus has done it.
    #[zbus(property)]
    fn set_shuffle(&mut self, value: bool) {
        self.session.ask(Request::SetShuffle(value));
    }

    /// How fast the sound is played, which is as fast as the file says.
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    /// The slowest qmus plays, which is the speed of the file itself.
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    /// The fastest qmus plays, which is the speed of the file itself.
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    /// How loud the sound is, from nothing to everything.
    #[zbus(property(emits_changed_signal = "false"))]
    fn volume(&self) -> f64 {
        self.session.now().volume
    }

    /// Sets how loud the sound is. The answer comes with the next
    /// [`Server::update`](super::Server::update), once qmus is that loud.
    #[zbus(property)]
    fn set_volume(&mut self, value: f64) {
        self.session.ask(Request::SetVolume(value));
    }

    /// The track being heard, its artists, its album and where the file is.
    #[zbus(property)]
    fn metadata(&self) -> std::collections::HashMap<String, zbus::zvariant::Value<'static>> {
        self.session.now().metadata()
    }

    /// How far into the track the sound has got. MPRIS asks for this one to be read rather than
    /// announced, so a client that wants to follow the sound asks for it as often as it needs it.
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        now::micros(self.session.now().position)
    }

    /// Whether there is a track after this one.
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.session.now().can_next
    }

    /// Whether there is a track before this one.
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.session.now().can_previous
    }

    /// Whether a track can be played; with nothing loaded, the first of the list is.
    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    /// Whether the track can be held; it always can, whether or not it is heard.
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    /// Whether the sound can be put somewhere else in the track; it can.
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        true
    }

    /// Whether a client on the bus can drive qmus at all, which it can.
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }

    /// Where the sound jumped to, so a client that was counting along can start again from here.
    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;
}
