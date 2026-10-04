//! The settings page and the update question: each switch shown changes what qmus does.

use qframe::storage::Ecosystem;

use super::*;

#[test]
fn the_update_question_is_asked_only_while_the_quvyta_switch_is_on() {
    let scratch = Scratch::new("settings-ask");
    let folder = albums(&scratch);
    let h = open(&scratch, &folder);
    assert_eq!(h.update_checks().len(), 1, "the switch is on at first");
    let quiet = Scratch::new("settings-quiet");
    let machine = machine(&quiet);
    Ecosystem::QUVYTA.set_update_notice_in(machine.config.as_ref().expect("folder"), false).expect("saved");
    let h = open_on(machine, &albums(&quiet), crate::locales::env(), 100);
    assert!(h.update_checks().is_empty(), "nothing is asked once it is off");
}

#[test]
fn the_update_switch_is_shown_only_where_it_can_be_kept() {
    let scratch = Scratch::new("settings-switch");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    // Tall enough for the whole settings page.
    h.resize(100, 40);
    click_icon(&mut h, "settings");
    settle(&mut h);
    assert!(h.screen().contains("Say when an update is out"), "{}", h.screen());
    let bare = Scratch::new("settings-bare");
    let machine = Machine { updates: None, ..machine(&bare) };
    let mut h = open_on(machine, &albums(&bare), crate::locales::env(), 100);
    h.resize(100, 40);
    click_icon(&mut h, "settings");
    settle(&mut h);
    assert!(h.screen().contains("Settings"), "{}", h.screen());
    assert!(!h.screen().contains("Say when an update is out"), "{}", h.screen());
}

#[test]
fn a_theme_chosen_for_qmus_alone_is_shown_as_its_own() {
    let scratch = Scratch::new("settings-member");
    let folder = albums(&scratch);
    let machine = machine(&scratch);
    let config = machine.config.clone().expect("folder");
    let opening = Opening::new(machine, folder);
    let mut h = Harness::member_in(opening.music, Ecosystem::QUVYTA, &config, super::super::APP, 100, 30);
    h.set_locale("en").set_glyph_mode(GlyphMode::Unicode).set_reduced_motion(true);
    settle(&mut h);
    click_icon(&mut h, "settings");
    settle(&mut h);
    // Each shared row has a box under it, checked while qmus follows the Quvyta-wide value. The
    // box is drawn in colour, so the rows under the theme and the icons are compared with theirs.
    let alike = |h: &Harness<Music>| {
        let screen = h.screen();
        let lines: Vec<&str> = screen.lines().collect();
        let under = |label: &str| {
            let at = lines.iter().position(|line| line.contains(label)).expect("the row") + 1;
            let row = u16::try_from(at).expect("row");
            (0..100).map(|column| (h.fg(column, row), h.bg(column, row))).collect::<Vec<_>>()
        };
        under("Theme") == under("Icons")
    };
    assert!(alike(&h), "both follow the Quvyta-wide choice at first:\n{}", h.screen());
    Ecosystem::QUVYTA
        .set_in(&config, super::super::APP, qframe::storage::Shared::Theme, "nordic", qframe::storage::Scope::App)
        .expect("saved");
    h.poll_preferences();
    settle(&mut h);
    assert!(h.screen().contains("Nordic"), "{}", h.screen());
    assert!(!alike(&h), "qmus hears that its theme is its own now:\n{}", h.screen());
}

#[test]
fn the_runtime_qmus_runs_follows_a_theme_another_application_sets() {
    let scratch = Scratch::new("settings-runtime");
    let folder = albums(&scratch);
    let machine = machine(&scratch);
    let config = machine.config.clone().expect("folder");
    let opening = Opening::new(machine, folder);
    let mut h = crate::runtime(opening, Some(config.clone())).harness_in(&config, 100, 30).expect("opened");
    h.set_locale("en").set_glyph_mode(GlyphMode::Unicode).set_reduced_motion(true);
    settle(&mut h);
    // The media icons are the shared set's: the play button is drawn and can be pressed.
    click_icon(&mut h, "media-play");
    wait_for(&mut h, |music| state(music) == State::Playing);
    Ecosystem::QUVYTA
        .set_in(&config, super::super::APP, qframe::storage::Shared::Theme, "nordic", qframe::storage::Scope::Ecosystem)
        .expect("saved");
    h.poll_preferences();
    settle(&mut h);
    click_icon(&mut h, "settings");
    settle(&mut h);
    assert!(h.screen().contains("Nordic"), "{}", h.screen());
}

