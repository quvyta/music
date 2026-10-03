//! The music in a folder: every sound file under it, with the names its tags give.
//!
//! qmus only reads here. It never writes to, renames or deletes a file of the person's music.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use lofty::prelude::*;

/// The endings of the files qmus plays.
const PLAYED: [&str; 10] = ["flac", "mp3", "ogg", "oga", "m4a", "mp4", "aac", "wav", "aiff", "aif"];

/// The endings of music files qmus lists but cannot play yet: they are shown faint rather than
/// hidden, so nobody wonders where their music went.
const LATER: [&str; 1] = ["opus"];

/// The most threads that read files at once.
const READERS: usize = 8;

/// One track of the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// Where the file is.
    pub path: PathBuf,
    /// The title from its tags, or its file name without the ending.
    pub title: String,
    /// The artist from its tags; empty when the tags name none.
    pub artist: String,
    /// The album from its tags, or the name of its folder.
    pub album: String,
    /// Its number on the album.
    pub number: Option<u32>,
    /// How long it plays, when the file says.
    pub duration: Option<Duration>,
    /// Whether qmus can play it yet; an Opus file cannot until qmus decodes Opus.
    pub playable: bool,
}

/// Every track under `folder`, in the order of artist, album, number and title. Folders that
/// cannot be read are passed over, and a folder reached twice through links is read once.
#[must_use]
pub fn scan(folder: &Path) -> Vec<Track> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let mut folders = vec![folder.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(real) = fs::canonicalize(&folder) else { continue };
        if !seen.insert(real) {
            continue;
        }
        let Ok(entries) = fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if let Some(playable) = kind(&path) {
                found.push((path, playable));
            }
        }
    }
    let mut tracks = read_all(found);
    tracks.sort_by_cached_key(order_key);
    tracks
}

/// The tracks of the files `found`, read on a few threads at once: most of the time goes into
/// waiting for the disk, and several files are waited for together.
fn read_all(found: Vec<(PathBuf, bool)>) -> Vec<Track> {
    let workers = std::thread::available_parallelism().map_or(1, std::num::NonZero::get).clamp(1, READERS);
    // Too few files are read faster than threads are started.
    if workers == 1 || found.len() < 64 {
        return found.into_iter().map(|(path, playable)| Track { playable, ..read(path) }).collect();
    }
    let share = found.len().div_ceil(workers);
    let mut shares: Vec<Vec<(PathBuf, bool)>> = Vec::with_capacity(workers);
    let mut rest = found.into_iter();
    for _ in 0..workers {
        shares.push(rest.by_ref().take(share).collect());
    }
    std::thread::scope(|scope| {
        let readers: Vec<_> = shares
            .into_iter()
            .map(|share| {
                scope.spawn(move || {
                    share.into_iter().map(|(path, playable)| Track { playable, ..read(path) }).collect::<Vec<_>>()
                })
            })
            .collect();
        // A reader that panicked on a strange file loses its share rather than the whole library.
        readers.into_iter().filter_map(|reader| reader.join().ok()).flatten().collect()
    })
}

/// Whether `path` ends the way a music file does: `Some(true)` for one qmus plays,
/// `Some(false)` for one it lists but cannot play yet, `None` for anything else.
fn kind(path: &Path) -> Option<bool> {
    let ending = path.extension()?.to_str()?;
    let among = |endings: &[&str]| endings.iter().any(|known| known.eq_ignore_ascii_case(ending));
    if among(&PLAYED) {
        Some(true)
    } else if among(&LATER) {
        Some(false)
    } else {
        None
    }
}

/// The track at `path`, named by its tags where they say and by its file and folder where not.
fn read(path: PathBuf) -> Track {
    let stem = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
    let folder = path
        .parent()
        .and_then(|parent| parent.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut track =
        Track { title: stem, artist: String::new(), album: folder, number: None, duration: None, playable: true, path };
    let Ok(file) = lofty::read_from_path(&track.path) else { return track };
    let duration = file.properties().duration();
    track.duration = (!duration.is_zero()).then_some(duration);
    if let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) {
        let given = |text: Option<std::borrow::Cow<'_, str>>| {
            text.map(|text| text.trim().to_owned()).filter(|text| !text.is_empty())
        };
        if let Some(title) = given(tag.title()) {
            track.title = title;
        }
        if let Some(artist) = given(tag.artist()) {
            track.artist = artist;
        }
        if let Some(album) = given(tag.album()) {
            track.album = album;
        }
        track.number = tag.track();
    }
    track
}

/// What tracks are ordered by: names without regard to case, then the number on the album.
fn order_key(track: &Track) -> (String, String, u32, String) {
    (
        track.artist.to_lowercase(),
        track.album.to_lowercase(),
        track.number.unwrap_or(u32::MAX),
        track.title.to_lowercase(),
    )
}

