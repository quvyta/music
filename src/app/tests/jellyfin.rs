//! A Jellyfin server added from the settings: logged in to with a password that is sent once and
//! never kept, its tracks listed beside the computer's, played, and their plays reported.

use super::accounts::{click_last, settings_of};
use super::*;
use crate::library::Location;
use crate::testing::jellyfin::{self, PASSWORD, Shelf, TOKEN};
use crate::testing::server::FakeServer;
use crate::testing::sine_wav;

/// A Jellyfin server of two short tracks of Kalben's "Ayrı".
fn server(scratch: &Scratch) -> FakeServer {
    let tone = scratch.path("served/tone.wav");
    sine_wav(&tone, 8_000, 1, 0.5, 440.0);
    jellyfin::start(Shelf {
        songs: vec![("j-1", "Gece Mavisi", "Kalben", "Ayrı", 1, 1), ("j-2", "Sabah Treni", "Kalben", "Ayrı", 2, 1)],
        playlists: Vec::new(),
        tone: std::fs::read(tone).expect("the tone"),
    })
}

/// Adds the Jellyfin server at `address` from the settings with `password`.
fn add(h: &mut Harness<Music>, address: &str, password: &str) {
    // Jellyfin's is the last kind that can be added.
    click_last(h, "Add account");
    settle(h);
    h.type_text("Ev Jellyfin");
    h.press("tab");
    h.type_text(address);
    // No way of logging in to choose: the user name follows the address.
    h.press("tab");
    h.type_text("hakan");
    h.press("tab");
    h.type_text(password);
    h.press("enter");
}

#[test]
fn a_jellyfin_server_added_lists_and_plays_its_tracks_and_the_password_is_kept_nowhere() {
    let scratch = Scratch::new("jellyfin-add");
    let server = server(&scratch);
    let mut h = settings_of(&scratch);
    add(&mut h, &server.url(), PASSWORD);
    wait_for(&mut h, |music| music.tracks().iter().any(|track| track.title == "Gece Mavisi"));
    let file = std::fs::read_to_string(scratch.path("config").join(crate::accounts::FILE)).expect("the accounts");
    assert!(file.contains("kind = \"jellyfin\"") && file.contains("Ev Jellyfin"), "{file}");
    assert!(!file.contains(PASSWORD) && !file.contains(TOKEN), "no login is written: {file}");
    let key = h.app().accounts[0].key.clone();
    assert!(key.starts_with("jellyfin-"), "{key}");
    h.press("esc");
    settle(&mut h);
    h.click_text("Ev Jellyfin");
    settle(&mut h);
    h.press("home");
    h.press("enter");
    let first = Location::Remote { source: crate::library::SourceKey(key.clone()), id: "j-1".to_owned() };
    wait_for(&mut h, |music| music.status().track == Some(first.clone()) && music.status().position > Duration::ZERO);
    wait_for(&mut h, |_| server.requests().iter().any(|request| request.path == "/Sessions/Playing"));
    let requests = server.requests();
    let login = requests.iter().filter(|request| request.body.contains(PASSWORD)).count();
    assert_eq!(login, 1, "the password went once, to log in");
    let device = &requests[0].headers["authorization"];
    assert!(device.contains(&format!("DeviceId=\"{key}\"")), "the server knows qmus by the account: {device}");
    assert!(
        requests.iter().any(|request| request.path == "/Audio/j-1/stream" && request.get("api_key") == Some(TOKEN))
    );
}

#[test]
fn a_wrong_jellyfin_password_is_said_in_the_dialog_and_nothing_is_kept() {
    let scratch = Scratch::new("jellyfin-refused");
    let server = server(&scratch);
    let mut h = settings_of(&scratch);
    add(&mut h, &server.url(), "yanlış");
    wait_for(&mut h, |music| music.account_dialog.as_ref().is_some_and(|dialog| dialog.problem.is_some()));
    assert!(h.screen().contains("did not accept this login"), "{}", h.screen());
    assert!(h.app().accounts.is_empty());
}