/// Clicks the switch of the settings row labelled `label`. A switch is drawn in colour alone, at
/// the right edge of the rows, where the visualizer's picker ends with its last word.
fn click_row(h: &mut Harness<Music>, label: &str) {
    let screen = h.screen();
    let lines: Vec<&str> = screen.lines().collect();
    let edge =
        lines.iter().find_map(|line| line.rfind("Off").map(|at| line[..at].chars().count() + 3)).expect("the edge");
    let row = lines.iter().position(|line| line.contains(label)).expect("the row");
    h.click(i32::try_from(edge).expect("a column") - 2, i32::try_from(row).expect("a row"));
}

#[test]
fn with_picking_up_turned_off_the_last_queue_does_not_come_back() {
    let scratch = Scratch::new("settings-resume");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    // Adamlar's long track, still heard when qmus closes.
    h.press("enter");
    settle(&mut h);
    click_icon(&mut h, "settings");
    settle(&mut h);
    click_row(&mut h, "Pick up where you left off");
    settle(&mut h);
    let kept = std::fs::read_to_string(scratch.path("config/music.conf")).expect("the settings file");
    assert!(kept.contains("resume-queue = false"), "{kept}");
    h.press("q");
    settle(&mut h);
    let h = open(&scratch, &folder);
    assert_eq!(h.app().current(), None, "nothing is held from the last run:\n{}", h.screen());
    // Turned back on, the default, the next run picks up again and the file forgets the key.
    let mut h = h;
    // Adamlar's long track, still heard when qmus closes.
    h.press("enter");
    settle(&mut h);
    click_icon(&mut h, "settings");
    settle(&mut h);
    click_row(&mut h, "Pick up where you left off");
    settle(&mut h);
    h.press("q");
    settle(&mut h);
    let kept = std::fs::read_to_string(scratch.path("config/music.conf")).unwrap_or_default();
    assert!(!kept.contains("resume-queue"), "{kept}");
    let h = open(&scratch, &folder);
    assert_eq!(h.app().current().map(|track| track.title.as_str()), Some("Uzun Yol"));
}

#[test]
fn a_visualizer_turned_off_leaves_the_player_bar_without_it() {
    let scratch = Scratch::new("settings-off");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    let bar = |h: &Harness<Music>| h.screen().lines().last().expect("the bar").to_owned();
    assert!(bar(&h).contains("▁▁▁▁"), "the visualizer rests in the bar:\n{}", h.screen());
    click_icon(&mut h, "settings");
    settle(&mut h);
    h.click_text("Off");
    settle(&mut h);
    assert!(!bar(&h).contains("▁▁▁▁"), "no visualizer:\n{}", h.screen());
    let kept = std::fs::read_to_string(scratch.path("config/music.conf")).expect("the settings file");
    assert!(kept.contains("visualizer = \"off\""), "{kept}");
}

/// Clicks the last place `text` shows on screen: a button sits right of the row naming it.
fn click_last(h: &mut Harness<Music>, text: &str) {
    let screen = h.screen();
    let (column, row) = screen
        .lines()
        .enumerate()
        .filter_map(|(row, line)| line.rfind(text).map(|at| (line[..at].chars().count(), row)))
        .last()
        .unwrap_or_else(|| panic!("no {text}:\n{screen}"));
    h.click(i32::try_from(column).expect("a column"), i32::try_from(row).expect("a row"));
}

/// The titles of the tracks the screen lists.
fn titles(h: &Harness<Music>) -> Vec<String> {
    h.app().tracks().iter().map(|track| track.title.clone()).collect()
}

