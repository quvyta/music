//! Writing playlists: a new one, one written over, the same one under a new name, and the
//! removal of a list file.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use super::{Track, is_playlist};
use crate::library::Location;

/// The ending every playlist qmus writes has: `.m3u8` says its paths and its titles are UTF-8.
const ENDING: &str = "m3u8";

/// A new playlist of `items` called `name` in `folder`; the path it was written to. The folder is
/// made when it is not there, so the first playlist on a machine works. A name is cleaned for the
/// file system and can therefore name nothing but a file in `folder`.
///
/// # Errors
///
/// Returns [`io::ErrorKind::InvalidInput`] when nothing is left of the name, and
/// [`io::ErrorKind::AlreadyExists`] when a playlist of that name is already there: a list the
/// person may have changed is never written over without being asked to.
pub fn write(folder: &Path, name: &str, items: &[Track]) -> io::Result<PathBuf> {
    let path = folder.join(format!("{}.{ENDING}", clean(name)?));
    free(&path)?;
    fs::create_dir_all(folder)?;
    save(&path, items)?;
    Ok(path)
}

/// The items of the playlist at `path`, written over it. The name and the folder stay as they
/// are: the playlist is already there, and only its list is given again.
///
/// # Errors
///
/// Returns the file system's reason when the playlist cannot be written.
pub fn replace(path: &Path, items: &[Track]) -> io::Result<()> {
    save(path, items)
}

/// The playlist at `path` under a new name; where it now is. The new name is cleaned as a name
/// for [`write()`] is, and the playlist stays in the folder it was in. A name is the whole name:
/// the file a renamed playlist gets is always an `.m3u8`, because that is what qmus writes.
///
/// # Errors
///
/// Returns [`io::ErrorKind::InvalidInput`] when nothing is left of the name, and
/// [`io::ErrorKind::AlreadyExists`] when a playlist of that name is already there, in which case
/// the old playlist is left where it was.
pub fn rename(path: &Path, new_name: &str) -> io::Result<PathBuf> {
    let folder = path.parent().unwrap_or(Path::new(""));
    let to = folder.join(format!("{}.{ENDING}", clean(new_name)?));
    free(&to)?;
    fs::rename(path, &to)?;
    Ok(to)
}

/// The playlist file at `path`, which must be one of the playlists in `folder`. Only that one file
/// goes: the tracks it lists and the other playlists are not touched, because a playlist is a
/// list and not an owner of the music. Whether to ask the person first is the screen's business,
/// not this module's.
///
/// # Errors
///
/// Returns [`io::ErrorKind::InvalidInput`] when the file is not a playlist, and
/// [`io::ErrorKind::PermissionDenied`] when it does not sit directly in `folder`: this is what
/// keeps a sound file, or any other file at all, out of what removes anything. Returns the file
/// system's reason when the file cannot be removed.
pub fn delete(folder: &Path, path: &Path) -> io::Result<()> {
    if !is_playlist(path) {
        let reason = format!("{} is not a playlist", path.display());
        return Err(io::Error::new(io::ErrorKind::InvalidInput, reason));
    }
    // A playlist is one of the playlists in the folder, so a file reached through `..` or kept in
    // a subfolder is not one of them, whatever the person meant by it.
    if path.parent() != Some(folder) {
        let reason = format!("{} is not in {}", path.display(), folder.display());
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, reason));
    }
    fs::remove_file(path)
}

/// The refusal of a name that is taken: the list that is there is the person's own, and qmus never
/// writes over it unless it is told to replace it.
fn free(path: &Path) -> io::Result<()> {
    if path.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} is already there", path.display())));
    }
    Ok(())
}

/// A name that can only ever be a file in the playlists folder: nothing in it can open a folder,
/// and nothing in it can climb out of one.
fn clean(name: &str) -> io::Result<String> {
    let name: String =
        name.trim().chars().map(|letter| if letter == '/' || letter == '\0' { '-' } else { letter }).collect();
    let name = name.trim_start_matches('.');
    if name.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "a playlist needs a name"));
    }
    Ok(name.to_owned())
}

/// Writes `items` to `path` whole or not at all, so a playlist that is half written is never the
/// one a reader sees.
fn save(path: &Path, items: &[Track]) -> io::Result<()> {
    let folder = path.parent().unwrap_or(Path::new(""));
    qframe::storage::atomic_write(path, content(folder, items).as_bytes())
}

