//! What the tests share: a temporary folder removed when the test ends, and short sound files made
//! on the spot. No test reads the person's own music.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A temporary folder that is removed when the test ends, whatever happened in it.
pub(crate) struct Scratch {
    root: PathBuf,
}

impl Scratch {
    /// A new, empty folder named after `name`.
    pub(crate) fn new(name: &str) -> Self {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let folder = format!("qmus-{name}-{}-{}", std::process::id(), COUNT.fetch_add(1, Ordering::Relaxed));
        let root = std::env::temp_dir().join(folder);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("scratch folder");
        Self { root: fs::canonicalize(root).expect("scratch folder") }
    }

    /// The path of `relative` inside the folder, with the folders on the way made.
    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("folder");
        }
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Writes a 16-bit PCM WAV of `seconds` of a sine at `hertz`, at `rate` frames a second over
/// `channels` channels, at half of full scale.
pub(crate) fn sine_wav(path: &Path, rate: u32, channels: u16, seconds: f64, hertz: f64) {
    wav(path, rate, channels, seconds, hertz, None);
}

/// A sine WAV like [`sine_wav`] whose tone is only on channel `only`, the others silent.
pub(crate) fn sine_wav_on(path: &Path, rate: u32, channels: u16, seconds: f64, hertz: f64, only: u16) {
    wav(path, rate, channels, seconds, hertz, Some(only));
}

fn wav(path: &Path, rate: u32, channels: u16, seconds: f64, hertz: f64, only: Option<u16>) {
    let frames = (f64::from(rate) * seconds).round() as u32;
    let data = frames * u32::from(channels) * 2;
    let mut bytes = Vec::with_capacity(44 + data as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for frame in 0..frames {
        let phase = f64::from(frame) / f64::from(rate) * hertz * std::f64::consts::TAU;
        let sample = (phase.sin() * f64::from(i16::MAX) * 0.5) as i16;
        for channel in 0..channels {
            let sample = if only.is_none_or(|only| only == channel) { sample } else { 0 };
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
    }
    fs::write(path, bytes).expect("sound file");
}

/// A sine WAV like [`sine_wav`], with an ID3v2 tag naming its title, artist, album and track.
pub(crate) fn tagged_wav(path: &Path, seconds: f64, title: &str, artist: &str, album: &str, track: u32) {
    use lofty::config::WriteOptions;
    use lofty::prelude::*;
    use lofty::tag::{Tag, TagType};

    sine_wav(path, 8_000, 1, seconds, 440.0);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.to_owned());
    tag.set_artist(artist.to_owned());
    tag.set_album(album.to_owned());
    tag.set_track(track);
    tag.save_to_path(path, WriteOptions::default()).expect("tag written");
}

/// A 16 × 16 PNG of one colour, for a cover.
pub(crate) fn solid_png(rgb: [u8; 3]) -> Vec<u8> {
    let picture = image::RgbImage::from_pixel(16, 16, image::Rgb(rgb));
    let mut bytes = std::io::Cursor::new(Vec::new());
    picture.write_to(&mut bytes, image::ImageFormat::Png).expect("a PNG");
    bytes.into_inner()
}
