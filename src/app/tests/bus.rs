//! The screen driven from the session bus, as a media key, `playerctl` or a desktop's "now
//! playing" corner drives it. A socket pair stands in for the bus: the screen serves on one end,
//! the test asks on the other, and the person's own session bus is never touched.

use std::collections::HashMap;
use std::sync::Arc;

use zbus::zvariant::{ObjectPath, OwnedValue};

use super::*;
use crate::app::Msg;
use crate::library::{Location, SourceKey, Track};
use crate::mpris::tests::{Client, changes, ends, entries, text};

/// The screen showing `folder` on one end of a socket pair, and a client on the other, once the
/// screen has taken its place there.
fn served(scratch: &Scratch, folder: &Path) -> (Harness<Music>, Client) {
    let (socket, client) = ends();
    let machine = Machine { bus: Bus::Socket(Arc::new(socket)), ..machine(scratch) };
    let mut h = open_on(machine, folder, crate::locales::env(), 100);
    wait_for(&mut h, |music| music.mpris.is_some());
    (h, client.join().expect("the client end connects"))
}

/// Waits for the first signal `wanted` picks, passing over the others.
fn signal_where(client: &Client, wanted: impl Fn(&zbus::message::Message) -> bool) -> zbus::message::Message {
    let start = Instant::now();
    loop {
        assert!(start.elapsed() < GENEROUS, "the signal never came");
        let signal = client.next_signal();
        if wanted(&signal) {
            return signal;
        }
    }
}

/// Waits for the signal that says `property` is now `wanted`, passing over the others.
fn announced(client: &Client, property: &str, wanted: &str) {
    signal_where(client, |signal| {
        let named = signal.header().member().is_some_and(|member| member.as_str() == "PropertiesChanged");
        named && changes(signal).1.get(property).is_some_and(|value| text(value) == wanted)
    });
}

#[test]
fn a_media_key_holds_the_track_heard_and_the_bus_hears_that_it_is_held() {
    let scratch = Scratch::new("bus-pause");
    let folder = albums(&scratch);
    let (mut h, client) = served(&scratch, &folder);
    h.press("enter");
    wait_for(&mut h, |music| state(music) == State::Playing && music.status().position > Duration::ZERO);
    assert_eq!(client.get::<String>("PlaybackStatus"), "Playing");
    client.call("PlayPause").expect("the call is answered");
    wait_for(&mut h, |music| state(music) == State::Paused);
    // The player itself holds the sound, not just the screen's note of it.
    let held = h.app().player.status().position;
    std::thread::sleep(Duration::from_millis(300));
    assert!(h.app().player.status().position <= held + Duration::from_millis(60), "nothing is heard while held");
    announced(&client, "PlaybackStatus", "Paused");
    assert_eq!(client.get::<String>("PlaybackStatus"), "Paused");
}

#[test]
fn the_track_heard_is_on_the_bus_with_its_name_its_artist_and_its_file() {
    let scratch = Scratch::new("bus-metadata");
    let folder = albums(&scratch);
    let (mut h, client) = served(&scratch, &folder);
    h.press("enter");
    wait_for(&mut h, |music| state(music) == State::Playing);
    let metadata: HashMap<String, OwnedValue> = client.get("Metadata");
    assert_eq!(text(&metadata["xesam:title"]), "Uzun Yol");
    assert_eq!(text(&metadata["xesam:album"]), "Eski");
    let artists = <Vec<String>>::try_from(metadata["xesam:artist"].clone()).expect("a list of artists");
    assert_eq!(artists, ["Adamlar"]);
    let url = text(&metadata["xesam:url"]).to_owned();
    assert!(url.starts_with("file://") && url.ends_with("/music/eski/1.wav"), "{url}");
    let length = i64::try_from(&metadata["mpris:length"]).expect("a length in microseconds");
    assert!((2_900_000..3_100_000).contains(&length), "three seconds, not {length}");
}

#[test]
fn next_on_the_bus_goes_on_to_the_next_track_and_announces_it() {
    let scratch = Scratch::new("bus-next");
    let folder = albums(&scratch);
    let (mut h, client) = served(&scratch, &folder);
    h.press("enter");
    wait_for(&mut h, |music| state(music) == State::Playing);
    client.call("Next").expect("the call is answered");
    wait_for(&mut h, |music| music.current().is_some_and(|track| track.title != "Uzun Yol"));
    let next = h.app().current().expect("a track").title.clone();
    signal_where(&client, |signal| {
        let named = signal.header().member().is_some_and(|member| member.as_str() == "PropertiesChanged");
        named
            && changes(signal)
                .1
                .get("Metadata")
                .is_some_and(|metadata| entries(metadata).get("xesam:title").is_some_and(|title| text(title) == next))
    });
}

