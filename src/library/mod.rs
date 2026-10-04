//! The music of the person: every sound file under the folders they gave, with the names its tags
//! give.
//!
//! qmus only reads here. It never writes to, renames or deletes a file of the person's music.
//!
//! [`scan`] walks one folder and reads every file in it. [`scan_with_index`] walks as many source
//! folders as the person has and reads through an index, so that the files that have not changed
//! are not opened at all; [`read_index`] hands back what that index holds without touching the
//! music. The index is a file of qmus's own and belongs outside the music folders.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

mod index;
mod tags;
mod walk;

use index::Entry;

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
    scan_all(&[folder.to_path_buf()])
}

/// Every track under `sources`, in the order [`scan`] gives, each file read: a folder named twice
/// or reached twice through links is walked once. For when there is nowhere to keep an index.
#[must_use]
pub fn scan_all(sources: &[PathBuf]) -> Vec<Track> {
    let mut tracks = tags::read_all(walk::files(sources));
    tracks.sort_by_cached_key(order_key);
    tracks
}

/// Every track under `sources`, in the same order [`scan`] gives, read through the index at
/// `index`: a file whose size and modification time are as the index left them is not opened at
/// all, a file that is new or has changed is read, and a file that has gone is dropped. The index
/// is written again at the end, in the order of the library.
///
/// A folder that cannot be read is passed over, and a folder reached twice through links or named
/// twice among the sources is walked once. An index that cannot be used is no reason to stop: the
/// files it does not account for are simply read, and an index of another shape is written over.
///
/// `index` is a file of qmus's own, so it belongs outside the sources: qmus writes nothing into
/// the music it reads.
#[must_use]
pub fn scan_with_index(sources: &[PathBuf], index: &Path) -> Vec<Track> {
    let found = walk::files(sources);
    // What the index says about each file, by the path of the file itself.
    let mut entries: HashMap<PathBuf, Entry> =
        index::read(index).into_iter().map(|entry| (entry.path().to_path_buf(), entry)).collect();
    // The tracks of the files the index still describes, and where the files still to be read go.
    let mut slots: Vec<Option<Track>> = Vec::with_capacity(found.len());
    let mut wanted: Vec<(usize, PathBuf, bool)> = Vec::new();
    for (at, (path, playable)) in found.into_iter().enumerate() {
        // The entry of a file that has not moved is what the library already knows; what is left is
        // read, and the entry of a file that has changed is thrown away rather than believed.
        match entries.remove(&path).filter(Entry::current) {
            Some(entry) => {
                slots.push(Some(entry.to_track()));
                entries.insert(path, entry);
            }
            None => {
                slots.push(None);
                wanted.push((at, path, playable));
            }
        }
    }
    let places: Vec<usize> = wanted.iter().map(|(at, ..)| *at).collect();
    let files: Vec<(PathBuf, bool)> = wanted.into_iter().map(|(_, path, playable)| (path, playable)).collect();
    // Reading on several threads is worth it for a library of new or changed files; a file whose
    // reader gave up is left out rather than making the whole library unreadable.
    for (at, track) in places.into_iter().zip(tags::read_all(files)) {
        // A file that cannot be asked about its size and time is not written down, so that it is
        // read again next time rather than trusted on words.
        if let Some(entry) = Entry::of(&track) {
            entries.insert(track.path.clone(), entry);
        }
        slots[at] = Some(track);
    }
    let mut tracks: Vec<Track> = slots.into_iter().flatten().collect();
    tracks.sort_by_cached_key(order_key);
    index::write(index, tracks.iter().filter_map(|track| entries.get(&track.path)));
    tracks
}

