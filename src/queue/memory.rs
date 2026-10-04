//! Writing the queue to a file and reading it back, so the next run comes to the same track and
//! the same moment.
//!
//! The file is a small text: one setting a line, the tracks one a line, and the order they play in
//! as a single line of places. It is written whole through a temporary file that is then renamed
//! over the old one, so a run that stops halfway leaves the queue that was there before it.

use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::library::{Location, SourceKey};
use std::time::Duration;

use qframe::storage::atomic_write;

use super::{Queue, Repeat};

/// The first line of the file, naming the shape of what follows it.
const HEADER: &str = "qmus queue 1";

/// A second in nanoseconds: a file that claims more of a second than this in its fraction is not
/// one this wrote.
const SECOND_IN_NANOS: u32 = 1_000_000_000;

/// What a file of the queue said, before the tracks in it are looked for on disk.
struct Written {
    /// What the queue does when the last track has been heard.
    repeat: Repeat,
    /// Whether the tracks after the one heard are in a shuffled order.
    shuffled: bool,
    /// The place in the play order of the track heard.
    cursor: usize,
    /// A track that had left the queue but was still being heard.
    still_playing: Option<Location>,
    /// The tracks, in the order they were given.
    order: Vec<Location>,
    /// The place of each track of the list in the order they play.
    play: Vec<usize>,
    /// How far the track heard has got.
    position: Duration,
}

impl Queue {
    /// Writes the queue and how far the track heard has got, so that the next run can come back to
    /// the same track and the same moment. The file is put in place whole or not at all.
    ///
    /// # Errors
    ///
    /// Returns the error of the file system when the folder holding the file, or the file itself,
    /// cannot be written.
    pub fn save(&self, file: &Path, position: Duration) -> io::Result<()> {
        // A queue kept in the state folder is the first thing written there, so the folder is
        // made rather than asked for.
        if let Some(folder) = file.parent() {
            fs::create_dir_all(folder)?;
        }
        atomic_write(file, &self.as_bytes(position))
    }

    /// Reads back a queue and how far the track heard has got. A file that is not there, that says
    /// something other than a queue, or that cannot be read gives `None`: a queue qmus cannot make
    /// sense of is better forgotten than guessed at.
    #[must_use]
    pub fn load(file: &Path) -> Option<(Self, Duration)> {
        Written::from_bytes(&fs::read(file).ok()?)?.queue()
    }

    /// The whole file: one setting a line, the tracks one a line, and the play order as a line of
    /// places. A track's name is written as it is, so a name that is not plain text still comes
    /// back as it was.
    fn as_bytes(&self, position: Duration) -> Vec<u8> {
        let mut text = Vec::new();
        line(&mut text, HEADER);
        line(&mut text, format!("repeat {}", word(self.repeat)));
        line(&mut text, if self.shuffled { "shuffle on" } else { "shuffle off" });
        line(&mut text, format!("position {} {}", position.as_secs(), position.subsec_nanos()));
        line(&mut text, format!("at {}", self.cursor));
        if let Some(named) = self.still_playing.as_ref().and_then(written) {
            let mut with_path = b"still ".to_vec();
            with_path.extend_from_slice(&named);
            line(&mut text, with_path);
        }
        line(&mut text, format!("order {}", self.order.len()));
        for track in &self.order {
            // A track that cannot be written takes an empty line: it names no file, so reading the
            // queue back drops it the way it drops a file that has gone, and the places stay right.
            line(&mut text, written(track).unwrap_or_default());
        }
        let mut play = b"play".to_vec();
        for place in &self.play {
            play.push(b' ');
            play.extend_from_slice(place.to_string().as_bytes());
        }
        line(&mut text, play);
        text
    }
}

impl Written {
    /// Reads what the file said, asking for every line it is made of.
    fn from_bytes(raw: &[u8]) -> Option<Self> {
        let lines: Vec<&[u8]> = raw.split(|byte| *byte == b'\n').map(without_carriage_return).collect();
        if *lines.first()? != HEADER.as_bytes() {
            return None;
        }
        let mut repeat = None;
        let mut shuffled = None;
        let mut cursor = None;
        let mut still_playing = None;
        let mut order = None;
        let mut play = None;
        let mut position = None;
        let mut at = 1;
        while at < lines.len() {
            let whole = lines[at];
            at += 1;
            if whole.is_empty() {
                continue;
            }
            let (key, rest) = match whole.iter().position(|byte| *byte == b' ') {
                Some(space) => whole.split_at(space),
                // A line with nothing after its key, such as a play order with no track in it.
                None => (whole, &b""[..]),
            };
            let value = rest.strip_prefix(b" ").unwrap_or(rest);
            match key {
                b"repeat" => repeat = Some(mode(value)?),
                b"shuffle" => shuffled = Some(setting(value)?),
                b"position" => position = Some(moment(value)?),
                b"at" => cursor = Some(number(value)?),
                b"still" => still_playing = Some(track(value)),
                b"order" => {
                    let mut taken = Vec::new();
                    for _ in 0..number(value)? {
                        taken.push(track(lines.get(at)?));
                        at += 1;
                    }
                    order = Some(taken);
                }
                b"play" => play = Some(places(value)?),
                _ => return None,
            }
        }
        Some(Self {
            repeat: repeat?,
            shuffled: shuffled?,
            cursor: cursor?,
            still_playing,
            order: order?,
            play: play?,
            position: position?,
        })
    }

