//! What the screen shows before anything plays.

use super::*;

#[test]
fn the_tracks_of_the_folder_are_listed_by_artist_album_and_number() {
    let scratch = Scratch::new("screen-list");
    let folder = albums(&scratch);
    let h = open(&scratch, &folder);
    let screen = h.screen();
    assert!(screen.contains("3 tracks"), "{screen}");
    let rows: Vec<usize> = ["Uzun Yol", "Aşk İçinde", "Haydi Söyle"]
        .iter()
        .map(|title| find(&h, title).unwrap_or_else(|| panic!("{title} is not listed:\n{screen}")).1 as usize)
        .collect();
    assert!(rows.windows(2).all(|pair| pair[0] < pair[1]), "Adamlar comes before Kalben:\n{screen}");
    assert!(screen.contains("Choose a track to play"), "nothing plays yet:\n{screen}");
    assert!(h.is_focused("tracks"), "the keyboard is on the tracks at once");
    assert!(screen.lines().next().is_some_and(|top| top.contains(" ~/music ")), "the folder from home:\n{screen}");
}

#[test]
fn a_folder_without_music_says_so_and_offers_nothing_to_play() {
    let scratch = Scratch::new("screen-empty");
    let folder = scratch.path("empty/x").parent().expect("folder").to_path_buf();
    std::fs::create_dir_all(&folder).expect("folder");
    let mut h = open(&scratch, &folder);
    assert!(h.screen().contains("No music here"), "{}", h.screen());
    h.press("p");
    settle(&mut h);
    assert_eq!(state(h.app()), State::Stopped);
}

#[test]
fn the_settings_button_opens_the_settings_and_esc_closes_them() {
    let scratch = Scratch::new("screen-settings");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    click_icon(&mut h, "settings");
    settle(&mut h);
    assert!(h.screen().contains("Settings"), "{}", h.screen());
    assert!(!h.screen().contains("Uzun Yol"), "the settings take the place of the tracks:\n{}", h.screen());
    h.press("esc");
    settle(&mut h);
    assert!(h.screen().contains("Uzun Yol"), "{}", h.screen());
}

#[test]
fn a_tiny_terminal_says_it_is_too_small() {
    let scratch = Scratch::new("screen-tiny");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    h.resize(20, 5);
    assert!(h.screen().contains("The terminal is too"), "{}", h.screen());
}

#[test]
fn a_narrow_terminal_keeps_the_track_and_both_times_on_screen() {
    let scratch = Scratch::new("screen-narrow");
    let folder = albums(&scratch);
    let mut h = open_on(machine(&scratch), &folder, crate::locales::env(), 40);
    h.press("enter");
    wait_for(&mut h, |music| music.status().position >= Duration::from_millis(100));
    let screen = h.screen();
    assert!(screen.contains("Uzun Yol"), "the track heard is named:\n{screen}");
    assert!(screen.contains("0:00") && screen.contains("0:03"), "both times fit:\n{screen}");
    assert!(!screen.contains("3 tracks"), "the top strip gives way below 60 columns:\n{screen}");
}

#[test]
fn opus_files_are_listed_faint_and_say_they_cannot_be_played_yet() {
    let scratch = Scratch::new("screen-opus");
    let folder = albums(&scratch);
    std::fs::write(folder.join("sonsuz/3 gece.opus"), "opus bytes").expect("file");
    let mut h = open(&scratch, &folder);
    let screen = h.screen();
    assert!(screen.contains("4 tracks"), "{screen}");
    let row = h.app().tracks().iter().position(|track| track.title == "3 gece").expect("the Opus file is listed");
    // The cursor stands on a row of its own, away from both rows compared.
    let cursor = if row == 0 { 1 } else { 0 };
    for _ in 0..cursor {
        h.press("down");
    }
    let other = (0..4).find(|&index| index != row && index != cursor).expect("a third row");
    let cell = |h: &Harness<Music>, title: &str| {
        let (x, y) = find(h, title).unwrap_or_else(|| panic!("{title} is listed:\n{}", h.screen()));
        h.fg(u16::try_from(x).expect("x"), u16::try_from(y).expect("y"))
    };
    let other_title = h.app().tracks()[other].title.clone();
    assert_ne!(cell(&h, "3 gece"), cell(&h, &other_title), "the Opus row is faint:\n{}", h.screen());
    for _ in cursor..row {
        h.press("down");
    }
    if row < cursor {
        h.press("up");
    }
    h.press("enter");
    settle(&mut h);
    assert_eq!(state(h.app()), State::Stopped, "nothing is played");
    assert!(h.screen().contains("Opus files cannot be played yet"), "{}", h.screen());
}

#[test]
fn the_key_overview_names_the_keys_the_keymap_gives() {
    let scratch = Scratch::new("screen-help");
    let folder = albums(&scratch);
    let mut env = crate::locales::env();
    env.keymap_mut().bind(qframe::keymap::Scope::App, "play-pause", &["k".parse().expect("chord")]);
    let mut h = open_on(machine(&scratch), &folder, env, 100);
    h.press("?");
    // The overview is longer than the screen; its filter brings the row up.
    h.type_text("pause");
    settle(&mut h);
    let screen = h.screen();
    let lines: Vec<&str> = screen.lines().filter(|line| line.contains("play or pause")).collect();
    assert_eq!(lines.len(), 1, "one row for play and pause:\n{screen}");
    assert!(lines[0].contains(" k "), "with the key it is bound to:\n{screen}");
}

#[test]
fn the_key_overview_names_the_keys_of_the_table_of_tracks() {
    let scratch = Scratch::new("screen-help-table");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    h.press("?");
    settle(&mut h);
    let screen = h.screen();
    // The table writes its own keys, the page keys among them, which qmus never names itself.
    assert!(screen.contains("pgup"), "{screen}");
    assert!(screen.lines().any(|line| line.contains("enter") && line.contains("open")), "{screen}");
}

#[test]
fn the_settings_are_read_from_and_kept_in_the_given_folder() {
    let scratch = Scratch::new("screen-config");
    let folder = albums(&scratch);
    let config = machine(&scratch).config.expect("folder");
    let _h = open(&scratch, &folder);
    assert!(config.join("quvyta.conf").is_file(), "the shared file is made in the folder given, not elsewhere");
}