/// The tracks the index at `index` holds, in the order of artist, album, number and title they
/// were in when it was written, without a byte of the music being touched.
///
/// An index that is missing, cut short, nonsense or of another shape holds no tracks at all. That
/// is no error: the index is a cache of what the tags said, and [`scan_with_index`] is what fills
/// the library again.
#[must_use]
pub fn read_index(index: &Path) -> Vec<Track> {
    index::read(index).iter().map(Entry::to_track).collect()
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

    #[test]
    fn a_second_scan_through_the_index_over_an_unchanged_folder_reads_no_tags() {
        let scratch = Scratch::new("index-kept");
        tagged_wav(&scratch.path("music/a.wav"), 0.2, "First", "Kalben", "Sonsuz", 1);
        tagged_wav(&scratch.path("music/b.wav"), 0.2, "Second", "Kalben", "Sonsuz", 2);
        let music = scratch.path("music");
        let index = scratch.path("cache/library.index");
        let first = scan_with_index(std::slice::from_ref(&music), &index);
        assert_eq!(titles_of(&first), ["First", "Second"]);

        // The index is qmus's own file, so a title can be written into it by hand.
        let text = std::fs::read_to_string(&index).expect("index");
        std::fs::write(&index, text.replace("Second", "Yazilan")).expect("index written");

        let second = scan_with_index(std::slice::from_ref(&music), &index);
        assert_eq!(
            titles_of(&second),
            ["First", "Yazilan"],
            "the hand-written title comes back: only a kept entry can give it"
        );
        let mut expected = first.clone();
        expected[1].title = "Yazilan".to_owned();
        assert_eq!(second, expected, "every other field of every track is as it was");
        assert_eq!(read_index(&index), second, "and the index keeps the name that was written by hand");
    }

    #[test]
    fn a_file_whose_tags_have_changed_is_read_again() {
        let scratch = Scratch::new("index-changed");
        tagged_wav(&scratch.path("music/a.wav"), 0.2, "Once", "Kalben", "Sonsuz", 1);
        let music = scratch.path("music");
        let index = scratch.path("cache/library.index");
        let _ = scan_with_index(std::slice::from_ref(&music), &index);

        // A different title, a different length and a new time: what the index notices.
        tagged_wav(&scratch.path("music/a.wav"), 0.9, "Yine", "Kalben", "Sonsuz", 1);
        let tracks = scan_with_index(std::slice::from_ref(&music), &index);
        assert_eq!(titles_of(&tracks), ["Yine"], "the tags on disk are what the library says now");
        assert_eq!(tracks[0].duration, Some(Duration::from_millis(900)));
        assert_eq!(read_index(&index), tracks, "and the index carries the new reading to the next start");
    }

    #[test]
    fn a_new_file_is_added_and_a_deleted_one_is_dropped() {
        let scratch = Scratch::new("index-changes");
        tagged_wav(&scratch.path("music/1.wav"), 0.2, "Bir", "Kalben", "Sonsuz", 1);
        tagged_wav(&scratch.path("music/2.wav"), 0.2, "Iki", "Kalben", "Sonsuz", 2);
        let music = scratch.path("music");
        let index = scratch.path("cache/library.index");
        let _ = scan_with_index(std::slice::from_ref(&music), &index);

        tagged_wav(&scratch.path("music/3.wav"), 0.2, "Uc", "Kalben", "Sonsuz", 3);
        std::fs::remove_file(scratch.path("music/1.wav")).expect("file");
        let tracks = scan_with_index(std::slice::from_ref(&music), &index);
        assert_eq!(titles_of(&tracks), ["Iki", "Uc"], "the new file is in and the gone one is out");
        assert_eq!(
            read_index(&index),
            tracks,
            "the index follows the folder, the file that is not there is out of it too"
        );
    }

    #[test]
    fn several_sources_are_walked_together_and_one_named_twice_is_walked_once() {
        let scratch = Scratch::new("index-sources");
        tagged_wav(&scratch.path("music/deep/c.wav"), 0.5, "Alone", "Adamlar", "Eski", 1);
        tagged_wav(&scratch.path("music/a.wav"), 0.2, "First", "Kalben", "Sonsuz", 1);
        tagged_wav(&scratch.path("more/b.wav"), 0.2, "Second", "Kalben", "Sonsuz", 2);
        let index = scratch.path("cache/library.index");

        let both = scan_with_index(&[scratch.path("music"), scratch.path("more")], &index);
        assert_eq!(titles_of(&both), ["Alone", "First", "Second"], "the tracks of both folders: {both:?}");

        // The same folder twice over, and a link to it from inside itself.
        std::os::unix::fs::symlink(scratch.path("more"), scratch.path("music/link")).expect("link");
        let again = scan_with_index(&[scratch.path("music"), scratch.path("more"), scratch.path("more")], &index);
        assert_eq!(again, both, "a folder named twice and reached through a link is one folder");
        assert_eq!(titles_of(&read_index(&index)), ["Alone", "First", "Second"]);
    }

    #[test]
    fn the_tracks_through_the_index_are_the_tracks_a_plain_scan_gives() {
        let scratch = Scratch::new("index-order");
        tagged_wav(&scratch.path("music/deep/c.wav"), 0.5, "Alone", "Adamlar", "Eski", 1);
        tagged_wav(&scratch.path("music/a.wav"), 0.2, "First", "Kalben", "Sonsuz", 1);
        tagged_wav(&scratch.path("music/b.wav"), 0.2, "Second", "Kalben", "Sonsuz", 2);
        let music = scratch.path("music");
        let index = scratch.path("cache/library.index");

        let read_from_tags = scan(&music);
        assert_eq!(
            scan_with_index(std::slice::from_ref(&music), &index),
            read_from_tags,
            "the same tracks in the same order"
        );
        assert_eq!(
            scan_with_index(std::slice::from_ref(&music), &index),
            read_from_tags,
            "and again, this time from the index"
        );
        assert_eq!(read_index(&index), read_from_tags, "which is what the index says it holds");
    }

    #[test]
    fn the_index_hands_the_library_over_without_the_music_being_there() {
        let scratch = Scratch::new("index-alone");
        tagged_wav(&scratch.path("music/a.wav"), 0.2, "First", "Kalben", "Sonsuz", 1);
        let index = scratch.path("cache/library.index");
        let tracks = scan_with_index(&[scratch.path("music")], &index);
        assert_eq!(tracks.len(), 1);

        // The music can be gone entirely and the index still says what was in it.
        std::fs::remove_dir_all(scratch.path("music")).expect("folder");
        assert_eq!(read_index(&index), tracks, "a track that is not on disk is still known by its name");
        assert!(
            scan_with_index(&[scratch.path("music")], &index).is_empty(),
            "a walk of what is there gives what is there"
        );
    }

    #[test]
    fn nothing_is_written_into_the_music_folders() {
        let scratch = Scratch::new("index-read-only");
        tagged_wav(&scratch.path("music/bir/1.wav"), 0.2, "Bir", "Kalben", "Sonsuz", 1);
        sine_wav(&scratch.path("music/iki/2.wav"), 8_000, 1, 0.2, 440.0);
        let music = scratch.path("music");
        let index = scratch.path("cache/library.index");
        let _ = scan_with_index(std::slice::from_ref(&music), &index);
        let _ = scan_with_index(std::slice::from_ref(&music), &index);

        assert_eq!(files_under(&music), ["1.wav", "2.wav"], "the folder holds the two files it was given");
        assert_eq!(
            std::fs::read_dir(scratch.path("cache")).expect("folder").count(),
            1,
            "the index is written outside the music"
        );
    }

    /// The titles of `tracks`, in the order they are in.
    fn titles_of(tracks: &[Track]) -> Vec<&str> {
        tracks.iter().map(|track| track.title.as_str()).collect()
    }

    /// The names of every file under `folder`, for a check that nothing else was put there.
    fn files_under(folder: &Path) -> Vec<String> {
        let mut found = Vec::new();
        let mut folders = vec![folder.to_path_buf()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).expect("folder").flatten() {
                if entry.path().is_dir() {
                    folders.push(entry.path());
                } else {
                    found.push(entry.file_name().to_string_lossy().into_owned());
                }
            }
        }
        found.sort();
        found
    }
}
