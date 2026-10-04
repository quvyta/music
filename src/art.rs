//! Album covers: the picture a track carries in its tags, or the one lying beside it in its
//! folder. qmus only reads them.

use std::fs;
use std::path::{Path, PathBuf};

use lofty::picture::PictureType;
use lofty::prelude::*;
use qframe::widgets::ImageData;

/// The names a cover lying in an album's folder goes by, before its ending, in the order looked
/// for.
const NAMES: [&str; 4] = ["cover", "folder", "front", "album"];

/// The endings of a cover lying in a folder.
const ENDINGS: [&str; 4] = ["jpg", "jpeg", "png", "webp"];

/// Where a track's cover is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cover {
    /// Carried in the track's own tags.
    Embedded(Vec<u8>),
    /// A picture in the track's folder.
    File(PathBuf),
}

/// The cover of the track at `track`: its tags' front cover, else the first picture its tags
/// carry, else a `cover`, `folder`, `front` or `album` picture in its folder, whatever the case
/// of the name.
#[must_use]
pub fn find(track: &Path) -> Option<Cover> {
    embedded(track).map(Cover::Embedded).or_else(|| beside(track).map(Cover::File))
}

/// The cover of the track at `track`, decoded to at most `max` pixels; `None` when it has none
/// or it cannot be read as a picture.
#[must_use]
pub fn load(track: &Path, max: (u32, u32)) -> Option<ImageData> {
    match find(track)? {
        Cover::Embedded(bytes) => ImageData::decode_bytes(&bytes, max).ok(),
        Cover::File(path) => ImageData::decode_file(&path, max).ok(),
    }
}

/// Keeps a copy of the cover of the track at `track` in `cache` under `name`, for a desktop that
/// shows it beside the track's name, and gives where the copy is; `None` when the track has no
/// cover or the copy cannot be written. The copy is the cover's own bytes, never decoded again, and
/// one already kept is not written twice.
#[must_use]
pub fn keep(track: &Path, cache: &Path, name: &str) -> Option<PathBuf> {
    let (bytes, ending) = match find(track)? {
        Cover::Embedded(bytes) => {
            let ending = ending_of(&bytes)?;
            (bytes, ending.to_owned())
        }
        Cover::File(path) => {
            let ending = path.extension()?.to_str()?.to_ascii_lowercase();
            (fs::read(&path).ok()?, ending)
        }
    };
    keep_as(&bytes, &ending, cache, name)
}

/// Keeps a copy of the cover `bytes`, a picture an account sent, in `cache` under `name`, as
/// [`keep`] keeps a file's; `None` when they are not a picture a desktop can show or cannot be
/// written.
#[must_use]
pub fn keep_bytes(bytes: &[u8], cache: &Path, name: &str) -> Option<PathBuf> {
    keep_as(bytes, ending_of(bytes)?, cache, name)
}

/// Writes `bytes` to `cache` as `name` with `ending`, unless a copy is there already.
fn keep_as(bytes: &[u8], ending: &str, cache: &Path, name: &str) -> Option<PathBuf> {
    let kept = cache.join(format!("{name}.{ending}"));
    if kept.is_file() {
        return Some(kept);
    }
    fs::create_dir_all(cache).ok()?;
    qframe::storage::atomic_write(&kept, bytes).ok()?;
    Some(kept)
}

/// The name an album's kept cover goes by: the same for every track of the album, whichever run
/// asks, and different for two albums of the same name in two folders.
#[must_use]
pub fn name_of(folder: &Path, album: &str) -> String {
    // FNV-1a: a hash whose value no version of Rust changes, so a cover kept by one version of
    // qmus is found by the next.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let bytes = folder.as_os_str().as_encoded_bytes().iter().chain([0].iter()).chain(album.as_bytes());
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// The ending a picture's bytes say it should have, when they are one a desktop can show.
fn ending_of(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG") {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("webp")
    } else if bytes.starts_with(b"GIF8") {
        Some("gif")
    } else {
        None
    }
}

/// The picture the track's tags carry: the front cover first.
fn embedded(track: &Path) -> Option<Vec<u8>> {
    let file = lofty::read_from_path(track).ok()?;
    let pictures: Vec<_> = file.tags().iter().flat_map(lofty::tag::Tag::pictures).collect();
    let chosen =
        pictures.iter().find(|picture| picture.pic_type() == PictureType::CoverFront).or_else(|| pictures.first())?;
    Some(chosen.data().to_vec())
}

