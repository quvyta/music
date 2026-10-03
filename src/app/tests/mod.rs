//! Screen tests. Every test reads music made on the spot in a temporary folder, keeps its settings
//! in that folder too and plays into the silent output, so nothing reaches the person's files,
//! their Quvyta settings or their speakers. The harness answers the update question itself.

use std::path::Path;
use std::time::{Duration, Instant};

use qframe::icons::GlyphMode;
use qframe::prelude::*;

use super::{Bus, Machine, Music, Opening, UpdateFolders};
use crate::audio::{AudioOut, State};
use crate::testing::{Scratch, tagged_wav};

mod bus;
mod menu;
mod now_playing;
mod pages;
mod player;
mod playlists;
mod screen;
mod settings;
mod visualizer;

/// Long enough for background work to come back.
const MOMENT: Duration = Duration::from_millis(300);

/// Long enough for any of these short tracks to be heard on a loaded machine.
const GENEROUS: Duration = Duration::from_secs(20);

/// A folder of three short tagged tracks: two of one album, one of another.
fn albums(scratch: &Scratch) -> std::path::PathBuf {
    tagged_wav(&scratch.path("music/sonsuz/1.wav"), 0.4, "Aşk İçinde", "Kalben", "Sonsuz", 1);
    tagged_wav(&scratch.path("music/sonsuz/2.wav"), 0.4, "Haydi Söyle", "Kalben", "Sonsuz", 2);
    tagged_wav(&scratch.path("music/eski/1.wav"), 3.0, "Uzun Yol", "Adamlar", "Eski", 1);
    scratch.path("music")
}

/// A machine whose settings live in the test's own folder and whose sound goes nowhere.
fn machine(scratch: &Scratch) -> Machine {
    let config = scratch.path("config/x").parent().expect("folder").to_path_buf();
    Machine {
        config: Some(config.clone()),
        updates: Some(UpdateFolders { config, state: scratch.path("state") }),
        audio: AudioOut::Null,
        home: Some(scratch.path("x").parent().expect("folder").to_path_buf()),
        state: Some(scratch.path("state")),
        bus: Bus::Nowhere,
        covers: Some(scratch.path("cache/art")),
        playlists: Some(scratch.path("data/playlists")),
    }
}

/// The screen showing `folder`, in English with Unicode glyphs, 100 × 20.
fn open(scratch: &Scratch, folder: &Path) -> Harness<Music> {
    open_on(machine(scratch), folder, crate::locales::env(), 100)
}

/// The screen on `machine` showing `folder`, with `env`, `width` × 20.
fn open_on(machine: Machine, folder: &Path, env: qframe::env::Env, width: u16) -> Harness<Music> {
    let opening = Opening::new(machine, folder.to_path_buf());
    let mut h = Harness::with_env(opening.music, env, width, 20);
    h.set_locale("en").set_glyph_mode(GlyphMode::Unicode).set_reduced_motion(true);
    settle(&mut h);
    h
}

/// Lets background work come back.
fn settle(h: &mut Harness<Music>) {
    for _ in 0..4 {
        h.advance(MOMENT);
    }
}

/// Lets real time pass, reading the player on its beat, until `what` holds.
fn wait_for(h: &mut Harness<Music>, what: impl Fn(&Music) -> bool) {
    let start = Instant::now();
    while !what(h.app()) {
        assert!(start.elapsed() < GENEROUS, "the screen never got there: {:?}\n{}", h.app(), h.screen());
        std::thread::sleep(Duration::from_millis(20));
        h.advance(super::TICK);
    }
}

/// Where `text` first shows on screen, as a cell to click.
fn find(h: &Harness<Music>, text: &str) -> Option<(i32, i32)> {
    h.screen().lines().enumerate().find_map(|(row, line)| {
        let at = line.find(text)?;
        let column = line[..at].chars().count();
        Some((i32::try_from(column).ok()?, i32::try_from(row).ok()?))
    })
}

/// Clicks the icon button drawn with the glyph `key`.
fn click_icon(h: &mut Harness<Music>, key: &str) {
    let glyph = h.env().icons().glyph(key).into_owned();
    let (x, y) = find(h, &glyph).unwrap_or_else(|| panic!("no {key} button:\n{}", h.screen()));
    h.click(x, y);
}

/// The player's state as the screen last read it.
fn state(music: &Music) -> State {
    music.status().state
}
