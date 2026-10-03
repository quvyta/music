//! The playlists: the queue kept as one, one opened and played, and one removed only once the
//! person has said yes.

use super::*;

/// Selects the whole name box and types `name` over it.
fn name_it(h: &mut Harness<Music>, name: &str) {
    h.press("ctrl+a");
    h.type_text(name);
}

/// The screen with the queue kept as a playlist called `name` and the playlists page shown.
fn with_a_playlist(scratch: &Scratch, name: &str) -> Harness<Music> {
    let folder = albums(scratch);
    let mut h = open(scratch, &folder);
    // Adamlar's long track is the first row: the queue is it and the two after it.
    h.press("enter");
    settle(&mut h);
    h.press("ctrl+s");
    settle(&mut h);
    name_it(&mut h, name);
    h.press("enter");
    settle(&mut h);
    h.press("alt+6");
    settle(&mut h);
    h
}

#[test]
fn ctrl_s_keeps_the_queue_as_a_playlist_another_player_can_read() {
    let scratch = Scratch::new("lists-save");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    h.press("enter");
    settle(&mut h);
    h.press("ctrl+s");
    settle(&mut h);
    assert!(h.screen().contains("Keep the queue as a playlist"), "{}", h.screen());
    // The name offered is the album heard.
    assert!(h.screen().contains("Eski"), "{}", h.screen());
    name_it(&mut h, "Akşam");
    h.press("enter");
    settle(&mut h);
    assert!(h.screen().contains("The queue is kept as Akşam"), "{}", h.screen());
    let written = std::fs::read_to_string(scratch.path("data/playlists/Akşam.m3u8")).expect("the playlist");
    let lines: Vec<&str> = written.lines().filter(|line| !line.starts_with('#')).collect();
    assert_eq!(lines, ["../../music/eski/1.wav", "../../music/sonsuz/1.wav", "../../music/sonsuz/2.wav"]);
    assert!(written.contains("#EXTINF:3,Adamlar - Uzun Yol"), "{written}");
}

#[test]
fn a_name_already_taken_is_said_in_the_dialog_and_nothing_is_written_over() {
    let scratch = Scratch::new("lists-taken");
    let mut h = with_a_playlist(&scratch, "Akşam");
    let before = std::fs::read(scratch.path("data/playlists/Akşam.m3u8")).expect("the playlist");
    h.press("alt+3");
    h.press("down");
    h.press("enter");
    settle(&mut h);
    h.press("ctrl+s");
    settle(&mut h);
    name_it(&mut h, "Akşam");
    h.press("enter");
    settle(&mut h);
    assert!(h.screen().contains("A playlist of that name is already there"), "{}", h.screen());
    assert_eq!(std::fs::read(scratch.path("data/playlists/Akşam.m3u8")).expect("the playlist"), before);
}

#[test]
fn a_playlist_opened_shows_its_tracks_and_enter_plays_from_the_row_chosen() {
    let scratch = Scratch::new("lists-open");
    let mut h = with_a_playlist(&scratch, "Akşam");
    assert!(h.screen().contains("Akşam"), "the playlist is listed:\n{}", h.screen());
    h.press("enter");
    settle(&mut h);
    let screen = h.screen();
    assert!(screen.contains("Uzun Yol") && screen.contains("Haydi Söyle"), "its tracks:\n{screen}");
    h.press("down");
    h.press("down");
    h.press("enter");
    wait_for(&mut h, |music| music.current().is_some_and(|track| track.title == "Haydi Söyle"));
    // The queue is the playlist from there on: the last of its tracks, nothing after.
    assert_eq!(h.app().queue.upcoming().count(), 1);
    h.press("esc");
    settle(&mut h);
    assert!(!h.screen().contains("Uzun Yol"), "esc goes back to the playlists:\n{}", h.screen());
}

#[test]
fn a_playlist_is_removed_only_after_yes_and_its_music_stays() {
    let scratch = Scratch::new("lists-remove");
    let mut h = with_a_playlist(&scratch, "Akşam");
    let file = scratch.path("data/playlists/Akşam.m3u8");
    h.press("delete");
    settle(&mut h);
    assert!(h.screen().contains("Remove the playlist Akşam?"), "{}", h.screen());
    h.press("esc");
    settle(&mut h);
    assert!(file.exists(), "esc keeps it");
    h.press("delete");
    settle(&mut h);
    // The button, on the row of the dialog's buttons, not the title above them.
    let screen = h.screen();
    let (row, line) = screen.lines().enumerate().find(|(_, line)| line.contains("Cancel")).expect("the buttons");
    let column = line[..line.rfind("Remove").expect("the button")].chars().count();
    h.click(i32::try_from(column).expect("a column"), i32::try_from(row).expect("a row"));
    settle(&mut h);
    assert!(!file.exists(), "yes removes it");
    assert!(scratch.path("music/eski/1.wav").exists(), "the music it listed stays");
    assert!(h.screen().contains("No playlists yet"), "{}", h.screen());
}

#[test]
fn a_track_of_a_playlist_that_is_not_in_the_folder_is_counted_apart() {
    let scratch = Scratch::new("lists-away");
    let folder = albums(&scratch);
    let lists = scratch.path("data/playlists");
    std::fs::create_dir_all(&lists).expect("folder");
    let text = format!("#EXTM3U\n{}\n/nowhere/gone.flac\n", scratch.path("music/eski/1.wav").display());
    std::fs::write(lists.join("Yarım.m3u8"), text).expect("playlist");
    let mut h = open(&scratch, &folder);
    h.press("alt+6");
    settle(&mut h);
    h.press("enter");
    settle(&mut h);
    let screen = h.screen();
    assert!(screen.contains("Yarım · 1 track is not in this folder"), "{screen}");
    assert!(screen.contains("Uzun Yol"), "{screen}");
}