#[test]
fn a_folder_added_in_the_settings_joins_the_library_and_is_kept() {
    let scratch = Scratch::new("settings-add-folder");
    let folder = albums(&scratch);
    tagged_wav(&scratch.path("ekstra/yeni/1.wav"), 0.4, "Gece Mavisi", "Kalben", "Yeni", 1);
    let mut h = open(&scratch, &folder);
    h.resize(100, 40);
    assert!(!titles(&h).contains(&"Gece Mavisi".to_owned()));
    click_icon(&mut h, "settings");
    settle(&mut h);
    click_last(&mut h, "Add");
    settle(&mut h);
    // The picker opens above the folder qmus opens with, where the added folder sits.
    h.click_text("ekstra");
    settle(&mut h);
    h.click_text("Choose folder");
    settle(&mut h);
    assert!(titles(&h).contains(&"Gece Mavisi".to_owned()), "{:?}\n{}", titles(&h), h.screen());
    assert_eq!(h.app().tracks().len(), 4);
    assert!(h.screen().contains("~/ekstra"), "the folder has its row:\n{}", h.screen());
    let kept = std::fs::read_to_string(scratch.path("config/music.conf")).expect("the settings file");
    assert!(kept.contains("library-folders") && kept.contains("ekstra"), "{kept}");
    // The next run reads it again without being told.
    h.press("q");
    settle(&mut h);
    let h = open(&scratch, &folder);
    assert!(titles(&h).contains(&"Gece Mavisi".to_owned()), "{:?}", titles(&h));
}

#[test]
fn a_folder_already_read_is_not_added_twice() {
    let scratch = Scratch::new("settings-add-inside");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    h.resize(100, 40);
    click_icon(&mut h, "settings");
    settle(&mut h);
    click_last(&mut h, "Add");
    settle(&mut h);
    // The picker's row, below the header that also names the folder.
    click_last(&mut h, "music");
    settle(&mut h);
    h.click_text("Choose folder");
    settle(&mut h);
    assert_eq!(h.app().tracks().len(), 3, "each track once:\n{}", h.screen());
    assert!(h.screen().contains("already read"), "{}", h.screen());
    let kept = std::fs::read_to_string(scratch.path("config/music.conf")).unwrap_or_default();
    assert!(!kept.contains("library-folders"), "{kept}");
}

#[test]
fn a_folder_removed_leaves_the_library_and_its_music_stays() {
    let scratch = Scratch::new("settings-remove-folder");
    let folder = albums(&scratch);
    let added = scratch.path("ekstra/yeni/1.wav");
    tagged_wav(&added, 0.4, "Gece Mavisi", "Kalben", "Yeni", 1);
    let conf = format!("library-folders = [\"{}\"]\n", scratch.path("ekstra").display());
    std::fs::create_dir_all(scratch.path("config")).expect("folder");
    std::fs::write(scratch.path("config/music.conf"), conf).expect("written");
    let mut h = open(&scratch, &folder);
    h.resize(100, 40);
    assert!(titles(&h).contains(&"Gece Mavisi".to_owned()), "{:?}", titles(&h));
    assert!(h.screen().contains("~/music +1"), "{}", h.screen());
    click_icon(&mut h, "settings");
    settle(&mut h);
    click_last(&mut h, "Remove");
    settle(&mut h);
    assert!(!titles(&h).contains(&"Gece Mavisi".to_owned()), "{:?}", titles(&h));
    assert!(added.exists(), "the music stays where it is");
    let kept = std::fs::read_to_string(scratch.path("config/music.conf")).unwrap_or_default();
    assert!(!kept.contains("library-folders"), "{kept}");
}

#[test]
fn reading_again_finds_music_added_and_keeps_the_track_heard() {
    let scratch = Scratch::new("settings-rescan");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    h.resize(100, 40);
    // Adamlar's long track.
    h.press("enter");
    settle(&mut h);
    let heard = h.app().current().map(|track| track.title.clone());
    assert_eq!(heard.as_deref(), Some("Uzun Yol"));
    tagged_wav(&scratch.path("music/eski/0.wav"), 0.4, "Önce", "Adamlar", "Eski", 0);
    click_icon(&mut h, "settings");
    settle(&mut h);
    click_last(&mut h, "Read again");
    settle(&mut h);
    assert!(titles(&h).contains(&"Önce".to_owned()), "{:?}", titles(&h));
    assert_eq!(h.app().current().map(|track| track.title.clone()), heard, "the same track is heard");
}

#[test]
fn the_index_is_kept_in_the_state_folder_and_nothing_is_written_among_the_music() {
    let scratch = Scratch::new("settings-index");
    let folder = albums(&scratch);
    let listing = |root: &Path| {
        let mut names: Vec<_> = walk(root);
        names.sort();
        names
    };
    let before = listing(&folder);
    let mut h = open(&scratch, &folder);
    h.press("q");
    settle(&mut h);
    assert!(scratch.path("state/library.index").exists());
    assert_eq!(listing(&folder), before);
}

/// Every path under `root`.
fn walk(root: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        }
        found.push(path);
    }
    found
}
