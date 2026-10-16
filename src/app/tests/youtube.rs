//! YouTube Music through the person's own yt-dlp: offered only when there is one, its liked music
//! listed with the browser's login and played, and a track kept whole asked of yt-dlp no more.

use super::accounts::{click_last, settings_of};
use super::*;
use crate::library::Location;
use crate::testing::server::{FakeServer, Response};
use crate::testing::sine_wav;
use crate::testing::ytdlp::{Fake, LIKED, Way, fake};

/// A server sending a short tone for every track's sound, and a stand-in yt-dlp that names it.
fn stand_in(scratch: &Scratch, way: Way) -> (FakeServer, Fake) {
    sine_wav(&scratch.path("served/tone.wav"), 8_000, 1, 1.0, 440.0);
    let bytes = std::fs::read(scratch.path("served/tone.wav")).expect("the tone");
    let server = FakeServer::start(move |_| Response::bytes(bytes.clone()).header("Content-Type", "audio/mp4"));
    let program = fake(scratch, LIKED, &format!("{}/audio/aaaa.m4a", server.url()), way);
    (server, program)
}

/// The settings, tall enough for the accounts, on a machine with the stand-in yt-dlp.
fn settings_with(scratch: &Scratch, ytdlp: &Fake) -> Harness<Music> {
    let machine = Machine { ytdlp: Some(ytdlp.program.clone()), ..machine(scratch) };
    let mut h = open_on(machine, &albums(scratch), crate::locales::test_env(), 110);
    h.resize(110, 60);
    click_icon(&mut h, "settings");
    settle(&mut h);
    h
}

/// Adds YouTube Music with the browser named `browser`.
fn add(h: &mut Harness<Music>, browser: &str) {
    // YouTube Music's is the last kind that can be added.
    click_last(h, "Add account");
    settle(h);
    h.click_text(browser);
    h.click_text("Connect");
}

#[test]
fn built_without_youtube_music_it_is_not_offered() {
    let scratch = Scratch::new("youtube-none");
    let h = settings_of(&scratch);
    assert!(!h.screen().contains("YouTube Music"), "{}", h.screen());
}

/// Puts a program called `name` in the scratch folder `bin`, which is never run.
fn program(scratch: &Scratch, name: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = scratch.path(&format!("bin/{name}"));
    std::fs::write(&path, "#!/bin/sh\nexit 99\n").expect("a program");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("its mode");
}

/// The settings on a machine that offers YouTube Music, looking for programs only in the scratch
/// folder `bin`, which holds no yt-dlp yet.
fn settings_without_ytdlp(scratch: &Scratch) -> Harness<Music> {
    std::fs::create_dir_all(scratch.path("bin")).expect("the folder");
    let machine = Machine { youtube_search: Some(scratch.path("bin").into_os_string()), ..machine(scratch) };
    let mut h = open_on(machine, &albums(scratch), crate::locales::test_env(), 110);
    h.resize(110, 60);
    click_icon(&mut h, "settings");
    settle(&mut h);
    h
}

#[test]
fn without_yt_dlp_youtube_music_is_still_offered_with_the_command_that_installs_it() {
    let scratch = Scratch::new("youtube-missing");
    program(&scratch, "pacman");
    let mut h = settings_without_ytdlp(&scratch);
    let screen = h.screen();
    assert!(screen.contains("YouTube Music") && screen.contains("not installed"), "{screen}");
    h.click_text("How to install");
    settle(&mut h);
    let shown = h.screen();
    assert!(shown.contains("pacman -S --needed yt-dlp"), "the package manager's own command:\n{shown}");
    assert!(h.handoffs().is_empty(), "nothing is run");
    // The person installs it in a terminal of their own, then asks qmus to look again.
    program(&scratch, "yt-dlp");
    h.click_text("Look again");
    settle(&mut h);
    assert_eq!(h.app().machine.ytdlp, Some(scratch.path("bin/yt-dlp")));
    let found = h.screen();
    assert!(!found.contains("pacman -S"), "the window closed:\n{found}");
    assert!(!found.contains("not installed"), "the row offers to add it now:\n{found}");
    assert!(h.handoffs().is_empty(), "still nothing run");
}

