//! The side bar and the pages beside the tracks: albums, artists, the queue and the search.

use super::*;

/// The screen showing `folder` at `width` × 24.
fn open_wide(scratch: &Scratch, folder: &Path, width: u16) -> Harness<Music> {
    let opening = Opening::new(machine(scratch), folder.to_path_buf());
    let mut h = Harness::with_env(opening.music, crate::locales::env(), width, 24);
    h.set_locale("en").set_glyph_mode(GlyphMode::Unicode).set_reduced_motion(true);
    settle(&mut h);
    h
}

/// The body: every line below the top strip and above the player bar, without the side bar.
fn body(h: &Harness<Music>) -> String {
    let screen = h.screen();
    let lines: Vec<&str> = screen.lines().collect();
    lines[1..lines.len().saturating_sub(1)]
        .iter()
        .map(|line| line.chars().skip(20).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Clicks `text` where it first shows.
fn click_text(h: &mut Harness<Music>, text: &str) {
    let (x, y) = find(h, text).unwrap_or_else(|| panic!("no {text}:\n{}", h.screen()));
    h.click(x, y);
    settle(h);
}

#[test]
fn an_album_opened_from_the_albums_page_plays_as_its_own_queue() {
    let scratch = Scratch::new("pages-albums");
    let folder = albums(&scratch);
    let mut h = open_wide(&scratch, &folder, 110);
    click_text(&mut h, "Albums");
    let shown = body(&h);
    assert!(shown.contains("Eski") && shown.contains("Sonsuz"), "{shown}");
    assert!(!shown.contains("Uzun Yol"), "albums, not tracks:\n{shown}");
    h.press("down");
    h.press("enter");
    settle(&mut h);
    let shown = body(&h);
    assert!(shown.contains("Sonsuz · Kalben"), "the album's name on top:\n{shown}");
    assert!(shown.contains("Aşk İçinde") && shown.contains("Haydi Söyle"), "{shown}");
    assert!(!shown.contains("Uzun Yol"), "only the album's tracks:\n{shown}");
    h.press("esc");
    let shown = body(&h);
    assert!(shown.contains("Eski") && !shown.contains("Aşk İçinde"), "back to the albums:\n{shown}");
    // Eski, the first album, holds the first track of the library; the queue is the album alone.
    h.press("up");
    h.press("enter");
    settle(&mut h);
    h.press("enter");
    wait_for(&mut h, |music| state(music) == State::Ended);
    assert_eq!(h.app().current().map(|track| track.title.as_str()), Some("Uzun Yol"), "the album ends the queue");
}

#[test]
fn an_artist_opened_shows_only_their_tracks() {
    let scratch = Scratch::new("pages-artists");
    let folder = albums(&scratch);
    let mut h = open_wide(&scratch, &folder, 110);
    h.press("alt+5");
    let shown = body(&h);
    assert!(shown.contains("Adamlar") && shown.contains("Kalben"), "{shown}");
    h.press("down");
    h.press("enter");
    settle(&mut h);
    let shown = body(&h);
    assert!(shown.contains("Aşk İçinde") && shown.contains("Haydi Söyle"), "{shown}");
    assert!(!shown.contains("Uzun Yol"), "{shown}");
}

#[test]
fn the_search_narrows_the_tracks_and_its_typing_is_not_taken_for_keys() {
    let scratch = Scratch::new("pages-search");
    let folder = albums(&scratch);
    let mut h = open_wide(&scratch, &folder, 110);
    h.press("/");
    // n is next and b is previous anywhere else; here they are letters.
    h.type_text("kalben haydi");
    settle(&mut h);
    let shown = body(&h);
    assert!(shown.contains("Haydi Söyle"), "{shown}");
    assert!(!shown.contains("Aşk İçinde") && !shown.contains("Uzun Yol"), "{shown}");
    assert_eq!(state(h.app()), State::Stopped, "nothing was played by the typing");
    h.press("enter");
    h.press("enter");
    wait_for(&mut h, |music| music.current().is_some_and(|track| track.title == "Haydi Söyle"));
    h.press("/");
    h.press("esc");
    settle(&mut h);
    let shown = body(&h);
    assert!(shown.contains("Uzun Yol"), "esc empties the search:\n{shown}");
    h.press("/");
    h.type_text("ASK icinde");
    settle(&mut h);
    let shown = body(&h);
    assert!(shown.contains("Aşk İçinde") && !shown.contains("Haydi Söyle"), "Turkish letters found plain:\n{shown}");
    h.type_text("zzz");
    settle(&mut h);
    assert!(body(&h).contains("Nothing found"), "{}", h.screen());
}

#[test]
fn the_queue_page_lists_what_comes_and_takes_a_track_out() {
    let scratch = Scratch::new("pages-queue");
    let folder = albums(&scratch);
    let mut h = open_wide(&scratch, &folder, 110);
    // Adamlar's long track first, then the two of Kalben.
    h.press("enter");
    h.press("alt+2");
    let shown = body(&h);
    assert!(shown.contains("♪") && shown.contains("Uzun Yol"), "{shown}");
    assert!(shown.contains("Aşk İçinde") && shown.contains("Haydi Söyle"), "{shown}");
    h.press("down");
    h.press("delete");
    settle(&mut h);
    let shown = body(&h);
    assert!(!shown.contains("Aşk İçinde"), "taken out:\n{shown}");
    h.press("n");
    assert_eq!(h.app().current().map(|track| track.title.as_str()), Some("Haydi Söyle"));
    h.press("enter");
    settle(&mut h);
    assert!(body(&h).lines().any(|line| line.contains("♪") && line.contains("Haydi Söyle")), "{}", h.screen());
}

#[test]
fn on_a_narrow_terminal_the_side_bar_opens_over_the_body_and_closes_on_a_choice() {
    let scratch = Scratch::new("pages-narrow");
    let folder = albums(&scratch);
    let mut h = open_wide(&scratch, &folder, 80);
    assert!(find(&h, "Artists").is_none(), "folded away:\n{}", h.screen());
    h.press("ctrl+b");
    settle(&mut h);
    click_text(&mut h, "Artists");
    assert!(find(&h, "Queue").is_none(), "closed again:\n{}", h.screen());
    assert!(h.screen().contains("Adamlar") && !h.screen().contains("Uzun Yol"), "{}", h.screen());
}