/// A cover picture in the track's folder.
fn beside(track: &Path) -> Option<PathBuf> {
    let folder = track.parent()?;
    let found: Vec<PathBuf> = fs::read_dir(folder).ok()?.flatten().map(|entry| entry.path()).collect();
    NAMES.iter().find_map(|name| {
        found
            .iter()
            .find(|path| {
                let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
                let ending = path.extension().and_then(|ending| ending.to_str()).unwrap_or_default();
                stem.eq_ignore_ascii_case(name) && ENDINGS.iter().any(|known| known.eq_ignore_ascii_case(ending))
            })
            .cloned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Scratch, sine_wav, solid_png, tagged_wav};

    #[test]
    fn a_cover_beside_the_track_is_found_whatever_the_case_of_its_name() {
        let scratch = Scratch::new("art-beside");
        let track = scratch.path("album/1.wav");
        sine_wav(&track, 8_000, 1, 0.1, 440.0);
        assert_eq!(find(&track), None);
        std::fs::write(scratch.path("album/notes.png"), solid_png([0, 0, 255])).expect("file");
        assert_eq!(find(&track), None, "only the usual names are covers");
        std::fs::write(scratch.path("album/Folder.JPG"), b"x").expect("file");
        std::fs::write(scratch.path("album/Cover.png"), solid_png([255, 0, 0])).expect("file");
        assert_eq!(find(&track), Some(Cover::File(scratch.path("album/Cover.png"))), "cover before folder");
        let image = load(&track, (64, 64)).expect("decoded");
        assert_eq!(image.pixel(0, 0).map(|rgb| (rgb.r, rgb.g, rgb.b)), Some((255, 0, 0)));
    }

    #[test]
    fn a_cover_is_kept_as_its_own_bytes_once_under_the_albums_name() {
        let scratch = Scratch::new("art-keep");
        let track = scratch.path("album/1.wav");
        sine_wav(&track, 8_000, 1, 0.1, 440.0);
        let cache = scratch.path("cache/art");
        assert_eq!(keep(&track, &cache, "x"), None, "no cover, nothing kept");
        let png = solid_png([0, 0, 255]);
        std::fs::write(scratch.path("album/Front.PNG"), &png).expect("file");
        let kept = keep(&track, &cache, "abc").expect("kept");
        assert_eq!(kept, cache.join("abc.png"));
        assert_eq!(std::fs::read(&kept).expect("read"), png, "the cover's own bytes");
        std::fs::write(&kept, b"left as it was").expect("file");
        assert_eq!(keep(&track, &cache, "abc"), Some(kept.clone()));
        assert_eq!(std::fs::read(&kept).expect("read"), b"left as it was", "not written twice");
    }

    #[test]
    fn two_albums_of_one_name_in_two_folders_keep_two_covers() {
        let one = name_of(Path::new("/m/a"), "Best Of");
        assert_eq!(one, name_of(Path::new("/m/a"), "Best Of"));
        assert_ne!(one, name_of(Path::new("/m/b"), "Best Of"));
        assert_ne!(one, name_of(Path::new("/m/a"), "Best Of 2"));
        assert_eq!(one.len(), 16);
    }

    #[test]
    fn a_cover_in_the_tags_comes_before_one_beside_the_track() {
        use lofty::config::WriteOptions;
        use lofty::picture::{MimeType, Picture};

        let scratch = Scratch::new("art-tags");
        let track = scratch.path("album/1.wav");
        tagged_wav(&track, 0.1, "Bir", "Kalben", "Sonsuz", 1);
        std::fs::write(scratch.path("album/cover.png"), solid_png([255, 0, 0])).expect("file");
        let mut tagged = lofty::read_from_path(&track).expect("read");
        let tag = tagged.primary_tag_mut().expect("the tag tagged_wav wrote");
        let back = Picture::unchecked(solid_png([0, 0, 255])).pic_type(PictureType::CoverBack).mime_type(MimeType::Png);
        let front =
            Picture::unchecked(solid_png([0, 255, 0])).pic_type(PictureType::CoverFront).mime_type(MimeType::Png);
        tag.push_picture(back.build());
        tag.push_picture(front.build());
        tagged.save_to_path(&track, WriteOptions::default()).expect("saved");
        let image = load(&track, (64, 64)).expect("decoded");
        assert_eq!(image.pixel(0, 0).map(|rgb| (rgb.r, rgb.g, rgb.b)), Some((0, 255, 0)), "the front cover");
    }
}