/// The file a playlist of `items` is: the header, and for every item what is said about it
/// followed by where it is.
fn content(folder: &Path, items: &[Track]) -> String {
    let mut text = String::from("#EXTM3U\n");
    for item in items {
        if item.title.is_some() || item.duration.is_some() {
            // M3U counts whole seconds, so a length is written as whole seconds and what a player
            // reads back is the second the file names. A length qmus does not know is the `-1`
            // the format has for it, and the comma is part of the line whether a title follows
            // it or not.
            let seconds = item.duration.and_then(|length| i64::try_from(length.as_secs()).ok()).unwrap_or(-1);
            text.push_str(&format!("#EXTINF:{seconds},{}\n", item.title.as_deref().unwrap_or_default()));
        }
        text.push_str(&line_of(folder, &item.location));
        text.push('\n');
    }
    text
}

/// What an item's line names: a file's path, or an account's track as `qmus://<key>/<id>`.
fn line_of(folder: &Path, track: &Location) -> String {
    match track {
        Location::File(path) => location(folder, path),
        Location::Remote { source, id } => format!("{}{}/{}", super::REMOTE, source.0, escaped(id)),
    }
}

/// `id` with every byte but letters, digits and `-._~` written as a percent escape, so a slash, a
/// space or a line's end in it cannot change what the line says.
fn escaped(id: &str) -> String {
    id.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

/// The path written on an item's line: from the playlist's folder where the track and the folder
/// share a root of their own, and whole where they do not.
fn location(folder: &Path, track: &Path) -> String {
    relative(folder, track).unwrap_or_else(|| track.to_path_buf()).display().to_string()
}

/// The track as a path from the playlist's folder: the part both have in common is dropped, and
/// each folder above it becomes a step up. A track on another tree shares nothing but the root of
/// the machine, and no path from there is a path at all, so it keeps its own.
fn relative(folder: &Path, track: &Path) -> Option<PathBuf> {
    let shared = folder.components().zip(track.components()).take_while(|(one, other)| one == other).count();
    let last = folder.components().nth(shared.checked_sub(1)?)?;
    if !matches!(last, Component::Normal(_)) {
        return None;
    }
    let root: PathBuf = folder.components().take(shared).collect();
    let up = folder.strip_prefix(&root).ok()?;
    let rest = track.strip_prefix(&root).ok()?;
    let mut path = PathBuf::new();
    for _ in up.components() {
        path.push("..");
    }
    if !rest.as_os_str().is_empty() {
        path.push(rest);
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::time::Duration;

    use super::super::{list, read};
    use super::*;
    use crate::testing::{Scratch, sine_wav};

    #[test]
    fn a_playlist_written_with_paths_from_its_folder_reads_back_with_every_track_there() {
        let scratch = Scratch::new("write-round-trip");
        let one = scratch.path("music/one.wav");
        let two = scratch.path("music/two.wav");
        sine_wav(&one, 8_000, 1, 0.1, 440.0);
        sine_wav(&two, 8_000, 1, 0.1, 440.0);
        let folder = scratch.path("playlists");
        let items = [
            Track {
                location: Location::from(one),
                title: Some("Kalben - Sonsuz".to_owned()),
                duration: Some(Duration::from_secs(187)),
            },
            Track { location: Location::from(two), title: Some("Sade".to_owned()), duration: None },
        ];
        let path = write(&folder, "Evening", &items).expect("playlist");
        assert_eq!(path, folder.join("Evening.m3u8"));
        let written = fs::read_to_string(&path).expect("text");
        assert_eq!(
            written,
            "#EXTM3U\n#EXTINF:187,Kalben - Sonsuz\n../music/one.wav\n#EXTINF:-1,Sade\n../music/two.wav\n"
        );
        let playlist = read(&path).expect("playlist");
        assert_eq!(playlist.name, "Evening");
        assert_eq!(playlist.path, path);
        assert_eq!(playlist.items.len(), 2);
        assert_eq!(playlist.items[0].location, Location::from(scratch.path("music/one.wav")), "the path written in");
        assert_eq!(playlist.items[0].title, Some("Kalben - Sonsuz".to_owned()));
        assert_eq!(playlist.items[0].duration, Some(Duration::from_secs(187)));
        assert!(playlist.items[0].present);
        assert_eq!(playlist.items[1].location, Location::from(scratch.path("music/two.wav")));
        assert_eq!(playlist.items[1].title, Some("Sade".to_owned()));
        assert_eq!(playlist.items[1].duration, None);
        assert!(playlist.items[1].present);
    }

    #[test]
    fn a_title_or_a_length_alone_still_gets_a_line_of_its_own_and_each_reads_back_as_it_was_given() {
        let scratch = Scratch::new("write-extinf");
        let folder = scratch.path("playlists");
        let items = ["both", "title", "length", "nothing"].map(|name| {
            let path = scratch.path(&format!("music/{name}.wav"));
            sine_wav(&path, 8_000, 1, 0.1, 440.0);
            path
        });
        let items = [
            Track {
                location: Location::from(items[0].clone()),
                title: Some("Both".to_owned()),
                duration: Some(Duration::from_secs(187)),
            },
            Track { location: Location::from(items[1].clone()), title: Some("Title alone".to_owned()), duration: None },
            Track { location: Location::from(items[2].clone()), title: None, duration: Some(Duration::from_secs(95)) },
            Track { location: Location::from(items[3].clone()), title: None, duration: None },
        ];
        let path = write(&folder, "Every", &items).expect("playlist");
        let written = fs::read_to_string(&path).expect("text");
        assert_eq!(
            written,
            "#EXTM3U\n#EXTINF:187,Both\n../music/both.wav\n#EXTINF:-1,Title alone\n../music/title.wav\n\
             #EXTINF:95,\n../music/length.wav\n../music/nothing.wav\n"
        );
        let playlist = read(&path).expect("playlist");
        assert_eq!(playlist.items.len(), 4);
        assert_eq!(playlist.items[0].title.as_deref(), Some("Both"));
        assert_eq!(playlist.items[0].duration, Some(Duration::from_secs(187)));
        assert_eq!(playlist.items[1].title.as_deref(), Some("Title alone"));
        assert_eq!(playlist.items[1].duration, None);
        assert_eq!(playlist.items[2].title, None);
        assert_eq!(playlist.items[2].duration, Some(Duration::from_secs(95)));
        assert_eq!(playlist.items[3].title, None);
        assert_eq!(playlist.items[3].duration, None);
    }

    #[test]
    fn a_track_on_another_tree_is_written_with_its_whole_path() {
        let scratch = Scratch::new("write-absolute");
        let folder = scratch.path("playlists");
        let items =
            [Track { location: Location::from(PathBuf::from("/mnt/sother/song.wav")), title: None, duration: None }];
        let path = write(&folder, "Far", &items).expect("playlist");
        let written = fs::read_to_string(&path).expect("text");
        assert_eq!(written, "#EXTM3U\n/mnt/sother/song.wav\n");
        let playlist = read(&path).expect("playlist");
        assert_eq!(playlist.items[0].location, Location::from(PathBuf::from("/mnt/sother/song.wav")));
        assert!(!playlist.items[0].present);
    }

    #[test]
    fn a_name_that_is_taken_is_never_written_over_and_replace_writes_over_it_where_it_is() {
        let scratch = Scratch::new("write-taken");
        let folder = scratch.path("playlists");
        let song = scratch.path("music/song.wav");
        sine_wav(&song, 8_000, 1, 0.1, 440.0);
        let items = [Track { location: Location::from(song), title: None, duration: None }];
        let path = write(&folder, "Evening", &[]).expect("playlist");
        let kept = fs::read(&path).expect("bytes");
        assert_eq!(kept, b"#EXTM3U\n");
        let error = write(&folder, "Evening", &items).expect_err("taken");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&path).expect("bytes"), kept);
        replace(&path, &items).expect("replaced");
        let written = fs::read_to_string(&path).expect("text");
        assert_eq!(written, "#EXTM3U\n../music/song.wav\n");
        let again = write(&folder, "Evening", &items).expect_err("taken");
        assert_eq!(again.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&path).expect("text"), written);
    }

    #[test]
    fn a_name_with_a_separator_in_it_or_dots_in_front_of_it_cannot_leave_the_folder() {
        let scratch = Scratch::new("write-name");
        let folder = scratch.path("playlists");
        let climbed = write(&folder, "../../etc/passwd", &[]).expect("playlist");
        let hidden = write(&folder, "..hidden", &[]).expect("playlist");
        assert_eq!(climbed.parent(), Some(folder.as_path()));
        assert_eq!(climbed.file_name(), Some(OsStr::new("-..-etc-passwd.m3u8")));
        assert_eq!(hidden.parent(), Some(folder.as_path()));
        assert_eq!(hidden.file_name(), Some(OsStr::new("hidden.m3u8")));
    }

    #[test]
    fn a_name_with_nothing_in_it_is_refused() {
        let scratch = Scratch::new("write-empty");
        let folder = scratch.path("playlists");
        for name in ["", "   ", "..", " ...  "] {
            let error = write(&folder, name, &[]).expect_err("no name");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{name:?}");
        }
    }

    #[test]
    fn a_playlist_under_a_new_name_moves_to_it_and_a_name_already_there_is_refused() {
        let scratch = Scratch::new("write-rename");
        let folder = scratch.path("playlists");
        let song = scratch.path("music/song.wav");
        sine_wav(&song, 8_000, 1, 0.1, 440.0);
        let items = [Track { location: Location::from(song), title: Some("Sade".to_owned()), duration: None }];
        let path = write(&folder, "Evening", &items).expect("playlist");
        let kept = fs::read(&path).expect("bytes");
        let morning = write(&folder, "Morning", &[]).expect("playlist");
        let error = rename(&path, "Morning").expect_err("taken");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(path.exists());
        let to = rename(&path, "Night").expect("renamed");
        assert_eq!(to, folder.join("Night.m3u8"));
        assert!(!path.exists());
        assert_eq!(fs::read(&to).expect("bytes"), kept);
        assert!(morning.exists());
    }

    #[test]
    fn a_sound_file_in_the_playlists_folder_is_never_removed() {
        let scratch = Scratch::new("delete-sound");
        let folder = scratch.path("playlists");
        let song = scratch.path("playlists/song.wav");
        sine_wav(&song, 8_000, 1, 0.1, 440.0);
        let kept = fs::read(&song).expect("bytes");
        let error = delete(&folder, &song).expect_err("sound file");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(fs::read(&song).expect("bytes"), kept);
    }

    #[test]
    fn a_playlist_outside_the_playlists_folder_is_never_removed() {
        let scratch = Scratch::new("delete-outside");
        let folder = scratch.path("playlists");
        let elsewhere = scratch.path("elsewhere/Other.m3u8");
        fs::write(&elsewhere, "#EXTM3U\n").expect("file");
        let error = delete(&folder, &elsewhere).expect_err("outside the folder");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(elsewhere.exists());
    }

    #[test]
    fn a_playlist_named_through_parent_folders_is_never_removed() {
        let scratch = Scratch::new("delete-parent");
        let folder = scratch.path("playlists");
        let elsewhere = scratch.path("elsewhere/Other.m3u8");
        fs::write(&elsewhere, "#EXTM3U\n").expect("file");
        let through = folder.join("../elsewhere/Other.m3u8");
        let error = delete(&folder, &through).expect_err("through parent folders");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(elsewhere.exists());
    }

    #[test]
    fn a_playlist_is_removed_while_the_music_it_lists_and_the_other_playlists_stay() {
        let scratch = Scratch::new("delete-playlist");
        let folder = scratch.path("playlists");
        let song = scratch.path("playlists/song.wav");
        sine_wav(&song, 8_000, 1, 0.1, 440.0);
        let kept = fs::read(&song).expect("bytes");
        let items = [Track { location: Location::from(song.clone()), title: None, duration: None }];
        let evening = write(&folder, "Evening", &items).expect("playlist");
        let morning = write(&folder, "Morning", &items).expect("playlist");
        delete(&folder, &evening).expect("deleted");
        assert!(!evening.exists());
        assert!(morning.exists());
        assert_eq!(fs::read(&song).expect("bytes"), kept);
    }

    #[test]
    fn a_playlist_leaves_nothing_but_itself_in_the_folder() {
        let scratch = Scratch::new("write-temporary");
        let folder = scratch.path("playlists");
        write(&folder, "Evening", &[]).expect("playlist");
        let mut names: Vec<String> = fs::read_dir(&folder)
            .expect("folder")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["Evening.m3u8"]);
        assert_eq!(list(&folder).len(), 1);
    }

    #[test]
    fn an_accounts_track_is_written_as_an_address_of_qmus_and_reads_back_as_itself() {
        let scratch = Scratch::new("playlist-remote");
        let song = scratch.path("music/song.wav");
        sine_wav(&song, 8_000, 1, 0.1, 440.0);
        let folder = scratch.path("playlists");
        let remote = Location::Remote {
            source: crate::library::SourceKey("navidrome-3f9a".to_owned()),
            id: "al/bum 7%".to_owned(),
        };
        let items = [
            Track { location: Location::from(song.clone()), title: None, duration: None },
            Track { location: remote.clone(), title: Some("Kalben - Gece Mavisi".to_owned()), duration: None },
        ];
        let path = write(&folder, "Karışık", &items).expect("written");
        let text = fs::read_to_string(&path).expect("the file");
        assert!(
            text.contains("qmus://navidrome-3f9a/al%2Fbum%207%25\n"),
            "a slash, a space and a percent escaped: {text}"
        );
        let playlist = read(&path).expect("read back");
        assert_eq!(playlist.items[0].location, Location::from(song));
        assert_eq!(playlist.items[1].location, remote);
        assert_eq!(playlist.items[1].title.as_deref(), Some("Kalben - Gece Mavisi"));
        assert!(playlist.items[1].present, "whether it is still there is the account's to say");
    }
}
