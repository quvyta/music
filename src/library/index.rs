//! The index: what the last scan found, written down so the next one does not have to read every
//! file again.
//!
//! The file is written through a temporary name and a rename, so a crash halfway through leaves
//! the old index whole or no index at all, never half of one. An index that cannot be read, that
//! is cut short, or that was written in another shape is thrown away without a word: the person is
//! never shown an error about a cache file, only a library that takes a moment longer to fill.
//!
//! The shape is plain text: a first line naming the version, then one line per file, its fields
//! separated by tabs. A path with spaces in it therefore needs nothing said about it, and the two
//! characters a line cannot carry, a tab and a newline, are written as `\t` and `\n` so that a
//! title holding either comes back whole. A file whose path is not text is left out of the index
//! and read again next time: a path written with its bytes turned into marks would name another
//! file.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use crate::library::Track;

/// The line every index begins with, naming the shape of what follows it. A change to the fields a
/// line holds changes this number, and a file that does not begin with this line is thrown away
/// rather than misread as one of these.
const FORMAT: &str = "qmus-library-index 1";

/// One file: where it is, how big it is, when it was last written, and what its tags said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    /// Where the file is.
    path: String,
    /// The title from its tags, or its file name without the ending.
    title: String,
    /// The artist from its tags; empty when the tags name none.
    artist: String,
    /// The album from its tags, or the name of its folder.
    album: String,
    /// How big the file is, which moves whenever it is written to.
    size: u64,
    /// When it was last written.
    modified: Stamp,
    /// Its number on the album.
    number: Option<u32>,
    /// How long it plays, when the file said so.
    duration: Option<Duration>,
    /// Whether qmus can play it yet.
    playable: bool,
}

/// When a file was last written, in seconds and nanoseconds since 1970.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    /// Whole seconds since 1970.
    secs: u64,
    /// The rest of the second.
    nanos: u32,
}

impl Entry {
    /// The entry for `track` together with what the file on disk says about itself, or `None` when
    /// the file cannot be asked about or its path cannot be written down as text.
    pub(crate) fn of(track: &Track) -> Option<Self> {
        let file = track.file()?;
        let path = file.to_str()?;
        let meta = fs::metadata(file).ok()?;
        Some(Self {
            path: path.to_owned(),
            title: track.title.clone(),
            artist: track.artist.clone(),
            album: track.album.clone(),
            size: meta.len(),
            modified: Stamp::of(&meta),
            number: track.number,
            duration: track.duration,
            playable: track.playable,
        })
    }

    /// Where the file is.
    pub(crate) fn path(&self) -> &Path {
        Path::new(&self.path)
    }

    /// Whether the file on disk is still the one this entry describes, which is all of what decides
    /// whether its tags are read again.
    pub(crate) fn current(&self) -> bool {
        fs::metadata(self.path()).is_ok_and(|meta| meta.len() == self.size && Stamp::of(&meta) == self.modified)
    }

