//! Walking the source folders: every sound file below them, each listed once.
//!
//! Sources can overlap and a folder can hold a link back to its own parent, so a folder is walked
//! by the real path the filesystem gives it and one already walked is not walked again. A folder
//! that cannot be read is passed over: a music library with one unreachable corner is still a music
//! library.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The endings of the files qmus plays.
const PLAYED: [&str; 10] = ["flac", "mp3", "ogg", "oga", "m4a", "mp4", "aac", "wav", "aiff", "aif"];

/// The endings of music files qmus lists but cannot play yet: they are shown faint rather than
/// hidden, so nobody wonders where their music went.
const LATER: [&str; 1] = ["opus"];

/// Every sound file below `sources`, each listed once, with whether qmus can play it yet, in the
/// order the folders were reached.
pub(crate) fn files(sources: &[PathBuf]) -> Vec<(PathBuf, bool)> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let mut folders = sources.to_vec();
    while let Some(folder) = folders.pop() {
        let Ok(real) = fs::canonicalize(&folder) else { continue };
        if !seen.insert(real) {
            continue;
        }
        let Ok(entries) = fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            // A link to a folder is followed, so a link back to a parent is walked once and no
            // further than the folder it points at.
            if path.is_dir() {
                folders.push(path);
            } else if let Some(playable) = kind(&path) {
                found.push((path, playable));
            }
        }
    }
    found
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
