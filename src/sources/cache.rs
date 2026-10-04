//! What each account listed the last time, kept in qmus's own cache so the next start shows it at
//! once while the account is asked again.
//!
//! The shape is the library index's: a first line naming the version, then one track a line with
//! its fields separated by tabs, a tab or a newline inside a field written as `\t` or `\n`. A file
//! that is missing, cut short or of another shape holds nothing; it is a cache, and the account is
//! asked again whatever it says.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::RemoteTrack;
use crate::library::index::{escape, unescape};

/// The line every listing begins with.
const FORMAT: &str = "qmus-source-listing 1";

/// Where the listing of the account `key` is kept under `folder`.
#[must_use]
pub fn file_of(folder: &Path, key: &str) -> PathBuf {
    // An account's key is made of letters, digits and dashes, but a name written by hand into the
    // accounts file is kept from reaching outside the folder all the same.
    let safe: String =
        key.chars().map(|letter| if letter.is_ascii_alphanumeric() || letter == '-' { letter } else { '_' }).collect();
    folder.join(format!("{safe}.listing"))
}

/// The tracks the listing at `file` holds, in its order; none when it cannot be read whole.
#[must_use]
pub fn read(file: &Path) -> Vec<RemoteTrack> {
    let Ok(text) = fs::read_to_string(file) else { return Vec::new() };
    let mut lines = text.lines();
    if lines.next() != Some(FORMAT) {
        return Vec::new();
    }
    let mut tracks = Vec::new();
    for line in lines {
        let Some(track) = track_of(line) else { return Vec::new() };
        tracks.push(track);
    }
    tracks
}

/// Keeps `tracks` as the listing at `file`, whole or not at all. A listing that cannot be written
/// is passed over: the account is asked again next time.
pub fn write(file: &Path, tracks: &[RemoteTrack]) {
    if let Some(folder) = file.parent() {
        let _ = fs::create_dir_all(folder);
    }
    let mut text = String::from(FORMAT);
    for track in tracks {
        text.push('\n');
        text.push_str(&line_of(track));
    }
    let _ = qframe::storage::atomic_write(file, text.as_bytes());
}

/// Forgets the listing at `file`, as when its account is removed.
pub fn forget(file: &Path) {
    let _ = fs::remove_file(file);
}

/// A track as a line of the listing.
fn line_of(track: &RemoteTrack) -> String {
    [
        escape(&track.id),
        escape(&track.title),
        escape(&track.artist),
        escape(&track.album),
        track.number.map_or_else(String::new, |number| number.to_string()),
        track.duration.map_or_else(String::new, |length| length.as_millis().to_string()),
        track.cover.as_deref().map_or_else(String::new, escape),
    ]
    .join("\t")
}

/// The track a line of the listing holds, or `None` when it holds none.
fn track_of(line: &str) -> Option<RemoteTrack> {
    let fields: Vec<&str> = line.split('\t').collect();
    let [id, title, artist, album, number, length, cover] = fields.as_slice() else { return None };
    if id.is_empty() {
        return None;
    }
    Some(RemoteTrack {
        id: unescape(id),
        title: unescape(title),
        artist: unescape(artist),
        album: unescape(album),
        number: if number.is_empty() { None } else { Some(number.parse().ok()?) },
        duration: if length.is_empty() { None } else { Some(Duration::from_millis(length.parse().ok()?)) },
        cover: (!cover.is_empty()).then(|| unescape(cover)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Scratch;

    fn track(id: &str, title: &str) -> RemoteTrack {
        RemoteTrack {
            id: id.to_owned(),
            title: title.to_owned(),
            artist: "Barış Manço".to_owned(),
            album: "Sahibinden İhtiyaçtan".to_owned(),
            number: Some(7),
            duration: Some(Duration::from_secs(187)),
            cover: Some("al-1".to_owned()),
        }
    }

    #[test]
    fn a_listing_kept_comes_back_as_it_was_whatever_its_titles_hold() {
        let scratch = Scratch::new("listing-roundtrip");
        let file = file_of(&scratch.path("cache/sources"), "subsonic-1a2b");
        let tracks = [
            track("1", "Gülpembe\tve ikinci satır\nburada \\t değil"),
            RemoteTrack {
                number: None, duration: None, cover: None, artist: String::new(), ..track("tr 2", "Yalnız")
            },
        ];
        write(&file, &tracks);
        assert_eq!(read(&file), tracks);
    }

    #[test]
    fn a_listing_missing_cut_short_or_of_another_shape_holds_nothing() {
        let scratch = Scratch::new("listing-broken");
        let file = file_of(&scratch.path("cache/sources"), "subsonic-1a2b");
        assert!(read(&file).is_empty());
        write(&file, &[track("1", "Bir"), track("2", "İki")]);
        let text = fs::read_to_string(&file).expect("written");
        fs::write(&file, &text[..text.len() - 5]).expect("cut");
        assert!(read(&file).is_empty(), "half a listing is none");
        fs::write(&file, text.replace(FORMAT, "qmus-source-listing 9")).expect("other");
        assert!(read(&file).is_empty(), "another shape is none");
    }

    #[test]
    fn a_key_never_leads_the_listing_outside_its_folder_and_a_forgotten_listing_is_gone() {
        let scratch = Scratch::new("listing-key");
        let folder = scratch.path("cache/sources");
        let file = file_of(&folder, "../../escape");
        assert_eq!(file.parent(), Some(folder.as_path()), "{}", file.display());
        write(&file, &[track("1", "Bir")]);
        forget(&file);
        assert!(!file.exists());
    }
}