    /// The track this entry describes, named by the file it was read from.
    pub(crate) fn to_track(&self) -> Track {
        Track {
            location: PathBuf::from(&self.path).into(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            number: self.number,
            duration: self.duration,
            playable: self.playable,
        }
    }

    /// This entry as a line of the index: its fields with a tab between them.
    fn to_line(&self) -> String {
        [
            escape(&self.path),
            self.size.to_string(),
            self.modified.secs.to_string(),
            self.modified.nanos.to_string(),
            escape(&self.title),
            escape(&self.artist),
            escape(&self.album),
            self.number.map_or_else(String::new, |number| number.to_string()),
            self.duration.map_or_else(String::new, |length| length.as_nanos().to_string()),
            if self.playable { "1".to_owned() } else { "0".to_owned() },
        ]
        .join("\t")
    }

    /// The entry `line` holds, or `None` when the line does not hold one: the wrong number of
    /// fields, or a field that is not what it stands for.
    fn of_line(line: &str) -> Option<Self> {
        let fields: Vec<&str> = line.split('\t').collect();
        let [path, size, secs, nanos, title, artist, album, number, length, playable] = fields.as_slice() else {
            return None;
        };
        Some(Self {
            path: unescape(path),
            title: unescape(title),
            artist: unescape(artist),
            album: unescape(album),
            size: size.parse().ok()?,
            modified: Stamp { secs: secs.parse().ok()?, nanos: nanos.parse().ok()? },
            number: number.parse().ok(),
            duration: length.parse().ok().map(Duration::from_nanos),
            playable: match *playable {
                "1" => true,
                "0" => false,
                _ => return None,
            },
        })
    }
}

impl Stamp {
    /// When the file behind `meta` was last written. A file stamped before 1970 is read as the
    /// epoch rather than dropped, because dropping it would make that file look new every time.
    fn of(meta: &fs::Metadata) -> Self {
        let Ok(modified) = meta.modified() else { return Self { secs: 0, nanos: 0 } };
        match modified.duration_since(UNIX_EPOCH) {
            Ok(since) => Self { secs: since.as_secs(), nanos: since.subsec_nanos() },
            Err(_) => Self { secs: 0, nanos: 0 },
        }
    }
}

/// The entries of the index at `path`, in the order they stand in the file, which is the order of
/// the library. Nothing here fails: an index that is missing, cut short, nonsense or of another
/// shape is no index at all, and the walk that follows reads every file for itself.
pub(crate) fn read(path: &Path) -> Vec<Entry> {
    let Ok(text) = fs::read_to_string(path) else { return Vec::new() };
    let mut lines = text.lines();
    if lines.next() != Some(FORMAT) {
        return Vec::new();
    }
    let mut entries = Vec::new();
    for line in lines {
        // One line that does not add up means the file is not an index of this shape, be it cut
        // short in the writing or something else altogether, so all of it is thrown away rather
        // than read as a library with a hole in it.
        let Some(entry) = Entry::of_line(line) else { return Vec::new() };
        entries.push(entry);
    }
    entries
}

/// Writes `entries` to the index at `path` through a temporary name and a rename, so that a crash
/// never leaves half an index where a whole one was. A path that cannot be written to is passed
/// over: the index is a cache, and a person without one still has their music.
pub(crate) fn write<'a>(path: &Path, entries: impl IntoIterator<Item = &'a Entry>) {
    let mut text = String::from(FORMAT);
    for entry in entries {
        text.push('\n');
        text.push_str(&entry.to_line());
    }
    // The state folder does not exist before the first run that keeps something there.
    if let Some(folder) = path.parent() {
        let _ = fs::create_dir_all(folder);
    }
    let _ = qframe::storage::atomic_write(path, text.as_bytes());
}

/// `text` with what a line cannot carry written out: a tab would be read as the field beside it and
/// a newline as the line after it, and a backslash as the start of either.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for letter in text.chars() {
        match letter {
            '\\' => out.push_str(r"\\"),
            '\t' => out.push_str(r"\t"),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            other => out.push(other),
        }
    }
    out
}

