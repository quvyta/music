//! The menu of a track, opened with the right button on its row.

use qframe::event::{MouseButton, MouseKind};

use super::*;

/// Right-clicks the row of the track called `title`.
fn menu_of(h: &mut Harness<Music>, title: &str) {
    let (x, y) = find(h, title).unwrap_or_else(|| panic!("no {title} row:\n{}", h.screen()));
    h.mouse(MouseKind::Down(MouseButton::Right), x, y);
    h.mouse(MouseKind::Up(MouseButton::Right), x, y);
    settle(h);
}

/// Clicks the entry `label` of the menu open: the last place it shows, over the rows.
fn choose(h: &mut Harness<Music>, label: &str) {
    let screen = h.screen();
    let (row, line) = screen.lines().enumerate().filter(|(_, line)| line.contains(label)).last().expect("the entry");
    let column = line[..line.find(label).expect("the entry")].chars().count();
    h.click(i32::try_from(column).expect("a column"), i32::try_from(row).expect("a row"));
    settle(h);
}

#[test]
fn play_next_puts_the_track_right_after_the_one_heard() {
    let scratch = Scratch::new("menu-next");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    h.press("enter");
    settle(&mut h);
    menu_of(&mut h, "Haydi Söyle");
    choose(&mut h, "Play next");
    let upcoming: Vec<_> = h.app().queue.upcoming().map(Path::to_path_buf).collect();
    assert_eq!(upcoming.get(1), Some(&scratch.path("music/sonsuz/2.wav")), "{upcoming:?}");
    assert!(h.screen().contains("Haydi Söyle plays next"), "{}", h.screen());
}

#[test]
fn a_track_is_added_to_the_end_of_a_playlist_chosen_from_the_menu() {
    let scratch = Scratch::new("menu-playlist");
    let folder = albums(&scratch);
    let lists = scratch.path("data/playlists");
    std::fs::create_dir_all(&lists).expect("folder");
    std::fs::write(lists.join("Gece.m3u8"), "#EXTM3U\n#EXTINF:3,Adamlar - Uzun Yol\n../../music/eski/1.wav\n")
        .expect("playlist");
    let mut h = open(&scratch, &folder);
    menu_of(&mut h, "Aşk İçinde");
    choose(&mut h, "Add to a playlist");
    choose(&mut h, "Gece");
    let written = std::fs::read_to_string(lists.join("Gece.m3u8")).expect("the playlist");
    let lines: Vec<&str> = written.lines().filter(|line| !line.starts_with('#')).collect();
    assert_eq!(lines, ["../../music/eski/1.wav", "../../music/sonsuz/1.wav"], "{written}");
    assert!(written.contains("#EXTINF:3,Adamlar - Uzun Yol"), "what it said of its tracks stays: {written}");
}

#[test]
fn go_to_the_album_opens_the_album_of_the_track() {
    let scratch = Scratch::new("menu-album");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    menu_of(&mut h, "Haydi Söyle");
    choose(&mut h, "Go to the album");
    let screen = h.screen();
    assert!(screen.contains("Sonsuz · Kalben"), "{screen}");
    assert!(!screen.contains("Uzun Yol") && screen.contains("Aşk İçinde"), "only the album's tracks:\n{screen}");
}