#[test]
fn a_volume_set_on_the_bus_is_the_volume_heard() {
    let scratch = Scratch::new("bus-volume");
    let folder = albums(&scratch);
    let (mut h, client) = served(&scratch, &folder);
    client.set("Volume", 0.4).expect("the volume is set");
    wait_for(&mut h, |music| music.volume == 40);
    let gain = h.app().player.gain();
    assert!((gain - 0.064).abs() < 1e-4, "forty out of a hundred on the cubic curve, not {gain}");
    assert!((client.get::<f64>("Volume") - 0.4).abs() < 1e-9);
}

#[test]
fn a_jump_from_the_bus_moves_the_sound_and_says_where_it_landed() {
    let scratch = Scratch::new("bus-seek");
    let folder = albums(&scratch);
    let (mut h, client) = served(&scratch, &folder);
    h.press("enter");
    wait_for(&mut h, |music| state(music) == State::Playing);
    h.press("p");
    wait_for(&mut h, |music| state(music) == State::Paused);
    let track: HashMap<String, OwnedValue> = client.get("Metadata");
    let id = ObjectPath::try_from(track["mpris:trackid"].clone()).expect("the track's path");
    client.call_with("SetPosition", &(id, 2_000_000_i64)).expect("the call is answered");
    wait_for(&mut h, |music| music.status().position >= Duration::from_secs(2));
    let seeked =
        signal_where(&client, |signal| signal.header().member().is_some_and(|member| member.as_str() == "Seeked"));
    let at: i64 = seeked.body().deserialize().expect("where it landed");
    assert!((2_000_000..2_100_000).contains(&at), "two seconds in, not {at}");
    assert!(h.app().player.status().position >= Duration::from_secs(2), "the player itself moved");
}

#[test]
fn a_file_from_outside_the_folder_is_not_played_and_the_screen_says_why() {
    let scratch = Scratch::new("bus-open");
    let folder = albums(&scratch);
    tagged_wav(&scratch.path("elsewhere/x.wav"), 0.4, "Başka", "Kimse", "Yok", 1);
    let (mut h, client) = served(&scratch, &folder);
    let url = format!("file://{}", scratch.path("elsewhere/x.wav").display());
    client.call_with("OpenUri", &(url,)).expect("the call is answered");
    let start = Instant::now();
    while !h.screen().contains("qmus plays the music of its own folder") {
        assert!(start.elapsed() < GENEROUS, "the screen never said why:\n{}", h.screen());
        std::thread::sleep(Duration::from_millis(20));
        h.advance(MOMENT);
    }
    assert_eq!(state(h.app()), State::Stopped);
}

#[test]
fn the_bus_names_the_kept_copy_of_the_albums_cover() {
    let scratch = Scratch::new("bus-art");
    let folder = albums(&scratch);
    std::fs::write(scratch.path("music/eski/cover.png"), crate::testing::solid_png([255, 0, 0])).expect("cover");
    let (mut h, client) = served(&scratch, &folder);
    h.press("enter");
    wait_for(&mut h, |music| music.art.as_ref().is_some_and(|art| art.read));
    let kept = h.app().art.as_ref().and_then(|art| art.file.clone()).expect("a copy for the desktop");
    let metadata: HashMap<String, OwnedValue> = client.get("Metadata");
    assert_eq!(text(&metadata["mpris:artUrl"]), format!("file://{}", kept.display()));
}

#[test]
fn an_accounts_track_is_on_the_bus_without_an_address_and_the_screen_goes_on_past_it() {
    let scratch = Scratch::new("bus-remote");
    let folder = albums(&scratch);
    let (mut h, client) = served(&scratch, &folder);
    let remote = Track {
        location: Location::Remote { source: SourceKey("navidrome-3f9a".into()), id: "tr-17".into() },
        title: "Uzak Yol".to_owned(),
        artist: "Adamlar".to_owned(),
        // Before "Eski" in the library's order, so it is the first row.
        album: "Ayrı".to_owned(),
        number: Some(1),
        duration: Some(Duration::from_secs(3)),
        playable: true,
    };
    let mut tracks = vec![remote];
    tracks.extend(h.app().tracks().iter().cloned());
    h.send(Msg::Scanned(tracks.into()));
    h.press("home");
    h.press("enter");
    let metadata: HashMap<String, OwnedValue> = client.get("Metadata");
    assert_eq!(text(&metadata["xesam:title"]), "Uzak Yol");
    assert!(!metadata.contains_key("xesam:url"), "an account's track names no address: {metadata:?}");
    // It cannot be played yet, so the next one is, as after any track that cannot be opened.
    wait_for(&mut h, |music| {
        music.current().is_some_and(|track| track.title == "Uzun Yol") && state(music) == State::Playing
    });
}