    /// The queue the file described, with how far the track heard has got, leaving out the tracks
    /// that are no longer on disk.
    ///
    /// A track that is gone takes its place with it: the queue moves on to the next track that is
    /// there, or to the last one that is, and the moment heard is forgotten with it, because the
    /// moment belongs to the track that was heard.
    fn queue(self) -> Option<(Queue, Duration)> {
        let Written { repeat, shuffled, cursor, still_playing, order, play, position: heard } = self;
        // Every track of the list is played exactly once, and the place heard is a real one; a file
        // that says otherwise is not one this wrote, and guessing where it meant would be worse
        // than forgetting it.
        if play.len() != order.len() || cursor > play.len() {
            return None;
        }
        let mut named = vec![false; order.len()];
        for place in &play {
            let seen = named.get_mut(*place)?;
            if std::mem::replace(seen, true) {
                return None;
            }
        }
        let still_playing = still_playing.filter(present);
        let mut kept = Vec::new();
        let mut place_of = vec![None; order.len()];
        for (was, held) in order.into_iter().enumerate() {
            if present(&held) {
                place_of[was] = Some(kept.len());
                kept.push(held);
            }
        }
        // Taking the tracks that are gone out of the play order moves every place after them along,
        // so where each of them ends up is counted as the order is walked.
        let mut where_now: Vec<Option<usize>> = Vec::new();
        let mut kept_play = Vec::new();
        for &held in &play {
            match place_of[held] {
                Some(where_in_list) => {
                    where_now.push(Some(kept_play.len()));
                    kept_play.push(where_in_list);
                }
                None => where_now.push(None),
            }
        }
        let play = kept_play;
        let was_heard = cursor.min(where_now.len());
        // A track still heard after leaving the queue is the one the moment belongs to, and the
        // queue has already moved past it.
        let (cursor, position) = if still_playing.is_some() {
            (was_heard.min(play.len()), heard)
        } else {
            match where_now.get(was_heard).copied().flatten() {
                Some(place) => (place, heard),
                None => {
                    let (earlier, later) = where_now.split_at(was_heard);
                    let next = later.iter().skip(1).chain(earlier).flatten().next().copied();
                    (next.unwrap_or(play.len()), Duration::ZERO)
                }
            }
        };
        Some((Queue { order: kept, play, cursor, still_playing, shuffled, repeat }, position))
    }
}

/// Appends one line of the file and the newline that ends it.
fn line(text: &mut Vec<u8>, bytes: impl AsRef<[u8]>) {
    text.extend_from_slice(bytes.as_ref());
    text.push(b'\n');
}

/// A line of the file without the carriage return that a text written elsewhere ends it with.
fn without_carriage_return(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// The word a line starts with when it names a track of an account rather than a file.
const REMOTE: &[u8] = b"remote\t";

/// The line naming `track`: a file's path as it is, or an account's track as `remote`, its
/// account's key and its id, set apart by tabs. Nothing for an account's track whose key or id
/// holds a tab or a line's end, which the line could not carry.
fn written(track: &Location) -> Option<Vec<u8>> {
    match track {
        Location::File(path) => Some(path.as_os_str().as_bytes().to_vec()),
        Location::Remote { source, id } => {
            let plain = |text: &str| !text.contains(['\t', '\n', '\r']);
            (plain(&source.0) && plain(id)).then(|| [REMOTE, source.0.as_bytes(), b"\t", id.as_bytes()].concat())
        }
    }
}

/// The track a line of the file names: an account's track when the line says so and holds
/// exactly its three fields, a file's path otherwise.
fn track(bytes: &[u8]) -> Location {
    if bytes.starts_with(REMOTE) {
        let fields: Vec<&[u8]> = bytes.split(|byte| *byte == b'\t').collect();
        if let [_, source, id] = fields.as_slice()
            && let (Ok(source), Ok(id)) = (std::str::from_utf8(source), std::str::from_utf8(id))
        {
            return Location::Remote { source: SourceKey(source.to_owned()), id: id.to_owned() };
        }
    }
    Location::File(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

/// Whether a track read back is still there to play: a file that has gone is not, and whether an
/// account's track still exists is for its account to say, so it is kept.
fn present(track: &Location) -> bool {
    match track {
        Location::File(path) => !path.as_os_str().is_empty() && path.exists(),
        Location::Remote { .. } => true,
    }
}

/// The number a setting of the file gives.
fn number(value: &[u8]) -> Option<usize> {
    std::str::from_utf8(value).ok()?.parse().ok()
}

/// The play order a line of the file gives: the place of every track of the list.
fn places(value: &[u8]) -> Option<Vec<usize>> {
    let text = std::str::from_utf8(value).ok()?;
    text.split(' ').filter(|word| !word.is_empty()).map(str::parse).collect::<Result<Vec<usize>, _>>().ok()
}

/// How far the track heard has got: whole seconds, then the rest of a second in nanoseconds.
fn moment(value: &[u8]) -> Option<Duration> {
    let text = std::str::from_utf8(value).ok()?;
    let mut words = text.split(' ').filter(|word| !word.is_empty());
    let seconds: u64 = words.next()?.parse().ok()?;
    let nanoseconds: u32 = words.next()?.parse().ok()?;
    if words.next().is_some() || nanoseconds >= SECOND_IN_NANOS {
        return None;
    }
    Some(Duration::new(seconds, nanoseconds))
}

/// Whether the file says the tracks are shuffled.
fn setting(value: &[u8]) -> Option<bool> {
    match value {
        b"on" => Some(true),
        b"off" => Some(false),
        _ => None,
    }
}

/// The word the file gives this mode.
fn word(repeat: Repeat) -> &'static str {
    match repeat {
        Repeat::Off => "off",
        Repeat::All => "all",
        Repeat::One => "one",
    }
}

/// The mode a word of the file names.
fn mode(value: &[u8]) -> Option<Repeat> {
    match value {
        b"off" => Some(Repeat::Off),
        b"all" => Some(Repeat::All),
        b"one" => Some(Repeat::One),
        _ => None,
    }
}
