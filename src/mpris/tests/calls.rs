//! What each call on the bus asks of qmus.

use std::time::Duration;

use zbus::zvariant::ObjectPath;

use super::Pair;
use crate::mpris::{Loop, Request};

#[test]
fn the_calls_that_play_give_their_own_request() {
    let pair = Pair::new();
    for member in ["PlayPause", "Play", "Pause", "Stop", "Next", "Previous"] {
        pair.client.call(member).expect("the call is answered");
    }
    assert_eq!(
        pair.asked.take(),
        vec![Request::PlayPause, Request::Play, Request::Pause, Request::Stop, Request::Next, Request::Previous]
    );
}

#[test]
fn asking_to_end_gives_a_request_to_end() {
    let pair = Pair::new();
    pair.client.identity_call("Quit").expect("the call is answered");
    assert_eq!(pair.asked.take(), vec![Request::Quit]);
}

#[test]
fn seeking_gives_the_offset_it_was_given_and_going_back_is_a_negative_one() {
    let pair = Pair::new();
    pair.client.call_with("Seek", &(5_000_000_i64,)).expect("the call is answered");
    pair.client.call_with("Seek", &(-2_500_000_i64,)).expect("the call is answered");
    assert_eq!(pair.asked.take(), vec![Request::SeekBy(5_000_000), Request::SeekBy(-2_500_000)]);
}

#[test]
fn going_to_a_place_in_the_track_being_heard_gives_that_place_in_that_track() {
    let pair = Pair::playing(7);
    let track: ObjectPath<'_> = "/org/quvyta/qmus/track/7".try_into().expect("a track of ours");
    pair.client.call_with("SetPosition", &(track, 90_000_000_i64)).expect("the call is answered");
    assert_eq!(pair.asked.take(), vec![Request::SetPosition { track: 7, position: Duration::from_secs(90) }]);
}

#[test]
fn going_to_a_place_in_another_track_is_ignored_as_the_standard_says() {
    let pair = Pair::playing(7);
    let other: ObjectPath<'_> = "/org/quvyta/qmus/track/8".try_into().expect("a track of ours");
    let elsewhere: ObjectPath<'_> = "/org/mpris/MediaPlayer2/TrackList/NoTrack".try_into().expect("the no-track path");
    pair.client.call_with("SetPosition", &(other.clone(), 1_000_000_i64)).expect("the call is answered");
    pair.client.call_with("SetPosition", &(elsewhere, 1_000_000_i64)).expect("the call is answered");
    assert_eq!(pair.asked.take(), Vec::new(), "a jump in a track that is not heard is not asked for");
}

#[test]
fn opening_a_file_of_this_machine_gives_its_path_as_it_is_written() {
    let pair = Pair::new();
    pair.client
        .call_with("OpenUri", &("file:///home/someone/M%C3%BCzik/A%C5%9Fk%20Bir%20Derdim%20Var.mp3",))
        .expect("the call is answered");
    assert_eq!(pair.asked.take(), vec![Request::Open("/home/someone/Müzik/Aşk Bir Derdim Var.mp3".into())]);
}

#[test]
fn opening_anything_that_is_not_a_file_of_this_machine_is_turned_away() {
    let pair = Pair::new();
    for uri in [
        "https://example.com/a-song.mp3",
        "http://example.com/stream",
        "file://elsewhere/song.mp3",
        "spotify:track:1234",
        "/home/someone/song.mp3",
    ] {
        assert!(pair.client.call_with("OpenUri", &(uri,)).is_err(), "qmus opens no {uri}");
    }
    assert_eq!(pair.asked.take(), Vec::new(), "a refused call asks qmus for nothing");
}

#[test]
fn the_three_writable_properties_give_their_own_request() {
    let pair = Pair::new();
    pair.client.set("Volume", 0.25_f64).expect("the volume is set");
    pair.client.set("Shuffle", true).expect("the queue is shuffled");
    pair.client.set("LoopStatus", "Track").expect("the queue repeats");
    assert_eq!(
        pair.asked.take(),
        vec![Request::SetVolume(0.25), Request::SetShuffle(true), Request::SetLoop(Loop::Track)]
    );
}

#[test]
fn repeating_the_queue_in_a_way_there_is_is_turned_away() {
    let pair = Pair::new();
    assert!(pair.client.set("LoopStatus", "Sideways").is_err());
    assert_eq!(pair.asked.take(), Vec::new());
}
