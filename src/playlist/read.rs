//! Reading a playlist file: the items it lists, what it says about them, and which of them are
//! still there.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use super::{Item, Playlist, name_of};
use crate::library::{Location, SourceKey};

/// What an `#EXTINF` line says about the item that comes after it.
#[derive(Default)]
struct Extended {
    title: Option<String>,
    duration: Option<Duration>,
}

/// The playlist at `path`, with its items in the order of the file. A track that is not there any
/// more is still an item, only one that is not present, because the list is what the person chose
/// and not a scan of what exists.
///
/// # Errors
///
/// Returns the file system's reason when the playlist cannot be read, and says so when a `.m3u8`
/// holds anything but UTF-8, which is what its name promises.
pub fn read(path: &Path) -> io::Result<Playlist> {
    let text = text(path)?;
    let folder = path.parent().unwrap_or(Path::new(""));
    let mut items = Vec::new();
    let mut extended: Option<Extended> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            extended = Some(Extended::of(info));
            continue;
        }
        // `#EXTM3U` and every other comment speak about the file, never about a track.
        if line.starts_with('#') {
            continue;
        }
        // A line that says nothing of an item is an item with no title and no length, and an
        // `#EXTINF` at the end of the file with no line after it says nothing of anything.
        let Extended { title, duration } = extended.take().unwrap_or_default();
        if let Some(remote) = remote_of(line) {
            items.push(Item { location: remote, title, duration, present: true });
            continue;
        }
        let address = is_address(line);
        let track = match line.strip_prefix("file://") {
            // A `file://` line names the same file a plain line does, only with its spaces and
            // its letters outside ASCII written as percent escapes.
            Some(url) => located(folder, &percent_decoded(url)),
            None if address => PathBuf::from(line),
            None => located(folder, line),
        };
        // Nothing is asked whether an address is there: it is never fetched, and whether it plays
        // is the player's business.
        let present = !address && track.exists();
        items.push(Item { location: Location::File(track), title, duration, present });
    }
    Ok(Playlist { name: name_of(path), path: path.to_path_buf(), items })
}

impl Extended {
    /// What an `#EXTINF` line says: whole seconds, then the title. Seconds that do not read as
    /// whole seconds are no length — `-1` is what players write for a stream, and `0` what they
    /// write for a track they do not know the length of — but the title beside them still is a
    /// title.
    fn of(info: &str) -> Self {
        let (seconds, title) = info.split_once(',').unwrap_or((info, ""));
        let title = title.trim();
        Self {
            title: (!title.is_empty()).then(|| title.to_owned()),
            duration: seconds.trim().parse::<u64>().ok().map(Duration::from_secs).filter(|length| !length.is_zero()),
        }
    }
}

/// The bytes of `path` as text. A `.m3u8` is UTF-8, which is what its name promises, so bytes
/// that are not is an error; a `.m3u` comes from players that wrote in the encoding of their own
/// country, and is read as Latin-1 where a byte is the letter of the same number.
fn text(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    let failed = match String::from_utf8(bytes) {
        Ok(text) => return Ok(text),
        Err(failed) => failed,
    };
    if path.extension().is_some_and(|ending| ending.eq_ignore_ascii_case("m3u8")) {
        let reason = format!("{} is not the UTF-8 its name promises: {}", path.display(), failed.utf8_error());
        return Err(io::Error::new(io::ErrorKind::InvalidData, reason));
    }
    Ok(failed.into_bytes().into_iter().map(char::from).collect())
}

/// The account's track a `qmus://<key>/<id>` line names, when it is one.
fn remote_of(line: &str) -> Option<Location> {
    let (key, id) = line.strip_prefix(super::REMOTE)?.split_once('/')?;
    let id = percent_decoded(id);
    (!key.is_empty() && !id.is_empty()).then(|| Location::Remote { source: SourceKey(key.to_owned()), id })
}

/// Whether a line is an address a player would fetch rather than a file on this machine.
fn is_address(line: &str) -> bool {
    line.starts_with("http://") || line.starts_with("https://")
}