/// An album of the library: its tracks, in the order listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Album {
    /// Its title.
    pub title: String,
    /// The artist its tracks name; empty when they name none.
    pub artist: String,
    /// The rows of its tracks in the list of tracks.
    pub tracks: Vec<usize>,
}

/// An artist of the library: how many albums and which tracks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artist {
    /// The name.
    pub name: String,
    /// How many albums the tracks come from.
    pub albums: usize,
    /// The rows of the artist's tracks in the list of tracks.
    pub tracks: Vec<usize>,
}

/// The albums of `tracks`, which are in the library's order, so an album's tracks stand together.
#[must_use]
pub fn albums(tracks: &[Track]) -> Vec<Album> {
    let mut albums: Vec<Album> = Vec::new();
    for (row, track) in tracks.iter().enumerate() {
        match albums.last_mut() {
            Some(album) if album.title == track.album && album.artist == track.artist => album.tracks.push(row),
            _ => albums.push(Album { title: track.album.clone(), artist: track.artist.clone(), tracks: vec![row] }),
        }
    }
    albums
}

/// The artists of `tracks`, in the library's order.
#[must_use]
pub fn artists(tracks: &[Track]) -> Vec<Artist> {
    let mut artists: Vec<Artist> = Vec::new();
    for (row, track) in tracks.iter().enumerate() {
        let new_album = row == 0 || tracks[row - 1].album != track.album || tracks[row - 1].artist != track.artist;
        match artists.last_mut() {
            Some(artist) if artist.name == track.artist => {
                artist.tracks.push(row);
                artist.albums += usize::from(new_album);
            }
            _ => artists.push(Artist { name: track.artist.clone(), albums: 1, tracks: vec![row] }),
        }
    }
    artists
}

/// `text` as a search compares it: lower case, Turkish dotted and dotless i alike, and the marks
/// of accented Latin letters left off, so "bjork" finds "Björk" and "sezen aksu" "SEZEN AKSU".
#[must_use]
pub fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|letter| match letter {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
            'ç' | 'ć' | 'č' => 'c',
            'ď' => 'd',
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' | 'ě' => 'e',
            'ğ' => 'g',
            'ı' | 'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
            'ł' => 'l',
            'ñ' | 'ń' | 'ň' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => 'o',
            'ř' => 'r',
            'ş' | 'ś' | 'š' => 's',
            'ť' => 't',
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => 'u',
            'ý' | 'ÿ' => 'y',
            'ź' | 'ż' | 'ž' => 'z',
            // The dot that lower-casing İ leaves behind.
            '\u{307}' => '\0',
            other => other,
        })
        .filter(|letter| *letter != '\0')
        .collect()
}

/// Whether every word of `query`, folded, is in the track's title, artist or album.
#[must_use]
pub fn matches(track: &Track, query: &str) -> bool {
    found(&format!("{} {} {}", track.title, track.artist, track.album), query)
}

