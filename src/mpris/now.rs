//! What qmus says is true right now: the state the two interfaces answer from, and the changes
//! they announce to the clients that are listening.
//!
//! A change is only announced when it is a change. The screen calls
//! [`crate::mpris::Server::update`] from its update loop, which runs far more often than anything
//! on the bus changes, so telling clients about every frame would drown them in news about
//! nothing.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use zbus::zvariant::{ObjectPath, OwnedValue, Str, Value};

use super::uri;

/// Where the ids of the tracks qmus has played live on the bus.
const TRACKS: &str = "/org/quvyta/qmus/track";

/// The path MPRIS tells clients to use while no track is loaded.
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";

/// What qmus is doing with the sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Playback {
    /// Nothing is loaded.
    #[default]
    Stopped,
    /// The track is heard.
    Playing,
    /// The track is held where it is.
    Paused,
}

impl Playback {
    /// The word MPRIS uses for this.
    pub(super) const fn word(self) -> &'static str {
        match self {
            Self::Stopped => "Stopped",
            Self::Playing => "Playing",
            Self::Paused => "Paused",
        }
    }
}

/// How the queue repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Loop {
    /// The queue ends after the last track.
    #[default]
    None,
    /// The track being heard is heard again.
    Track,
    /// The queue starts over after the last track.
    Playlist,
}

impl Loop {
    /// The word MPRIS uses for this.
    pub(super) const fn word(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Track => "Track",
            Self::Playlist => "Playlist",
        }
    }

    /// The way of repeating the client asked for, when it is one MPRIS knows.
    pub(super) fn of(word: &str) -> Option<Self> {
        match word {
            "None" => Some(Self::None),
            "Track" => Some(Self::Track),
            "Playlist" => Some(Self::Playlist),
            _ => None,
        }
    }
}

/// Everything qmus is playing right now, as a client on the bus should see it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Now {
    /// Which track this is; `0` while nothing is loaded.
    pub id: u64,
    /// The name of the track.
    pub title: String,
    /// The names of the artists, as the tags give them.
    pub artists: Vec<String>,
    /// The name of the album.
    pub album: String,
    /// The names of the album's artists, as the tags give them.
    pub album_artists: Vec<String>,
    /// The place of the track on its album, when the tags say.
    pub track_number: Option<u32>,
    /// How long the track plays, when the file says.
    pub length: Option<Duration>,
    /// The picture of the album, in the cache, when there is one.
    pub art: Option<PathBuf>,
    /// The file the sound comes from.
    pub path: Option<PathBuf>,
    /// What qmus is doing with the sound.
    pub status: Playback,
    /// How loud the sound is, from nothing to everything.
    pub volume: f64,
    /// Whether the queue is shuffled.
    pub shuffle: bool,
    /// How the queue repeats.
    pub loop_status: Loop,
    /// Whether there is a track after this one.
    pub can_next: bool,
    /// Whether there is a track before this one.
    pub can_previous: bool,
    /// How far into the track the sound has got.
    pub position: Duration,
}

impl Now {
    /// The object path that names the track `id`.
    pub(super) fn track_path(id: u64) -> ObjectPath<'static> {
        // A literal with a number at the end of it is a valid object path whatever the number is;
        // the fallback stands in for a bus that would say otherwise.
        ObjectPath::try_from(format!("{TRACKS}/{id}")).unwrap_or_else(|_| no_track())
    }

    /// The track the object path `path` names, when it names one of ours.
    pub(super) fn track_of(path: &ObjectPath<'_>) -> Option<u64> {
        path.as_str().strip_prefix(TRACKS)?.strip_prefix('/')?.parse().ok()
    }

    /// The track as MPRIS describes it: what is heard, and where it lives. A client that reads
    /// this knows the title, the artists, how long the track is and which file it is.
    pub(super) fn metadata(&self) -> HashMap<String, Value<'static>> {
        let mut metadata = HashMap::new();
        if self.id == 0 {
            // MPRIS asks for this one entry and nothing else while there is no track.
            metadata.insert("mpris:trackid".to_owned(), Value::from(no_track()));
            return metadata;
        }
        metadata.insert("mpris:trackid".to_owned(), Value::from(Self::track_path(self.id)));
        if let Some(length) = self.length {
            metadata.insert("mpris:length".to_owned(), Value::from(micros(length)));
        }
        if let Some(art) = &self.art {
            metadata.insert("mpris:artUrl".to_owned(), Value::from(uri::url_of(art)));
        }
        metadata.insert("xesam:title".to_owned(), Value::from(self.title.clone()));
        if !self.artists.is_empty() {
            metadata.insert("xesam:artist".to_owned(), Value::from(self.artists.clone()));
        }
        if !self.album.is_empty() {
            metadata.insert("xesam:album".to_owned(), Value::from(self.album.clone()));
        }
        if !self.album_artists.is_empty() {
            metadata.insert("xesam:albumArtist".to_owned(), Value::from(self.album_artists.clone()));
        }
        if let Some(number) = self.track_number {
            // MPRIS counts tracks in a signed number, and no album has more tracks than that.
            let number = i32::try_from(number).unwrap_or(i32::MAX);
            metadata.insert("xesam:trackNumber".to_owned(), Value::from(number));
        }
        if let Some(path) = &self.path {
            metadata.insert("xesam:url".to_owned(), Value::from(uri::url_of(path)));
        }
        metadata
    }

    /// The properties that say something else than they did in `before`, with what they say now.
    /// `Position` is not among them: MPRIS asks for it to be read, never announced.
    pub(super) fn changed_since(&self, before: &Now) -> HashMap<&'static str, OwnedValue> {
        let mut changed = HashMap::new();
        let metadata = self.metadata();
        if metadata != before.metadata() {
            changed.insert("Metadata", OwnedValue::from(metadata));
        }
        if self.status != before.status {
            changed.insert("PlaybackStatus", Str::from(self.status.word()).into());
        }
        if self.volume != before.volume {
            changed.insert("Volume", self.volume.into());
        }
        if self.shuffle != before.shuffle {
            changed.insert("Shuffle", self.shuffle.into());
        }
        if self.loop_status != before.loop_status {
            changed.insert("LoopStatus", Str::from(self.loop_status.word()).into());
        }
        if self.can_next != before.can_next {
            changed.insert("CanGoNext", self.can_next.into());
        }
        if self.can_previous != before.can_previous {
            changed.insert("CanGoPrevious", self.can_previous.into());
        }
        changed
    }
}

/// The path MPRIS tells clients to use while no track is loaded.
fn no_track() -> ObjectPath<'static> {
    // A literal in this program, so the bus has no reason to refuse it.
    ObjectPath::from_static_str_unchecked(NO_TRACK)
}

/// `duration` in the microseconds MPRIS counts in.
pub(super) fn micros(duration: Duration) -> i64 {
    i64::try_from(duration.as_micros()).unwrap_or(i64::MAX)
}

/// The time `micros` microseconds stand for. A client that asks for a place before the start of
/// the track is asking for the start of it.
pub(super) fn duration(micros: i64) -> Duration {
    Duration::from_micros(u64::try_from(micros).unwrap_or(0))
}