/// A path named by a playlist line: as it is written when it is whole, and from the playlist's own
/// folder when it is not, with its `..` steps taken, so it is the same path the library knows the
/// track by.
fn located(folder: &Path, named: &str) -> PathBuf {
    let path = Path::new(named);
    let whole = if path.is_absolute() { path.to_path_buf() } else { folder.join(path) };
    let mut resolved = PathBuf::new();
    for part in whole.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            other => resolved.push(other),
        }
    }
    resolved
}

/// The path in a `file://` line with its percent escapes read as the characters they stand for: a
/// space is written `%20` and a letter outside ASCII its UTF-8 bytes in hex, so the escapes are
/// read back to the bytes of the name and then to the name itself.
fn percent_decoded(url: &str) -> String {
    let escapes = url.as_bytes();
    let mut bytes = Vec::with_capacity(escapes.len());
    let mut at = 0;
    while let Some(&letter) = escapes.get(at) {
        // A percent escape stands for the byte its two hexadecimal digits name; everything else,
        // a `%` that no two digits follow included, is the letter itself.
        let escape = if letter == b'%' { escaped(escapes.get(at + 1..at + 3)) } else { None };
        match escape {
            Some(byte) => {
                bytes.push(byte);
                at += 3;
            }
            None => {
                bytes.push(letter);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The byte two hexadecimal digits name, or nothing when they are not two hexadecimal digits.
fn escaped(digits: Option<&[u8]>) -> Option<u8> {
    let [high, low] = *digits? else { return None };
    Some(hexadecimal(high)? * 16 + hexadecimal(low)?)
}

/// What one hexadecimal digit of an escape is worth.
fn hexadecimal(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::testing::{Scratch, sine_wav};

    #[test]
    fn a_file_written_by_hand_gives_its_items_in_order_with_every_path_resolved() {
        let scratch = Scratch::new("read-plain");
        let here = scratch.path("music/here.wav");
        sine_wav(&here, 8_000, 1, 0.1, 440.0);
        sine_wav(&scratch.path("music/there.wav"), 8_000, 1, 0.1, 440.0);
        let folder = scratch.path("playlists");
        fs::create_dir_all(&folder).expect("folder");
        let text = format!(
            "#EXTM3U\n# a comment about nothing\n\n#EXTINF:187,Kalben - Sonsuz\n{}\n../music/there.wav\n#EXTINF:99,Nothing\n",
            here.display()
        );
        fs::write(folder.join("Evening.m3u8"), text).expect("playlist");
        let playlist = read(&folder.join("Evening.m3u8")).expect("playlist");
        assert_eq!(playlist.name, "Evening");
        assert_eq!(playlist.path, folder.join("Evening.m3u8"));
        assert_eq!(playlist.items.len(), 2);
        assert_eq!(
            playlist.items[0],
            Item {
                location: Location::from(here),
                title: Some("Kalben - Sonsuz".to_owned()),
                duration: Some(Duration::from_secs(187)),
                present: true,
            }
        );
        assert_eq!(
            playlist.items[1].location,
            Location::from(scratch.path("music/there.wav")),
            "the path the library knows"
        );
        assert_eq!(playlist.items[1].title, None);
        assert_eq!(playlist.items[1].duration, None);
    }

    #[test]
    fn a_file_address_line_gives_the_file_it_names_with_its_escapes_spelled_out() {
        let scratch = Scratch::new("read-url");
        let song = scratch.path("music/çark bir.wav");
        sine_wav(&song, 8_000, 1, 0.1, 440.0);
        let folder = scratch.path("playlists");
        fs::create_dir_all(&folder).expect("folder");
        let written = song.to_str().expect("path").replace('ç', "%C3%A7").replace(' ', "%20");
        fs::write(folder.join("Evening.m3u8"), format!("#EXTM3U\nfile://{written}\n")).expect("playlist");
        let playlist = read(&folder.join("Evening.m3u8")).expect("playlist");
        assert_eq!(playlist.items.len(), 1);
        assert_eq!(playlist.items[0].location, Location::from(song));
        assert!(playlist.items[0].present);
    }

    #[test]
    fn a_playlist_in_latin_one_gives_the_letters_its_bytes_name() {
        let scratch = Scratch::new("read-latin");
        let song = scratch.path("music/song.wav");
        sine_wav(&song, 8_000, 1, 0.1, 440.0);
        let folder = scratch.path("playlists");
        fs::create_dir_all(&folder).expect("folder");
        let mut bytes = b"#EXTM3U\n#EXTINF:-1,caf\xE7\n".to_vec();
        bytes.extend_from_slice(song.to_str().expect("path").as_bytes());
        bytes.push(b'\n');
        fs::write(folder.join("Cafe.m3u"), &bytes).expect("playlist");
        let playlist = read(&folder.join("Cafe.m3u")).expect("playlist");
        assert_eq!(playlist.items.len(), 1);
        assert_eq!(playlist.items[0].title, Some("caf\u{e7}".to_owned()));
        assert_eq!(playlist.items[0].duration, None);
        assert_eq!(playlist.items[0].location, Location::from(song));
    }

    #[test]
    fn a_playlist_that_is_not_the_text_its_name_promises_is_an_error() {
        let scratch = Scratch::new("read-not-text");
        let folder = scratch.path("playlists");
        fs::create_dir_all(&folder).expect("folder");
        fs::write(folder.join("Cafe.m3u8"), b"#EXTM3U\n#EXTINF:-1,caf\xE7\n").expect("playlist");
        let error = read(&folder.join("Cafe.m3u8")).expect_err("not UTF-8");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn an_address_line_is_kept_as_it_is_written_and_is_never_asked_whether_it_is_there() {
        let scratch = Scratch::new("read-address");
        let folder = scratch.path("playlists");
        fs::create_dir_all(&folder).expect("folder");
        let text = "#EXTM3U\n#EXTINF:210,Radio\nhttps://stream.example/live.mp3\nhttp://other.example/one.mp3\n";
        fs::write(folder.join("Radio.m3u8"), text).expect("playlist");
        let playlist = read(&folder.join("Radio.m3u8")).expect("playlist");
        assert_eq!(playlist.items.len(), 2);
        assert_eq!(playlist.items[0].location, Location::from(PathBuf::from("https://stream.example/live.mp3")));
        assert_eq!(playlist.items[0].title, Some("Radio".to_owned()));
        assert_eq!(playlist.items[0].duration, Some(Duration::from_secs(210)));
        assert!(!playlist.items[0].present);
        assert_eq!(playlist.items[1].location, Location::from(PathBuf::from("http://other.example/one.mp3")));
        assert_eq!(playlist.items[1].title, None);
        assert_eq!(playlist.items[1].duration, None);
        assert!(!playlist.items[1].present);
    }

    #[test]
    fn a_track_that_is_not_there_any_more_stays_in_the_list() {
        let scratch = Scratch::new("read-gone");
        let folder = scratch.path("playlists");
        fs::create_dir_all(&folder).expect("folder");
        let gone = scratch.path("music/gone.wav");
        let text = format!("#EXTM3U\n#EXTINF:120,Gone\n{}\n", gone.display());
        fs::write(folder.join("Evening.m3u8"), text).expect("playlist");
        let playlist = read(&folder.join("Evening.m3u8")).expect("playlist");
        assert_eq!(playlist.items.len(), 1);
        assert_eq!(playlist.items[0].location, Location::from(gone));
        assert_eq!(playlist.items[0].title, Some("Gone".to_owned()));
        assert!(!playlist.items[0].present);
    }

    #[test]
    fn a_playlist_that_is_not_there_is_an_error_and_never_a_panic() {
        let scratch = Scratch::new("read-missing");
        let error = read(&scratch.path("playlists/Evening.m3u8")).expect_err("playlist");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }
}
