//! The playlists: the files in a folder that list music, read and written as the standard M3U8 so
//! that another player can read them too.
//!
//! A playlist names its tracks by paths relative to its own folder wherever it can, which is what
//! makes it portable: the folder can be carried anywhere and the tracks are still found. A track
//! that is not there any more stays in the list as an item that is not present, because a
//! playlist is what the person chose rather than a scan of what happens to exist.
//!
//! Nothing here touches the music itself. Only list files are written, renamed and removed, and
//! [`delete`] refuses anything that is not a playlist in the playlists folder.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

mod read;
mod write;

pub use read::read;
pub use write::{delete, rename, replace, write};

/// The endings of the files qmus reads as a playlist, in either case.
const ENDINGS: [&str; 2] = ["m3u8", "m3u"];

/// One playlist file found in the playlists folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The playlist's name: its file name without the ending.
    pub name: String,
    /// Where the file is.
    pub path: PathBuf,
}

/// One playlist: what it is called, where it is, and the items it lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    /// The playlist's name: its file name without the ending.
    pub name: String,
    /// Where the file is.
    pub path: PathBuf,
    /// Its items, in the order of the file.
    pub items: Vec<Item>,
}

/// One line of a playlist: where a track is, and what the file says about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Where the track is: a file, a path from the playlist's folder, or an `http(s)` address
    /// kept as it is written.
    pub path: PathBuf,
    /// The title from `#EXTINF`, if the file gives one.
    pub title: Option<String>,
    /// The length from `#EXTINF`, if the file gives a readable one.
    pub duration: Option<Duration>,
    /// Whether a file is there now. An `http(s)` item is never present.
    pub present: bool,
}

/// One track to write into a playlist: where it is, and what to say about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// Where the track is.
    pub path: PathBuf,
    /// The title to write in `#EXTINF`; none writes no title.
    pub title: Option<String>,
    /// The length to write in `#EXTINF`, in whole seconds; none writes `-1`.
    pub duration: Option<Duration>,
}

/// Every playlist file in `folder`, ordered by name without regard to case. A folder that is not
/// there, or that cannot be read, holds no playlists: an empty list is what a screen draws when
/// the person has none, and a folder that is not there yet is the usual way to begin.
#[must_use]
pub fn list(folder: &Path) -> Vec<Entry> {
    let Ok(read) = fs::read_dir(folder) else { return Vec::new() };
    let mut playlists: Vec<Entry> = read
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_playlist(path))
        .map(|path| Entry { name: name_of(&path), path })
        .collect();
    playlists.sort_by_key(|entry| entry.name.to_lowercase());
    playlists
}

/// Whether `path` is a playlist file: its ending, whatever its case, says it is. This is what
/// tells a list file from a sound file, and it is the check that keeps the person's music out of
/// everything that removes or writes.
fn is_playlist(path: &Path) -> bool {
    path.extension()
        .and_then(|ending| ending.to_str())
        .is_some_and(|ending| ENDINGS.iter().any(|known| known.eq_ignore_ascii_case(ending)))
}

/// A playlist's name: its file name without the ending, so `Evening.m3u8` is `Evening`.
fn name_of(path: &Path) -> String {
    path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::testing::Scratch;

    #[test]
    fn playlists_are_listed_by_name_without_regard_to_case_and_nothing_else_is() {
        let scratch = Scratch::new("list-order");
        let folder = scratch.path("playlists");
        write(&folder, "Zebra", &[]).expect("playlist");
        write(&folder, "apple", &[]).expect("playlist");
        write(&folder, "Mango", &[]).expect("playlist");
        fs::write(scratch.path("playlists/Berry.M3U"), "#EXTM3U\n").expect("file");
        fs::write(scratch.path("playlists/notes.txt"), "words").expect("file");
        fs::create_dir_all(scratch.path("playlists/Deep.m3u8")).expect("folder");
        let entries = list(&folder);
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["apple", "Berry", "Mango", "Zebra"]);
        assert_eq!(entries[0].path, folder.join("apple.m3u8"));
    }

    #[test]
    fn a_playlists_folder_that_is_not_there_holds_no_playlists() {
        let scratch = Scratch::new("list-missing");
        assert!(list(&scratch.path("nowhere")).is_empty());
    }
}