/// What [`escape`] wrote, read back as it was.
pub(crate) fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut letters = text.chars();
    while let Some(letter) = letters.next() {
        if letter != '\\' {
            out.push(letter);
            continue;
        }
        match letters.next() {
            Some('\\') => out.push('\\'),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            // A backslash in front of anything else is only that thing: a name holding one is
            // written as a backslash and a backslash.
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Scratch, sine_wav};

    /// A track of a file in `scratch` at `relative`, saying everything an entry can hold: a title
    /// with a tab and a newline in it, a path with a space and letters outside ASCII.
    fn track_of(scratch: &Scratch, relative: &str) -> Track {
        let path = scratch.path(relative);
        sine_wav(&path, 8_000, 1, 0.1, 440.0);
        Track {
            location: path.into(),
            title: "Gülpembe\tve ikinci satır\nburada".to_owned(),
            artist: "Barış Manço".to_owned(),
            album: "Sahibinden İhtiyaçtan".to_owned(),
            number: Some(7),
            // A length with a fraction of a millisecond in it, which a file whose length is counted in
            // samples says.
            duration: Some(Duration::from_nanos(1_234_567_890)),
            playable: true,
        }
    }

    #[test]
    fn an_entry_written_down_comes_back_as_the_track_it_was_made_from() {
        let scratch = Scratch::new("index-roundtrip");
        let track = track_of(&scratch, "Müzik/Bir Derdim Var.wav");
        let index = scratch.path("cache/library.index");
        let entry = Entry::of(&track).expect("entry");
        write(&index, [&entry]);
        assert_eq!(read(&index), [entry], "every field of the entry survives the round trip");
        assert_eq!(
            Some(read(&index)[0].path()),
            track.file(),
            "a path with spaces and letters outside ASCII stays itself"
        );
        assert_eq!(read(&index)[0].to_track(), track, "and the track is the one that went in");
    }

    #[test]
    fn a_path_with_the_characters_a_line_cannot_carry_survives_too() {
        let scratch = Scratch::new("index-odd-path");
        // A tab and a newline in a file name are legal, and a backslash before them must not be
        // read as the escape of something else.
        let track = track_of(&scratch, "müzik/bir\\tek ve\ttab\nsatır.wav");
        let index = scratch.path("cache/library.index");
        write(&index, std::slice::from_ref(&Entry::of(&track).expect("entry")));
        assert_eq!(read(&index).first().map(Entry::path), track.file());
    }

    #[test]
    fn a_track_without_a_number_or_a_length_survives_too() {
        let scratch = Scratch::new("index-bare");
        let path = scratch.path("music/acik.wav");
        sine_wav(&path, 8_000, 1, 0.1, 440.0);
        let track = Track {
            location: path.into(),
            title: "Açık".to_owned(),
            artist: String::new(),
            album: "music".to_owned(),
            number: None,
            duration: None,
            playable: false,
        };
        let index = scratch.path("cache/library.index");
        write(&index, std::iter::once(&Entry::of(&track).expect("entry")));
        assert_eq!(read(&index).first().map(Entry::to_track), Some(track));
    }

    #[test]
    fn an_index_that_is_missing_nonsense_cut_short_or_of_another_shape_holds_nothing() {
        let scratch = Scratch::new("index-broken");
        let index = scratch.path("cache/library.index");
        assert!(read(&index).is_empty(), "no index is no index");

        std::fs::write(&index, "this is not an index ][").expect("file");
        assert!(read(&index).is_empty(), "nonsense is no index");

        std::fs::write(&index, "qmus-library-index 9\n/music/a.wav\t10\t1\t2\tA\t\tmusic\t1\t1000\t1\n").expect("file");
        assert!(read(&index).is_empty(), "another shape's index is not read as this one's");

        let track = track_of(&scratch, "music/a.wav");
        let entry = Entry::of(&track).expect("entry");
        write(&index, std::iter::once(&entry));
        assert_eq!(read(&index).first(), Some(&entry));
        let text = std::fs::read_to_string(&index).expect("index");
        let cut = text.find('\n').expect("a line");
        std::fs::write(&index, &text[..cut]).expect("file");
        assert!(read(&index).is_empty(), "an index with no tracks in it is an index of nothing: {text:?}");

        write(&index, std::iter::once(&entry));
        let text = std::fs::read_to_string(&index).expect("index");
        // Cut inside the line of a file, which is what a crash in the writing leaves behind.
        std::fs::write(&index, &text[..text.len() - 12]).expect("file");
        assert!(read(&index).is_empty(), "half an index is no index");
    }

    #[test]
    fn an_index_is_written_into_a_folder_that_was_not_there_yet() {
        let scratch = Scratch::new("index-new-folder");
        let track = track_of(&scratch, "music/a.wav");
        let entry = Entry::of(&track).expect("entry");
        let index = scratch.path("state").join("quvyta").join("music").join("library.index");
        write(&index, [&entry]);
        assert_eq!(read(&index), [entry]);
    }

    #[test]
    fn an_index_that_cannot_be_written_is_passed_over_and_leaves_nothing_behind() {
        let scratch = Scratch::new("index-unwritable");
        let track = track_of(&scratch, "music/a.wav");
        let entry = Entry::of(&track).expect("entry");

        let written = scratch.path("cache/library.index");
        write(&written, [&entry]);
        assert_eq!(std::fs::read_dir(scratch.path("cache")).expect("folder").count(), 1, "only the index itself");

        // The path of the index is a folder here, so nothing can be written there at all.
        let folder = scratch.path("not-a-file");
        std::fs::create_dir(&folder).expect("folder");
        write(&folder, [&entry]);
        assert!(read(&folder).is_empty(), "an index that could not be written is no index");
        assert_eq!(std::fs::read_dir(&folder).expect("folder").count(), 0, "and no temporary file is left behind");
    }
}
