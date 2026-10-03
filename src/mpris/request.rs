//! One call that arrives on the bus, as a value qmus answers.
//!
//! The server itself changes nothing: a media key, a `playerctl` command and a "now playing"
//! corner all become one of these, and qmus decides what to do with it. The one thing the server
//! does decide is which calls make sense at all: a jump inside another track is dropped before it
//! gets here, because MPRIS says the client should not have asked for it.

use std::path::PathBuf;
use std::time::Duration;

use super::Loop;

/// What a client on the bus asks qmus to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Play the track if it is held, hold it if it is heard.
    PlayPause,
    /// Play, whether it is heard or not.
    Play,
    /// Hold the track where it is.
    Pause,
    /// Stop playing.
    Stop,
    /// Go on to the next track.
    Next,
    /// Go back to the previous track.
    Previous,
    /// Jump by so many microseconds; a negative offset goes back.
    SeekBy(i64),
    /// Go to this place in this track.
    SetPosition {
        /// The track to jump in, which is the one being heard.
        track: u64,
        /// Where in it to jump to.
        position: Duration,
    },
    /// Play the sound at this level, from nothing to everything.
    SetVolume(f64),
    /// Play in order (`false`) or in shuffled order (`true`).
    SetShuffle(bool),
    /// Repeat the queue, one track or all of it.
    SetLoop(Loop),
    /// Play this file.
    Open(PathBuf),
    /// End qmus.
    Quit,
}
