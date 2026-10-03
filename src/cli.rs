//! The command line: `qmus [FOLDER]`, `--version` and `--help`.
//!
//! A folder given is the music shown; without one qmus shows the person's Music folder. A path
//! that does not exist is a one-line message and exit code 2 before any screen is drawn.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    /// Open the screen with the music of this folder.
    Screen(PathBuf),
    /// Print the version and leave.
    Version,
    /// Print how to use qmus and leave.
    Help,
    /// A path that does not exist, or is not a folder.
    Missing(PathBuf),
    /// An option qmus does not know, or more than one path.
    Unknown(String),
}

/// Reads the arguments after the program name. `music` is the folder shown when none is given,
/// and relative paths are read from `cwd`.
#[must_use]
pub fn parse(args: impl IntoIterator<Item = OsString>, music: &Path, cwd: &Path) -> Invocation {
    let mut path = None;
    let mut only_paths = false;
    for arg in args {
        let text = arg.to_string_lossy();
        if !only_paths && text.starts_with('-') && text.len() > 1 {
            match text.as_ref() {
                "--version" | "-V" => return Invocation::Version,
                "--help" | "-h" => return Invocation::Help,
                "--" => only_paths = true,
                other => return Invocation::Unknown(other.to_owned()),
            }
            continue;
        }
        if path.is_some() {
            return Invocation::Unknown(text.into_owned());
        }
        path = Some(PathBuf::from(arg));
    }
    // The Music folder may not exist yet; the screen then says so and offers nothing to play.
    let Some(path) = path else { return Invocation::Screen(music.to_path_buf()) };
    let path = if path.is_absolute() { path } else { cwd.join(path) };
    match fs::canonicalize(&path) {
        Ok(folder) if folder.is_dir() => Invocation::Screen(folder),
        _ => Invocation::Missing(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Scratch;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn a_folder_is_shown_and_nothing_shows_the_music_folder() {
        let scratch = Scratch::new("cli-folders");
        let albums = scratch.path("albums/x").parent().expect("folder").to_path_buf();
        std::fs::create_dir_all(&albums).expect("folder");
        let music = scratch.path("Music");
        let root = albums.parent().expect("root").to_path_buf();
        assert_eq!(parse(args(&["albums"]), &music, &root), Invocation::Screen(albums.clone()));
        assert_eq!(parse(args(&[albums.to_str().expect("path")]), &music, &music), Invocation::Screen(albums));
        assert_eq!(parse(args(&[]), &music, &root), Invocation::Screen(music));
    }

    #[test]
    fn a_path_that_is_not_a_folder_is_missing_and_options_are_read() {
        let scratch = Scratch::new("cli-missing");
        let file = scratch.path("song.flac");
        std::fs::write(&file, "x").expect("file");
        let root = file.parent().expect("root").to_path_buf();
        let root = root.as_path();
        assert_eq!(parse(args(&["gone"]), root, root), Invocation::Missing(root.join("gone")));
        assert_eq!(parse(args(&["song.flac"]), root, root), Invocation::Missing(file.clone()));
        assert_eq!(parse(args(&["--version"]), root, root), Invocation::Version);
        assert_eq!(parse(args(&["-h"]), root, root), Invocation::Help);
        assert_eq!(parse(args(&["--shuffle"]), root, root), Invocation::Unknown("--shuffle".into()));
        assert_eq!(parse(args(&[".", "."]), root, root), Invocation::Unknown(".".into()));
    }
}
