//! What qmus says on the bus when what it is playing changes.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use zbus::zvariant::OwnedValue;

use super::{Pair, changes, entries, names, number, text, yes};
use crate::mpris::{Now, Playback, Request};
use crate::testing::Scratch;

/// The track qmus is heard playing while a test watches the bus.
fn heard() -> Now {
    Now {
        id: 3,
        title: "Bir Derdim Var".to_owned(),
        artists: vec!["mor ve ötesi".to_owned()],
        album: "Dünya Yalan Söylüyor".to_owned(),
        album_artists: vec!["mor ve ötesi".to_owned()],
        track_number: Some(3),
        length: Some(Duration::from_secs(252)),
        status: Playback::Playing,
        volume: 0.8,
        can_next: true,
        ..Now::default()
    }
}

#[test]
fn a_track_that_starts_playing_is_announced_in_one_signal_with_what_changed() {
    let pair = Pair::new();
    pair.server.update(&heard());
    let (interface, changed, forgotten) = changes(&pair.client.next_signal());
    assert_eq!(interface, super::PLAYBACK);
    assert!(forgotten.is_empty(), "nothing stops being known");
    assert_eq!(names(&changed), ["CanGoNext", "Metadata", "PlaybackStatus", "Volume"]);
    assert_eq!(text(&changed["PlaybackStatus"]), "Playing");
    assert_eq!(number(&changed["Volume"]), 0.8);
    assert!(yes(&changed["CanGoNext"]));
    assert!(pair.client.quiet(), "one change is one signal");
}

#[test]
fn a_client_reads_the_new_track_off_the_signal_and_off_the_object_alike() {
    let pair = Pair::new();
    pair.server.update(&heard());
    let (_, changed, _) = changes(&pair.client.next_signal());
    let announced = entries(changed.get("Metadata").expect("the track is announced"));
    let read: HashMap<String, OwnedValue> = pair.client.get("Metadata");
    assert_eq!(text(announced.get("xesam:title").expect("the title")), "Bir Derdim Var");
    assert_eq!(text(announced.get("mpris:trackid").expect("the track")), "/org/quvyta/qmus/track/3");
    assert_eq!(i64::try_from(announced.get("mpris:length").expect("the length")).ok(), Some(252_000_000));
    assert_eq!(read, announced, "what the signal said and what the object says are one thing");
}

#[test]
fn only_the_properties_that_changed_are_announced_the_next_time() {
    let pair = Pair::new();
    pair.server.update(&heard());
    let _ = pair.client.next_signal();
    pair.server.update(&Now { volume: 0.2, ..heard() });
    let (interface, changed, forgotten) = changes(&pair.client.next_signal());
    assert_eq!(interface, super::PLAYBACK);
    assert_eq!(names(&changed), ["Volume"], "the rest of the track is as it was");
    assert!(forgotten.is_empty());
    assert!(pair.client.quiet());
}

#[test]
fn saying_the_same_thing_again_says_nothing_at_all() {
    let pair = Pair::new();
    pair.server.update(&heard());
    let _ = pair.client.next_signal();
    for _ in 0..5 {
        pair.server.update(&heard());
    }
    assert!(pair.client.quiet(), "the screen says what is true over and over; the bus hears it once");
}

#[test]
fn where_the_sound_stands_is_read_when_asked_and_never_announced() {
    let pair = Pair::new();
    let playing = Now { position: Duration::from_secs(10), ..heard() };
    pair.server.update(&playing);
    let (_, changed, _) = changes(&pair.client.next_signal());
    assert!(!changed.contains_key("Position"), "MPRIS asks for the place in the track to be read");
    pair.server.update(&Now { position: Duration::from_secs(75), ..playing });
    assert!(pair.client.quiet(), "the place in the track is read, never announced");
    let position: i64 = pair.client.get("Position");
    assert_eq!(position, 75_000_000, "and it is read in microseconds");
}

#[test]
fn a_jump_is_announced_so_that_a_client_counting_along_can_start_again() {
    let pair = Pair::playing(4);
    // The track that began playing was announced first, and the bus keeps the order it was given.
    let (interface, _, _) = changes(&pair.client.next_signal());
    assert_eq!(interface, super::PLAYBACK);
    pair.server.seeked(Duration::from_millis(31_250));
    let signal = pair.client.next_signal();
    assert_eq!(signal.header().interface().expect("the interface").as_str(), super::PLAYBACK);
    assert_eq!(signal.header().member().expect("the signal").as_str(), "Seeked");
    let position: i64 = signal.body().deserialize().expect("the signal carries the place jumped to");
    assert_eq!(position, 31_250_000, "the place is announced in microseconds");
    assert!(pair.client.quiet());
}

#[test]
fn a_cover_whose_name_has_spaces_and_other_letters_in_it_becomes_a_url_that_can_be_opened() {
    let scratch = Scratch::new("mpris-urls");
    let art = scratch.path("kapaklar/Dünya Yalan Söylüyor #1.png");
    let song = scratch.path("Müzik/mor ve ötesi/Bir Derdim Var.flac");
    for file in [&art, &song] {
        std::fs::create_dir_all(file.parent().expect("a folder")).expect("the folder is made");
        std::fs::write(file, b"not really what it says it is").expect("the file is written");
    }
    let folder = art.parent().and_then(Path::parent).expect("the scratch folder").display().to_string();
    let pair = Pair::new();
    pair.server.update(&Now { art: Some(art), path: Some(song.clone()), ..heard() });
    let (_, changed, _) = changes(&pair.client.next_signal());
    let metadata = entries(changed.get("Metadata").expect("the track is announced"));
    assert_eq!(
        text(metadata.get("mpris:artUrl").expect("the cover")),
        format!("file://{folder}/kapaklar/D%C3%BCnya%20Yalan%20S%C3%B6yl%C3%BCyor%20%231.png")
    );
    let url = text(metadata.get("xesam:url").expect("the file")).to_owned();
    assert!(url.starts_with("file:///"), "a file URL has no host: {url}");
    assert!(url.ends_with("/M%C3%BCzik/mor%20ve%20%C3%B6tesi/Bir%20Derdim%20Var.flac"), "{url}");
    pair.client.call_with("OpenUri", &(url.clone(),)).expect("the call is answered");
    assert_eq!(pair.asked.take(), vec![Request::Open(song)], "the URL names the same file again");
}