/// Whether every word of `query`, folded, is in `text`.
#[must_use]
pub fn found(text: &str, query: &str) -> bool {
    let haystack = fold(text);
    fold(query).split_whitespace().all(|word| haystack.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Scratch, sine_wav, tagged_wav};

    #[test]
    fn tags_name_the_tracks_and_order_them_by_artist_album_and_number() {
        let scratch = Scratch::new("scan-tags");
        tagged_wav(&scratch.path("music/b.wav"), 0.2, "Second", "Kalben", "Sonsuz", 2);
        tagged_wav(&scratch.path("music/a.wav"), 0.2, "First", "Kalben", "Sonsuz", 1);
        tagged_wav(&scratch.path("music/deep/c.wav"), 0.5, "Alone", "Adamlar", "Eski", 1);
        let tracks = scan(&scratch.path("music"));
        let titles: Vec<&str> = tracks.iter().map(|track| track.title.as_str()).collect();
        assert_eq!(titles, ["Alone", "First", "Second"]);
        assert_eq!(tracks[1].artist, "Kalben");
        assert_eq!(tracks[1].album, "Sonsuz");
        assert_eq!(tracks[1].number, Some(1));
        assert_eq!(tracks[0].duration, Some(Duration::from_millis(500)));
    }

    #[test]
    fn a_large_folder_is_read_whole_with_every_file_named_by_its_own_tags() {
        let scratch = Scratch::new("scan-large");
        // Enough files that they are read on several threads; the opus ones keep their kind.
        for number in 1..=150_u32 {
            let ending = if number % 50 == 0 { "opus" } else { "wav" };
            let path = scratch.path(&format!("music/{}/{number}.{ending}", number % 7));
            if ending == "wav" {
                tagged_wav(&path, 0.01, &format!("Song {number}"), "Kalben", "Sonsuz", number);
            } else {
                std::fs::write(&path, b"not decoded").expect("file");
            }
        }
        let tracks = scan(&scratch.path("music"));
        assert_eq!(tracks.len(), 150);
        let numbers: Vec<Option<u32>> =
            tracks.iter().filter(|track| track.playable).map(|track| track.number).collect();
        let expected: Vec<Option<u32>> = (1..=150).filter(|number| number % 50 != 0).map(Some).collect();
        assert_eq!(numbers, expected, "every tagged file is listed once, by its number");
        assert!(
            tracks
                .iter()
                .filter(|track| track.playable)
                .all(|track| track.title == format!("Song {}", track.number.unwrap_or(0)))
        );
        assert_eq!(tracks.iter().filter(|track| !track.playable).count(), 3);
        assert!(
            tracks
                .iter()
                .filter(|track| !track.playable)
                .all(|track| track.path.extension().is_some_and(|e| e == "opus"))
        );
    }

    #[test]
    fn an_untagged_file_is_named_by_its_file_and_its_folder() {
        let scratch = Scratch::new("scan-plain");
        sine_wav(&scratch.path("music/Demo Album/01 - opening.wav"), 8_000, 1, 0.1, 440.0);
        let tracks = scan(&scratch.path("music"));
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "01 - opening");
        assert_eq!(tracks[0].album, "Demo Album");
        assert_eq!(tracks[0].artist, "");
    }

    #[test]
    fn only_sound_files_are_listed_and_a_link_back_is_read_once() {
        let scratch = Scratch::new("scan-links");
        sine_wav(&scratch.path("music/song.WAV"), 8_000, 1, 0.1, 440.0);
        std::fs::write(scratch.path("music/cover.jpg"), "picture").expect("file");
        std::fs::write(scratch.path("music/notes.txt"), "words").expect("file");
        std::os::unix::fs::symlink(scratch.path("music"), scratch.path("music/again")).expect("link");
        let tracks = scan(&scratch.path("music"));
        assert_eq!(tracks.len(), 1, "{tracks:?}");
    }

    #[test]
    fn opus_files_are_listed_as_not_playable_yet() {
        let scratch = Scratch::new("scan-opus");
        sine_wav(&scratch.path("music/a.wav"), 8_000, 1, 0.1, 440.0);
        std::fs::write(scratch.path("music/b.OPUS"), "opus bytes").expect("file");
        let tracks = scan(&scratch.path("music"));
        let listed: Vec<(&str, bool)> = tracks.iter().map(|track| (track.title.as_str(), track.playable)).collect();
        assert_eq!(listed, [("a", true), ("b", false)]);
    }

    #[test]
    fn a_folder_that_is_not_there_holds_no_music() {
        let scratch = Scratch::new("scan-missing");
        assert!(scan(&scratch.path("nowhere")).is_empty());
    }

    #[test]
    fn tracks_gather_into_albums_and_artists_in_the_order_listed() {
        let scratch = Scratch::new("scan-groups");
        tagged_wav(&scratch.path("music/a/1.wav"), 0.1, "Bir", "Kalben", "Sonsuz", 1);
        tagged_wav(&scratch.path("music/a/2.wav"), 0.1, "Iki", "Kalben", "Sonsuz", 2);
        tagged_wav(&scratch.path("music/b/1.wav"), 0.1, "Uc", "Kalben", "Kalben", 1);
        tagged_wav(&scratch.path("music/c/1.wav"), 0.1, "Dort", "Adamlar", "Eski", 1);
        let tracks = scan(&scratch.path("music"));
        let albums = albums(&tracks);
        let named: Vec<(&str, &str, usize)> =
            albums.iter().map(|album| (album.title.as_str(), album.artist.as_str(), album.tracks.len())).collect();
        assert_eq!(named, [("Eski", "Adamlar", 1), ("Kalben", "Kalben", 1), ("Sonsuz", "Kalben", 2)]);
        let artists = artists(&tracks);
        let named: Vec<(&str, usize, usize)> =
            artists.iter().map(|artist| (artist.name.as_str(), artist.albums, artist.tracks.len())).collect();
        assert_eq!(named, [("Adamlar", 1, 1), ("Kalben", 2, 3)]);
    }

    #[test]
    fn a_search_folds_case_turkish_i_and_accents() {
        assert_eq!(fold("İSTANBUL Işık"), "istanbul isik");
        assert_eq!(fold("Björk Señor Ça"), "bjork senor ca");
        let track = Track {
            path: PathBuf::from("x.flac"),
            title: "Gülpembe".to_owned(),
            artist: "Barış Manço".to_owned(),
            album: "Sahibinden İhtiyaçtan".to_owned(),
            number: None,
            duration: None,
            playable: true,
        };
        assert!(matches(&track, "baris gulpembe"));
        assert!(matches(&track, "IHTIYAC"));
        assert!(matches(&track, ""));
        assert!(!matches(&track, "baris tarkan"));
    }
}
