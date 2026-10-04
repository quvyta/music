//! A Spotify Premium account added from the settings: logged in with the browser, its liked songs
//! listed beside the computer's and played; an account that is not Premium is turned away.

use super::accounts::click_last;
use super::*;
use crate::library::Location;
use crate::testing::sine_wav;
use crate::testing::spotify::{self, CODE};

/// The settings, tall enough for the accounts, on a machine whose Spotify is `server`.
fn settings_with(scratch: &Scratch, server: &crate::testing::server::FakeServer) -> Harness<Music> {
    sine_wav(&scratch.path("served/tone.wav"), 8_000, 1, 0.4, 440.0);
    let machine =
        Machine { spotify: Some(spotify::setup(server, &scratch.path("served/tone.wav"))), ..machine(scratch) };
    let mut h = open_on(machine, &albums(scratch), crate::locales::env(), 110);
    h.resize(110, 60);
    click_icon(&mut h, "settings");
    settle(&mut h);
    h
}

/// Clicks into the box under "Address the browser ended on".
fn paste_box(h: &mut Harness<Music>) {
    let (x, y) = find(h, "Address the browser ended on").unwrap_or_else(|| panic!("no paste box:\n{}", h.screen()));
    h.click(x + 4, y + 1);
}

/// Starts the browser login from the Spotify row, then comes back the way a browser on another
/// machine does: the address it ended on, pasted.
fn log_in(h: &mut Harness<Music>, code: &str) {
    // Spotify's is the last kind that can be added.
    click_last(h, "Add account");
    settle(h);
    h.click_text("Log in with the browser");
    settle(h);
    let opened = h.opens().last().and_then(|open| open.target.clone()).expect("the login page went to the browser");
    let opened = opened.to_string_lossy().into_owned();
    let state = opened.split("state=").nth(1).expect("a state").split('&').next().expect("the state").to_owned();
    paste_box(h);
    h.type_text(&format!("http://127.0.0.1:8898/login?code={code}&state={state}"));
    h.press("enter");
}

#[test]
fn a_spotify_account_logged_in_with_the_browser_lists_and_plays_its_liked_songs() {
    let scratch = Scratch::new("spotify-add");
    let server = spotify::start("premium");
    let mut h = settings_with(&scratch, &server);
    log_in(&mut h, CODE);
    wait_for(&mut h, |music| music.tracks().iter().any(|track| track.title == "Gece Mavisi"));
    assert!(
        h.opens()
            .iter()
            .all(|open| open.target.as_ref().is_some_and(|target| target.to_string_lossy().contains("/authorize?")))
    );
    let key = h.app().accounts[0].key.clone();
    assert!(key.starts_with("spotify-"), "{key}");
    let file = std::fs::read_to_string(scratch.path("config").join(crate::accounts::FILE)).expect("the accounts");
    assert!(file.contains("kind = \"spotify\"") && !file.contains("erisim") && !file.contains("yenile"), "{file}");
    assert!(!h.screen().contains("Report plays"), "nothing to report to Spotify:\n{}", h.screen());
    h.press("esc");
    settle(&mut h);
    h.click_text("Spotify");
    settle(&mut h);
    h.press("home");
    h.press("enter");
    let first = Location::Remote { source: crate::library::SourceKey(key), id: "sp1".to_owned() };
    wait_for(&mut h, |music| music.status().track == Some(first.clone()) && music.status().position > Duration::ZERO);
}

#[test]
fn an_account_that_is_not_premium_is_said_to_be_one_and_is_not_kept() {
    let scratch = Scratch::new("spotify-free");
    let server = spotify::start("free");
    let mut h = settings_with(&scratch, &server);
    log_in(&mut h, CODE);
    wait_for(&mut h, |music| music.account_dialog.as_ref().is_some_and(|dialog| dialog.problem.is_some()));
    assert!(h.screen().contains("only Premium accounts play"), "{}", h.screen());
    assert!(h.app().accounts.is_empty());
}

#[test]
fn a_pasted_address_of_another_login_is_said_to_be_wrong() {
    let scratch = Scratch::new("spotify-wrong");
    let server = spotify::start("premium");
    let mut h = settings_with(&scratch, &server);
    click_last(&mut h, "Add account");
    settle(&mut h);
    h.click_text("Log in with the browser");
    settle(&mut h);
    paste_box(&mut h);
    h.type_text("http://127.0.0.1:8898/login?code=kod-42&state=baska");
    h.press("enter");
    settle(&mut h);
    assert!(h.screen().contains("not the address this login ended on"), "{}", h.screen());
    assert!(!server.requests().iter().any(|request| request.path == "/api/token"), "nothing was exchanged");
}