#[test]
fn looking_again_before_yt_dlp_is_installed_says_so_and_a_machine_with_no_package_manager_names_yt_dlps_page() {
    let scratch = Scratch::new("youtube-no-manager");
    let mut h = settings_without_ytdlp(&scratch);
    h.click_text("How to install");
    settle(&mut h);
    assert!(h.screen().contains("github.com/yt-dlp/yt-dlp"), "{}", h.screen());
    h.click_text("Look again");
    settle(&mut h);
    assert!(h.screen().contains("still not found"), "{}", h.screen());
    assert!(h.app().machine.ytdlp.is_none());
}

#[test]
fn yt_dlp_installed_while_the_settings_were_closed_is_found_when_they_open() {
    let scratch = Scratch::new("youtube-later");
    let mut h = settings_without_ytdlp(&scratch);
    assert!(h.screen().contains("not installed"), "{}", h.screen());
    h.press("esc");
    settle(&mut h);
    program(&scratch, "yt-dlp");
    click_icon(&mut h, "settings");
    settle(&mut h);
    assert!(h.screen().contains("YouTube Music") && !h.screen().contains("not installed"), "{}", h.screen());
}

#[test]
fn the_liked_music_is_listed_with_the_browsers_login_and_plays_and_is_kept() {
    let scratch = Scratch::new("youtube-add");
    let (server, ytdlp) = stand_in(&scratch, Way::Answers);
    let mut h = settings_with(&scratch, &ytdlp);
    add(&mut h, "Chromium");
    wait_for(&mut h, |music| music.tracks().iter().any(|track| track.title == "Gece Mavisi"));
    let file = std::fs::read_to_string(scratch.path("config").join(crate::accounts::FILE)).expect("the accounts");
    assert!(file.contains("kind = \"youtube\"") && file.contains("browser = \"chromium\""), "{file}");
    assert!(!h.screen().contains("Report plays"), "{}", h.screen());
    let key = h.app().accounts[0].key.clone();
    h.press("esc");
    settle(&mut h);
    h.click_text("YouTube Music");
    settle(&mut h);
    h.press("home");
    h.press("enter");
    let first = Location::Remote { source: crate::library::SourceKey(key), id: "yt-aaaaaaa".to_owned() };
    wait_for(&mut h, |music| music.status().track == Some(first.clone()) && music.status().position > Duration::ZERO);
    let calls = ytdlp.calls();
    assert!(
        calls.iter().any(|call| call.contains("--cookies-from-browser chromium") && call.contains("list=LM")),
        "{calls:?}"
    );
    let asked = || ytdlp.calls().iter().filter(|call| call.contains(" -g ")).count();
    // Both heard to their end, each fetched once from where yt-dlp said.
    wait_for(&mut h, |music| state(music) == State::Ended);
    assert_eq!((asked(), server.requests().len()), (2, 2));
    // Kept whole, the first is played again without asking yt-dlp or the server.
    h.press("home");
    h.press("enter");
    wait_for(&mut h, |music| music.status().track == Some(first.clone()) && music.status().position > Duration::ZERO);
    assert_eq!((asked(), server.requests().len()), (2, 2), "played from the cache");
}

#[test]
fn no_youtube_login_in_the_browser_is_said_on_the_account() {
    let scratch = Scratch::new("youtube-nologin");
    let (_server, ytdlp) = stand_in(&scratch, Way::NoLogin);
    let mut h = settings_with(&scratch, &ytdlp);
    add(&mut h, "Firefox");
    wait_for(&mut h, |music| {
        music.accounts.first().is_some_and(|account| {
            matches!(music.standing.get(&account.key), Some(crate::app::accounts::Standing::Failed(_)))
        })
    });
    settle(&mut h);
    assert!(h.screen().contains("did not accept this login"), "{}", h.screen());
}
