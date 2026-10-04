//! An account's music in the library, beside the files of this computer: asked for once logged in
//! to, kept for the next start, faint while the account cannot be reached, forgotten with it.

use super::accounts::{PASSWORD, click_first, click_last, fill_in, settings_of};
use super::*;
use crate::testing::server::{FakeServer, Response};

/// The key of the account the tests write by hand.
pub(super) const KEY: &str = "subsonic-1a2b";

/// An answer of the API that went well, with `inner` in it.
pub(super) fn ok(inner: &str) -> Response {
    let comma = if inner.is_empty() { "" } else { "," };
    Response::json(&format!(r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1"{comma}{inner}}}}}"#))
}

/// A server whose catalogue is two tracks of Kalben's "Sonsuz", an album the computer has as well;
/// with `listing` false, it takes the login but cannot list.
fn home_server(listing: bool) -> FakeServer {
    server_of(listing, "Sonsuz")
}

/// A server whose catalogue is two tracks of Kalben's `album`.
fn server_of(listing: bool, album: &'static str) -> FakeServer {
    FakeServer::start(move |request| match request.path.as_str() {
        "/rest/search3" if !listing => Response::status(500),
        "/rest/search3" if request.get("songOffset") == Some("0") => ok(&format!(
            r#""searchResult3":{{"song":[{},{}]}}"#,
            format_args!(
                r#"{{"id":"tr-1","title":"Gece Mavisi","artist":"Kalben","album":"{album}","track":3,"duration":187}}"#
            ),
            format_args!(
                r#"{{"id":"tr-2","title":"Sabah Treni","artist":"Kalben","album":"{album}","track":4,"duration":201}}"#
            ),
        )),
        "/rest/search3" => ok(r#""searchResult3":{}"#),
        _ => ok(""),
    })
}

/// Writes the accounts file of a run before, naming `server`.
pub(super) fn account_of_before(scratch: &Scratch, server: &FakeServer) {
    std::fs::create_dir_all(scratch.path("config")).expect("folder");
    let file = format!(
        "[[account]]\nkey = \"{KEY}\"\nkind = \"subsonic\"\nname = \"Ev sunucusu\"\naddress = \"{}\"\nuser = \"hakan\"\n",
        server.url()
    );
    std::fs::write(scratch.path("config").join(crate::accounts::FILE), file).expect("written");
}

/// Logs in to the account of the run before from the settings, and closes them.
pub(super) fn log_in_again(h: &mut Harness<Music>) {
    click_last(h, "Log in");
    settle(h);
    h.type_text(PASSWORD);
    h.press("enter");
    wait_for(h, |music| music.account_dialog.is_none());
    h.press("esc");
    settle(h);
}

/// Where the listing of the account `key` is kept.
fn listing_of(scratch: &Scratch, key: &str) -> std::path::PathBuf {
    crate::sources::cache::file_of(&scratch.path("cache/sources"), key)
}

/// Where the listing of the account written by hand is kept.
fn listing(scratch: &Scratch) -> std::path::PathBuf {
    listing_of(scratch, KEY)
}

/// The colour `title` is written in on the screen.
fn colour_of(h: &Harness<Music>, title: &str) -> Option<qframe::color::Rgb> {
    let (x, y) = find(h, title).unwrap_or_else(|| panic!("{title} is listed:\n{}", h.screen()));
    h.fg(u16::try_from(x).expect("x"), u16::try_from(y).expect("y"))
}

#[test]
fn a_server_added_lists_its_tracks_beside_the_computers_and_keeps_them_for_the_next_start() {
    let scratch = Scratch::new("catalogue-add");
    let server = home_server(true);
    let mut h = settings_of(&scratch);
    click_first(&mut h, "Add account");
    settle(&mut h);
    fill_in(&mut h, "Ev sunucusu", &server.url(), PASSWORD);
    wait_for(&mut h, |music| music.tracks().iter().any(|track| track.title == "Gece Mavisi"));
    h.press("esc");
    settle(&mut h);
    let screen = h.screen();
    assert!(screen.contains("Gece Mavisi") && screen.contains("Sabah Treni"), "the server's tracks:\n{screen}");
    assert!(screen.contains("Aşk İçinde") && screen.contains("Uzun Yol"), "and the computer's:\n{screen}");
    let file = listing_of(&scratch, &h.app().accounts[0].key);
    let kept = std::fs::read_to_string(file).expect("what it listed is kept");
    assert!(!kept.contains(PASSWORD), "the listing holds no login: {kept}");
}

#[test]
fn a_start_shows_what_the_account_listed_before_without_a_login_or_a_question() {
    let scratch = Scratch::new("catalogue-start");
    let server = home_server(true);
    account_of_before(&scratch, &server);
    {
        let mut first = settings_of(&scratch);
        log_in_again(&mut first);
        wait_for(&mut first, |music| music.tracks().iter().any(|track| track.title == "Gece Mavisi"));
    }
    let asked = server.requests().len();
    let mut h = open(&scratch, &albums(&scratch));
    wait_for(&mut h, |music| music.tracks().iter().any(|track| track.title == "Gece Mavisi"));
    assert!(h.screen().contains("Gece Mavisi"), "{}", h.screen());
    assert_eq!(h.app().tracks().len(), 5, "three of the computer's and two of the server's");
    assert_eq!(server.requests().len(), asked, "nothing was asked of the server before a login");
}

#[test]
fn an_account_that_cannot_list_keeps_its_tracks_faint() {
    let scratch = Scratch::new("catalogue-faint");
    let good = home_server(true);
    account_of_before(&scratch, &good);
    {
        let mut first = settings_of(&scratch);
        log_in_again(&mut first);
        wait_for(&mut first, |music| music.tracks().iter().any(|track| track.title == "Gece Mavisi"));
    }
    // The same account, now at a server that takes the login and fails to list.
    let failing = home_server(false);
    account_of_before(&scratch, &failing);
    let mut h = settings_of(&scratch);
    wait_for(&mut h, |music| music.tracks().len() == 5);
    h.press("esc");
    settle(&mut h);
    assert_eq!(colour_of(&h, "Gece Mavisi"), colour_of(&h, "Haydi Söyle"), "reachable as far as known");
    click_icon(&mut h, "settings");
    settle(&mut h);
    log_in_again(&mut h);
    wait_for(&mut h, |music| matches!(music.standing.get(KEY), Some(crate::app::accounts::Standing::Failed(_))));
    settle(&mut h);
    assert!(h.screen().contains("Gece Mavisi"), "what was listed stays:\n{}", h.screen());
    assert_ne!(colour_of(&h, "Gece Mavisi"), colour_of(&h, "Haydi Söyle"), "faint:\n{}", h.screen());
}

#[test]
fn a_removed_account_takes_its_tracks_and_its_listing_with_it() {
    let scratch = Scratch::new("catalogue-remove");
    let server = home_server(true);
    account_of_before(&scratch, &server);
    let mut h = settings_of(&scratch);
    click_last(&mut h, "Log in");
    settle(&mut h);
    h.type_text(PASSWORD);
    h.press("enter");
    wait_for(&mut h, |music| music.tracks().iter().any(|track| track.title == "Gece Mavisi"));
    assert!(listing(&scratch).exists());
    click_last(&mut h, "Remove");
    settle(&mut h);
    // The dialog's own Remove, below the row's.
    click_last(&mut h, "Remove");
    settle(&mut h);
    h.press("esc");
    settle(&mut h);
    assert!(!h.screen().contains("Gece Mavisi"), "{}", h.screen());
    assert_eq!(h.app().tracks().len(), 3, "the computer's tracks stay");
    assert!(!listing(&scratch).exists(), "the listing is forgotten");
}

#[test]
fn an_album_of_the_same_name_on_the_computer_and_the_server_is_two_albums() {
    let scratch = Scratch::new("catalogue-albums");
    let server = home_server(true);
    account_of_before(&scratch, &server);
    let mut h = settings_of(&scratch);
    log_in_again(&mut h);
    wait_for(&mut h, |music| music.tracks().iter().any(|track| track.title == "Gece Mavisi"));
    h.press("alt+4");
    settle(&mut h);
    let screen = h.screen();
    let rows = screen.lines().filter(|line| line.contains("Sonsuz") && line.contains("Kalben")).count();
    assert_eq!(rows, 2, "the computer's and the server's, each whole:\n{screen}");
}

/// The screen with the account of the run before logged in to and its tracks listed, at `server`.
pub(super) fn listed_from(scratch: &Scratch, server: &FakeServer) -> Harness<Music> {
    account_of_before(scratch, server);
    let mut h = settings_of(scratch);
    log_in_again(&mut h);
    wait_for(&mut h, |music| music.tracks().iter().any(|track| track.title == "Gece Mavisi"));
    settle(&mut h);
    h
}

/// The titles the track table shows, top to bottom.
pub(super) fn titles_shown(h: &Harness<Music>) -> Vec<String> {
    let all = ["Aşk İçinde", "Haydi Söyle", "Uzun Yol", "Gece Mavisi", "Sabah Treni"];
    let screen = h.screen();
    screen
        .lines()
        .filter_map(|line| all.iter().find(|title| line.contains(*title)).map(|title| (*title).to_owned()))
        .filter(|title| !screen.lines().last().is_some_and(|bar| bar.contains(title.as_str())))
        .collect()
}

#[test]
fn the_source_picker_shows_one_sources_music_and_is_not_there_without_an_account() {
    let scratch = Scratch::new("catalogue-picker");
    let bare = open(&scratch, &albums(&scratch));
    assert!(!bare.screen().contains("This computer"), "no picker without an account:\n{}", bare.screen());
    drop(bare);
    let server = home_server(true);
    let mut h = listed_from(&scratch, &server);
    h.click_text("Ev sunucusu");
    settle(&mut h);
    assert_eq!(titles_shown(&h), ["Gece Mavisi", "Sabah Treni"], "{}", h.screen());
    h.click_text("This computer");
    settle(&mut h);
    assert_eq!(titles_shown(&h), ["Uzun Yol", "Aşk İçinde", "Haydi Söyle"], "{}", h.screen());
    h.press("alt+4");
    settle(&mut h);
    let rows = h.screen().lines().filter(|line| line.contains("Sonsuz") && line.contains("Kalben")).count();
    assert_eq!(rows, 1, "only the computer's album:\n{}", h.screen());
}

#[test]
fn a_search_is_grouped_by_source_and_the_picker_counts_what_each_finds() {
    let scratch = Scratch::new("catalogue-search");
    // An album that comes before "Sonsuz", so only the grouping puts the computer's first.
    let server = server_of(true, "Ayrı");
    let mut h = listed_from(&scratch, &server);
    h.press("/");
    h.type_text("kalben");
    settle(&mut h);
    let screen = h.screen();
    assert!(screen.contains("This computer · 2") && screen.contains("Ev sunucusu · 2"), "{screen}");
    assert_eq!(titles_shown(&h), ["Aşk İçinde", "Haydi Söyle", "Gece Mavisi", "Sabah Treni"], "{screen}");
}

#[test]
fn beside_an_account_each_title_says_where_it_is_from() {
    let scratch = Scratch::new("catalogue-marks");
    let server = home_server(true);
    let h = listed_from(&scratch, &server);
    let (local, remote) =
        (h.env().icons().glyph("folder-music").into_owned(), h.env().icons().glyph("category-network").into_owned());
    let screen = h.screen();
    let line = |title: &str| screen.lines().find(|line| line.contains(title)).unwrap_or_default().to_owned();
    assert!(line("Uzun Yol").contains(&local) && !line("Uzun Yol").contains(&remote), "{screen}");
    assert!(line("Gece Mavisi").contains(&remote), "{screen}");
}
