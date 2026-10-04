//! An account's tracks played: fetched from the server as they are heard, one after the other with
//! no stop between, and passed over, with the reason, when they cannot be had.

use super::catalogue::{KEY, account_of_before, listed_from, log_in_again, ok, titles_shown};
use super::*;
use crate::library::Location;
use crate::testing::server::{FakeServer, Response};
use crate::testing::sine_wav;

/// A server listing two tracks of Kalben's "Ayrı", an album before the computer's in the library's
/// order, and sending a half-second tone for each, or answering `stream` with `refused`.
fn playing_server(scratch: &Scratch, refused: Option<u16>) -> FakeServer {
    server_playing(scratch, refused, 0.5, 1)
}

/// The same server, its tones `seconds` long and listed as `listed` seconds.
fn server_playing(scratch: &Scratch, refused: Option<u16>, seconds: f64, listed: u32) -> FakeServer {
    let tone = scratch.path("served/tone.wav");
    sine_wav(&tone, 8_000, 1, seconds, 440.0);
    let bytes = std::fs::read(tone).expect("the tone");
    FakeServer::start(move |request| match request.path.as_str() {
        "/rest/search3" if request.get("songOffset") == Some("0") => ok(&format!(
            r#""searchResult3":{{"song":[{},{}]}}"#,
            format_args!(
                r#"{{"id":"tr-1","title":"Gece Mavisi","artist":"Kalben","album":"Ayrı","track":1,"duration":{listed},"coverArt":"al-1"}}"#
            ),
            format_args!(
                r#"{{"id":"tr-2","title":"Sabah Treni","artist":"Kalben","album":"Ayrı","track":2,"duration":{listed},"coverArt":"al-1"}}"#
            ),
        )),
        "/rest/search3" => ok(r#""searchResult3":{}"#),
        "/rest/getPlaylists" => ok(r#""playlists":{"playlist":[{"id":"p1","name":"Yol","songCount":2}]}"#),
        "/rest/getPlaylist" => ok(concat!(
            r#""playlist":{"id":"p1","name":"Yol","entry":["#,
            r#"{"id":"tr-2","title":"Sabah Treni","artist":"Kalben","album":"Ayrı","track":2,"duration":1},"#,
            r#"{"id":"tr-1","title":"Gece Mavisi","artist":"Kalben","album":"Ayrı","track":1,"duration":1}"#,
            r#"]}"#
        )),
        "/rest/getCoverArt" => {
            Response::bytes(crate::testing::solid_png([255, 0, 0])).header("Content-Type", "image/png")
        }
        "/rest/stream" => match refused {
            Some(status) => Response::status(status),
            None => Response::bytes(bytes.clone()).header("Content-Type", "audio/wav"),
        },
        _ => ok(""),
    })
}

/// Shows only the account's tracks and plays the first.
fn play_first_of_account(h: &mut Harness<Music>) {
    h.click_text("Ev sunucusu");
    settle(h);
    h.press("home");
    h.press("enter");
}

/// Where the track `id` of the account is played from.
fn remote(id: &str) -> Location {
    Location::Remote { source: crate::library::SourceKey(KEY.to_owned()), id: id.to_owned() }
}

#[test]
fn an_accounts_track_is_heard_from_its_server_and_the_next_follows_with_no_stop() {
    let scratch = Scratch::new("remote-play");
    let server = playing_server(&scratch, None);
    let mut h = listed_from(&scratch, &server);
    play_first_of_account(&mut h);
    wait_for(&mut h, |music| music.status().track == Some(remote("tr-1")) && music.status().position > Duration::ZERO);
    assert_eq!(state(h.app()), State::Playing);
    wait_for(&mut h, |music| music.current().is_some_and(|track| track.title == "Sabah Treni"));
    assert_eq!(h.app().player.plays(), 1, "the second followed in the same sound; it was not started anew");
    wait_for(&mut h, |music| state(music) == State::Ended);
    let streamed: Vec<_> = server
        .requests()
        .iter()
        .filter(|request| request.path == "/rest/stream")
        .map(|r| r.get("id").map(str::to_owned))
        .collect();
    assert_eq!(streamed, [Some("tr-1".to_owned()), Some("tr-2".to_owned())], "each fetched once, in order");
}

#[test]
fn a_track_the_server_will_not_send_is_passed_over_with_the_reason() {
    let scratch = Scratch::new("remote-refused");
    let server = playing_server(&scratch, Some(500));
    let mut h = listed_from(&scratch, &server);
    h.click_text("Ev sunucusu");
    settle(&mut h);
    h.press("home");
    h.press("enter");
    wait_for(&mut h, |music| music.current().is_some_and(|track| track.title == "Sabah Treni"));
    assert!(h.screen().contains("Gece Mavisi could not be played"), "{}", h.screen());
}

#[test]
fn an_accounts_track_without_a_login_asks_for_one_and_the_queue_goes_on() {
    let scratch = Scratch::new("remote-login");
    let server = playing_server(&scratch, None);
    // A run before logged in and listed; this one has its listing and no password.
    drop(listed_from(&scratch, &server));
    let mut h = open(&scratch, &albums(&scratch));
    wait_for(&mut h, |music| music.tracks().iter().any(|track| track.title == "Gece Mavisi"));
    settle(&mut h);
    play_first_of_account(&mut h);
    wait_for(&mut h, |music| music.current().is_some_and(|track| track.title == "Sabah Treni"));
    let screen = h.screen();
    assert!(screen.contains("Log in to Ev sunucusu in the settings"), "{screen}");
    assert!(!server.requests().iter().any(|request| request.path == "/rest/stream"), "nothing was fetched");
}

#[test]
fn logging_in_again_lets_an_accounts_track_play() {
    let scratch = Scratch::new("remote-again");
    let server = playing_server(&scratch, None);
    account_of_before(&scratch, &server);
    let mut h = super::accounts::settings_of(&scratch);
    log_in_again(&mut h);
    wait_for(&mut h, |music| music.tracks().iter().any(|track| track.title == "Gece Mavisi"));
    settle(&mut h);
    play_first_of_account(&mut h);
    wait_for(&mut h, |music| music.status().track == Some(remote("tr-1")) && state(music) == State::Playing);
}

/// What the server was told of the tracks heard: each track and whether it was heard through.
fn reports(server: &FakeServer) -> Vec<(String, String)> {
    server
        .requests()
        .iter()
        .filter(|request| request.path == "/rest/scrobble")
        .map(|request| {
            (request.get("id").unwrap_or_default().to_owned(), request.get("submission").unwrap_or_default().to_owned())
        })
        .collect()
}

#[test]
fn the_server_is_told_a_track_began_and_was_heard_through() {
    let scratch = Scratch::new("remote-scrobble");
    let server = playing_server(&scratch, None);
    let mut h = listed_from(&scratch, &server);
    play_first_of_account(&mut h);
    wait_for(&mut h, |music| music.current().is_some_and(|track| track.title == "Sabah Treni"));
    wait_for(&mut h, |music| state(music) == State::Ended);
    wait_for(&mut h, |_| reports(&server).len() >= 4);
    // Each report goes on its own, so two of one moment may arrive in either order.
    let mut told = reports(&server);
    told.sort();
    let pairs = [("tr-1", "false"), ("tr-1", "true"), ("tr-2", "false"), ("tr-2", "true")];
    assert_eq!(told, pairs.map(|(id, done)| (id.to_owned(), done.to_owned())), "each begun and heard through, once");
}

#[test]
fn with_reporting_turned_off_in_the_settings_the_server_is_told_nothing() {
    let scratch = Scratch::new("remote-no-scrobble");
    let server = playing_server(&scratch, None);
    account_of_before(&scratch, &server);
    let mut h = super::accounts::settings_of(&scratch);
    h.click_text("Report plays");
    settle(&mut h);
    let file = std::fs::read_to_string(scratch.path("config").join(crate::accounts::FILE)).expect("the file");
    assert!(file.contains("scrobble = false"), "the choice is kept: {file}");
    log_in_again(&mut h);
    wait_for(&mut h, |music| music.tracks().iter().any(|track| track.title == "Gece Mavisi"));
    play_first_of_account(&mut h);
    wait_for(&mut h, |music| {
        music.current().is_some_and(|track| track.title == "Sabah Treni") && state(music) == State::Ended
    });
    assert!(reports(&server).is_empty(), "nothing was reported: {:?}", reports(&server));
}

#[test]
fn an_accounts_album_cover_is_shown_and_kept_for_the_desktop() {
    let scratch = Scratch::new("remote-cover");
    let server = playing_server(&scratch, None);
    let mut h = listed_from(&scratch, &server);
    play_first_of_account(&mut h);
    wait_for(&mut h, |music| music.art.as_ref().is_some_and(|art| art.read));
    let art = h.app().art.clone().expect("the cover");
    assert!(art.image.is_some(), "the server's picture is read");
    let kept = art.file.expect("a copy for the desktop");
    assert_eq!(std::fs::read(kept).expect("the copy"), crate::testing::solid_png([255, 0, 0]));
    h.press("alt+1");
    settle(&mut h);
    let red = qframe::color::Rgb::new(255, 0, 0);
    let shown =
        (1..40).any(|row| (0..60).any(|column| h.fg(column, row) == Some(red) || h.bg(column, row) == Some(red)));
    assert!(shown, "the cover is on the page:\n{}", h.screen());
}

#[test]
fn half_a_track_heard_counts_before_it_ends() {
    let scratch = Scratch::new("remote-half");
    let server = server_playing(&scratch, None, 3.0, 3);
    let mut h = listed_from(&scratch, &server);
    play_first_of_account(&mut h);
    wait_for(&mut h, |_| reports(&server).contains(&("tr-1".to_owned(), "true".to_owned())));
    let status = h.app().player.status();
    assert_eq!(status.track, Some(remote("tr-1")), "still the first track: {status:?}");
    assert!(status.position < Duration::from_millis(2_900), "told before its end: {status:?}");
}

#[test]
fn a_queue_of_an_accounts_tracks_kept_as_a_playlist_lists_them_again() {
    let scratch = Scratch::new("remote-playlist");
    let server = playing_server(&scratch, None);
    let mut h = listed_from(&scratch, &server);
    // Adamlar's track first, then the queue goes on into the server's.
    h.press("home");
    h.press("enter");
    settle(&mut h);
    h.press("ctrl+s");
    settle(&mut h);
    super::playlists::name_it(&mut h, "Karışık");
    h.press("enter");
    settle(&mut h);
    let written = std::fs::read_to_string(scratch.path("data/playlists/Karışık.m3u8")).expect("the playlist");
    assert!(
        written.contains(&format!("qmus://{KEY}/tr-1")) && written.contains("#EXTINF:1,Kalben - Gece Mavisi"),
        "{written}"
    );
    h.press("alt+6");
    settle(&mut h);
    h.press("enter");
    settle(&mut h);
    let screen = h.screen();
    assert!(
        screen.contains("Gece Mavisi") && screen.contains("Sabah Treni") && screen.contains("Uzun Yol"),
        "{screen}"
    );
}

#[test]
fn an_accounts_playlist_opens_in_its_order_and_is_copied_into_one_of_qmuss_without_writing_over() {
    let scratch = Scratch::new("remote-lists");
    let server = playing_server(&scratch, None);
    let mut h = listed_from(&scratch, &server);
    h.press("alt+6");
    wait_for(&mut h, |music| !music.shelf.remote.is_empty());
    assert!(h.screen().contains("Yol · Ev sunucusu"), "{}", h.screen());
    h.press("home");
    h.press("enter");
    wait_for(&mut h, |music| music.shelf.open.is_some());
    settle(&mut h);
    assert_eq!(titles_shown(&h), ["Sabah Treni", "Gece Mavisi"], "the playlist's own order:\n{}", h.screen());
    h.click_text("Copy to a qmus playlist");
    settle(&mut h);
    h.press("enter");
    settle(&mut h);
    let file = scratch.path("data/playlists/Yol.m3u8");
    let written = std::fs::read_to_string(&file).expect("the copy");
    let lines: Vec<&str> = written.lines().filter(|line| !line.starts_with('#')).collect();
    assert_eq!(lines, [format!("qmus://{KEY}/tr-2"), format!("qmus://{KEY}/tr-1")], "{written}");
    h.click_text("Copy to a qmus playlist");
    settle(&mut h);
    h.press("enter");
    settle(&mut h);
    assert!(h.screen().contains("A playlist of that name is already there"), "{}", h.screen());
    assert_eq!(std::fs::read_to_string(&file).expect("the copy"), written, "nothing is written over");
}

#[test]
fn find_on_another_source_searches_the_other_one_for_the_track_and_plays_nothing() {
    let scratch = Scratch::new("remote-elsewhere");
    // The server has a track of the same name as one of the computer's.
    let server = FakeServer::start(|request| match request.path.as_str() {
        "/rest/search3" if request.get("songOffset") == Some("0") => ok(concat!(
            r#""searchResult3":{"song":["#,
            r#"{"id":"tr-9","title":"Aşk İçinde","artist":"Kalben","album":"Canlı","track":1,"duration":200},"#,
            r#"{"id":"tr-1","title":"Gece Mavisi","artist":"Kalben","album":"Ayrı","track":1,"duration":187}"#,
            r#"]}"#
        )),
        "/rest/search3" => ok(r#""searchResult3":{}"#),
        _ => ok(""),
    });
    let mut h = listed_from(&scratch, &server);
    h.click_text("This computer");
    settle(&mut h);
    super::menu::menu_of(&mut h, "Aşk İçinde");
    super::menu::choose(&mut h, "Find on another source");
    let screen = h.screen();
    assert!(screen.contains("Ev sunucusu · 1"), "the server's own, found:\n{screen}");
    assert_eq!(h.app().list.len(), 1, "one track found:\n{screen}");
    let shown = h.app().list.first().map(|row| h.app().tracks()[*row].location.clone());
    assert_eq!(shown, Some(remote("tr-9")), "the server's track, not the computer's");
    assert_eq!(h.app().status().track, None, "nothing was played");
}
