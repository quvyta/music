//! Reading what a file says about itself: its tags, and where it came from where its tags say
//! nothing.

use std::path::PathBuf;

use lofty::prelude::*;

use crate::library::Track;

/// The most threads that read files at once.
const READERS: usize = 8;

/// The tracks of the files `found`, read on a few threads at once: most of the time goes into
/// waiting for the disk, and several files are waited for together.
pub(crate) fn read_all(found: Vec<(PathBuf, bool)>) -> Vec<Track> {
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
