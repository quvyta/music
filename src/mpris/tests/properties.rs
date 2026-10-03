//! What the properties on the bus say, read as a desktop or a `playerctl` would read them.

use std::collections::HashMap;
use std::time::Duration;

use zbus::zvariant::OwnedValue;

use super::{Pair, text};
use crate::mpris::{Loop, Now, Playback};

/// The sound files qmus plays, as the kinds of file the desktop knows them by.
const KINDS: [&str; 7] =
    ["audio/flac", "audio/mpeg", "audio/ogg", "audio/mp4", "audio/aac", "audio/x-wav", "audio/x-aiff"];

#[test]
fn qmus_says_who_it_is_on_the_interface_the_desktop_looks_at() {
    let pair = Pair::new();
    let identity: String = pair.client.identity("Identity");
    let desktop: String = pair.client.identity("DesktopEntry");
    let can_quit: bool = pair.client.identity("CanQuit");
    let can_raise: bool = pair.client.identity("CanRaise");
    let has_track_list: bool = pair.client.identity("HasTrackList");
    let schemes: Vec<String> = pair.client.identity("SupportedUriSchemes");
    let kinds: Vec<String> = pair.client.identity("SupportedMimeTypes");
    assert_eq!(identity, "qmus");
    assert_eq!(desktop, "quvyta-music");
    assert!(can_quit, "qmus can be asked to end");
    assert!(!can_raise, "a program in a terminal has no window to raise");
    assert!(!has_track_list, "the queue is qmus's own, not a track list on the bus");
    assert_eq!(schemes, vec!["file".to_owned()], "qmus opens the person's own files");
    for kind in KINDS {
        assert!(kinds.iter().any(|known| known == kind), "qmus plays {kind}");
    }
}

#[test]
fn qmus_plays_at_the_speed_of_the_file_and_answers_to_the_desktop() {
    let pair = Pair::new();
    let rate: f64 = pair.client.get("Rate");
    let slowest: f64 = pair.client.get("MinimumRate");
    let fastest: f64 = pair.client.get("MaximumRate");
    let can_play: bool = pair.client.get("CanPlay");
    let can_pause: bool = pair.client.get("CanPause");
    let can_seek: bool = pair.client.get("CanSeek");
    let can_control: bool = pair.client.get("CanControl");
    assert_eq!([rate, slowest, fastest], [1.0, 1.0, 1.0]);
    assert!([can_play, can_pause, can_seek, can_control].into_iter().all(|can| can));
}

#[test]
fn with_nothing_loaded_qmus_is_stopped_and_names_the_track_that_is_not_there() {
    let pair = Pair::new();
    let status: String = pair.client.get("PlaybackStatus");
    let metadata: HashMap<String, OwnedValue> = pair.client.get("Metadata");
    assert_eq!(status, "Stopped");
    let track = metadata.get("mpris:trackid").expect("the track that is not there");
    assert_eq!(text(track), "/org/mpris/MediaPlayer2/TrackList/NoTrack");
    assert_eq!(metadata.len(), 1, "nothing else is said about a track that is not there");
}

#[test]
fn what_is_heard_is_read_off_the_object_as_it_is_said() {
    let pair = Pair::new();
    pair.server.update(&Now {
        id: 12,
        title: "Bir Derdim Var".to_owned(),
        artists: vec!["mor ve ötesi".to_owned()],
        album: "Dünya Yalan Söylüyor".to_owned(),
        album_artists: vec!["mor ve ötesi".to_owned()],
        track_number: Some(3),
        length: Some(Duration::from_secs(252)),
        status: Playback::Paused,
        volume: 0.42,
        shuffle: true,
        loop_status: Loop::Playlist,
        can_next: true,
        can_previous: false,
        position: Duration::from_millis(61_500),
        ..Now::default()
    });
    let status: String = pair.client.get("PlaybackStatus");
    let volume: f64 = pair.client.get("Volume");
    let shuffle: bool = pair.client.get("Shuffle");
    let loop_status: String = pair.client.get("LoopStatus");
    let position: i64 = pair.client.get("Position");
    let can_next: bool = pair.client.get("CanGoNext");
    let can_previous: bool = pair.client.get("CanGoPrevious");
    assert_eq!(status, "Paused");
    assert_eq!(volume, 0.42);
    assert!(shuffle);
    assert_eq!(loop_status, "Playlist");
    assert_eq!(position, 61_500_000, "the place in the track is read in microseconds");
    assert!(can_next);
    assert!(!can_previous);
}

#[test]
fn the_place_in_the_track_and_whether_it_is_heard_are_not_for_a_client_to_say() {
    let pair = Pair::new();
    assert!(pair.client.set("Position", 5_000_000_i64).is_err(), "the sound does not jump by being told to");
    assert!(pair.client.set("PlaybackStatus", "Playing").is_err(), "qmus plays or holds on its own");
    assert!(pair.client.set("Metadata", HashMap::<String, OwnedValue>::new()).is_err());
    assert_eq!(pair.asked.take(), Vec::new());
}
