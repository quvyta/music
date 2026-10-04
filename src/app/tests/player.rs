//! Playing: choosing a row, holding and going on, and one track giving way to the next.

use super::*;

#[test]
fn enter_plays_the_track_under_the_cursor_and_the_bar_follows_it() {
    let scratch = Scratch::new("play-enter");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    // Adamlar's long track is the first row.
    h.press("enter");
    wait_for(&mut h, |music| music.status().position >= Duration::from_secs(1));
    let screen = h.screen();
    assert!(screen.contains("Uzun Yol · Adamlar"), "the bar names the track heard:\n{screen}");
    assert!(screen.contains("0:01"), "the bar shows how far it has got:\n{screen}");
    assert!(screen.contains(&h.env().icons().glyph("media-pause").into_owned()), "{screen}");
}

#[test]
fn the_play_button_holds_the_sound_and_goes_on_from_there() {
    let scratch = Scratch::new("play-button");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    click_icon(&mut h, "media-play");
    wait_for(&mut h, |music| state(music) == State::Playing && music.status().position > Duration::ZERO);
    click_icon(&mut h, "media-pause");
    settle(&mut h);
    assert_eq!(state(h.app()), State::Paused);
    let held = h.app().status().position;
    std::thread::sleep(Duration::from_millis(300));
    h.advance(super::super::TICK);
    assert!(h.app().status().position <= held + Duration::from_millis(60), "nothing is heard while held");
    assert!(h.screen().contains(&h.env().icons().glyph("media-play").into_owned()), "{}", h.screen());
    h.press("p");
    h.advance(super::super::TICK);
    let from = h.app().status().position;
    assert!(from + Duration::from_millis(100) >= held, "goes on from where it was left: {from:?} against {held:?}");
    wait_for(&mut h, |music| state(music) == State::Playing && music.status().position > held);
}

#[test]
fn a_track_heard_to_its_end_gives_way_to_the_next_row() {
    let scratch = Scratch::new("play-next");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    h.press("down");
    h.press("enter");
    wait_for(&mut h, |music| music.current().is_some_and(|track| track.title == "Haydi Söyle"));
    assert_eq!(state(h.app()), State::Playing);
    wait_for(&mut h, |music| state(music) == State::Ended);
    assert!(h.screen().contains("Haydi Söyle · Kalben"), "the last track stays named:\n{}", h.screen());
}

#[test]
fn the_next_and_previous_buttons_step_through_the_list() {
    let scratch = Scratch::new("play-steps");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    h.press("enter");
    settle(&mut h);
    click_icon(&mut h, "media-next");
    settle(&mut h);
    assert_eq!(h.app().current().map(|track| track.title.as_str()), Some("Aşk İçinde"));
    click_icon(&mut h, "media-previous");
    settle(&mut h);
    assert_eq!(h.app().current().map(|track| track.title.as_str()), Some("Uzun Yol"));
}

#[test]
fn a_track_chosen_while_the_sound_is_held_is_heard() {
    let scratch = Scratch::new("play-held-next");
    // Long enough that the first is still heard when the pause lands on a loaded machine.
    tagged_wav(&scratch.path("music/1.wav"), 3.0, "Aşk İçinde", "Kalben", "Sonsuz", 1);
    tagged_wav(&scratch.path("music/2.wav"), 3.0, "Haydi Söyle", "Kalben", "Sonsuz", 2);
    let mut h = open(&scratch, &scratch.path("music"));
    h.press("enter");
    wait_for(&mut h, |music| music.status().position > Duration::ZERO);
    assert_eq!(h.app().current().map(|track| track.title.as_str()), Some("Aşk İçinde"));
    click_icon(&mut h, "media-pause");
    settle(&mut h);
    assert_eq!(state(h.app()), State::Paused);
    click_icon(&mut h, "media-next");
    wait_for(&mut h, |music| music.current().is_some_and(|track| track.title == "Haydi Söyle"));
    wait_for(&mut h, |music| state(music) == State::Playing && music.status().position >= Duration::from_millis(200));
    assert!(h.screen().contains(&h.env().icons().glyph("media-pause").into_owned()), "{}", h.screen());
}

#[test]
fn previous_starts_a_track_over_once_it_has_played_three_seconds() {
    let scratch = Scratch::new("play-restart");
    tagged_wav(&scratch.path("music/a.wav"), 0.4, "Kısa", "Adamlar", "Eski", 1);
    tagged_wav(&scratch.path("music/b.wav"), 6.0, "Uzun", "Adamlar", "Eski", 2);
    let folder = scratch.path("music");
    let mut h = open(&scratch, &folder);
    h.press("down");
    h.press("enter");
    wait_for(&mut h, |music| music.status().position >= Duration::from_millis(3_300));
    click_icon(&mut h, "media-previous");
    settle(&mut h);
    assert_eq!(h.app().current().map(|track| track.title.as_str()), Some("Uzun"), "the same track, over");
    assert!(h.app().status().position < Duration::from_secs(2), "from its start: {:?}", h.app().status());
}

#[test]
fn the_steps_are_offered_only_once_a_track_is_loaded() {
    let scratch = Scratch::new("play-steps-off");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    let color = |h: &Harness<Music>, key: &str| {
        let glyph = h.env().icons().glyph(key).into_owned();
        let (x, y) = find(h, &glyph).unwrap_or_else(|| panic!("no {key} button:\n{}", h.screen()));
        h.fg(u16::try_from(x).expect("x"), u16::try_from(y).expect("y"))
    };
    let (next_off, previous_off, play) =
        (color(&h, "media-next"), color(&h, "media-previous"), color(&h, "media-play"));
    assert_ne!(next_off, play, "next is drawn as not offered while nothing is loaded");
    assert_ne!(previous_off, play, "previous too");
    click_icon(&mut h, "media-next");
    settle(&mut h);
    assert_eq!(state(h.app()), State::Stopped, "a step offers nothing to play");
    h.press("enter");
    settle(&mut h);
    assert_ne!(color(&h, "media-next"), next_off, "next is offered once a track is loaded");
    assert_ne!(color(&h, "media-previous"), previous_off);
}

#[test]
fn a_file_that_cannot_be_played_is_passed_over_and_the_next_one_plays() {
    let scratch = Scratch::new("play-broken");
    std::fs::write(scratch.path("music/Bozuk/00 kırık.flac"), "not music").expect("file");
    tagged_wav(&scratch.path("music/b.wav"), 0.6, "Sağlam", "Adamlar", "Eski", 1);
    let folder = scratch.path("music");
    let mut h = open(&scratch, &folder);
    assert!(h.screen().contains("00 kırık"), "the broken file is listed:\n{}", h.screen());
    h.press("enter");
    wait_for(&mut h, |music| music.current().is_some_and(|track| track.title == "Sağlam"));
    wait_for(&mut h, |music| state(music) == State::Playing);
    assert!(h.screen().contains("00 kırık could not be played"), "and says why it was passed over:\n{}", h.screen());
}

#[test]
fn a_sound_device_that_goes_away_stops_the_player_and_says_so() {
    let scratch = Scratch::new("play-device-gone");
    let folder = albums(&scratch);
    let machine = Machine { audio: AudioOut::Vanishing(Duration::from_millis(300)), ..machine(&scratch) };
    let mut h = open_on(machine, &folder, crate::locales::env(), 100);
    h.press("enter");
    wait_for(&mut h, |music| state(music) == State::Stopped && music.status().problem.is_some());
    settle(&mut h);
    let screen = h.screen();
    assert!(screen.contains("The sound output stopped"), "{screen}");
    assert_eq!(h.app().current().map(|track| track.title.as_str()), Some("Uzun Yol"), "nothing is skipped");
    assert!(screen.contains(&h.env().icons().glyph("media-play").into_owned()), "play is offered again:\n{screen}");
}

#[test]
fn the_seek_keys_move_the_track_heard_and_its_time() {
    let scratch = Scratch::new("play-seek");
    tagged_wav(&scratch.path("music/long.wav"), 20.0, "Uzun Hava", "Kalben", "Sonsuz", 1);
    let folder = scratch.path("music");
    let mut h = open(&scratch, &folder);
    h.press("enter");
    wait_for(&mut h, |music| state(music) == State::Playing && music.status().position > Duration::ZERO);
    h.press("shift+right");
    h.press("shift+right");
    h.advance(super::super::TICK);
    assert!(h.app().status().position >= Duration::from_secs(10), "{:?}", h.app().status());
    assert!(h.screen().contains("0:10"), "{}", h.screen());
    h.press("shift+left");
    h.advance(super::super::TICK);
    let back = h.app().status().position;
    assert!(back >= Duration::from_secs(5) && back < Duration::from_secs(8), "{back:?}");
    assert!(h.screen().contains("0:05"), "{}", h.screen());
}

#[test]
fn the_volume_keys_and_the_mute_button_change_what_goes_out() {
    let scratch = Scratch::new("play-volume");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    assert!(h.screen().contains(" 100"), "{}", h.screen());
    h.press("-");
    h.press("-");
    assert!(h.screen().contains(" 90"), "{}", h.screen());
    assert!((h.app().player.gain() - 0.729).abs() < 1e-4, "{}", h.app().player.gain());
    click_icon(&mut h, "media-volume");
    assert_eq!(h.app().player.gain(), 0.0);
    assert!(h.screen().contains(&h.env().icons().glyph("media-muted").into_owned()), "{}", h.screen());
    click_icon(&mut h, "media-muted");
    assert!((h.app().player.gain() - 0.729).abs() < 1e-4);
    h.press("+");
    assert!(h.screen().contains(" 95"), "{}", h.screen());
}

/// The title of the track loaded, once there is one.
fn heard(music: &Music) -> Option<String> {
    music.current().map(|track| track.title.clone())
}

#[test]
fn repeating_one_plays_the_track_again_and_next_still_goes_on() {
    let scratch = Scratch::new("play-repeat-one");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    h.press("down");
    h.press("enter");
    h.press("r");
    h.press("r");
    assert!(h.screen().contains(&h.env().icons().glyph("media-repeat-once").into_owned()), "{}", h.screen());
    // Long enough for the short track to have ended twice.
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(1_500) {
        std::thread::sleep(Duration::from_millis(20));
        h.advance(super::super::TICK);
    }
    assert_eq!(heard(h.app()).as_deref(), Some("Aşk İçinde"));
    assert_eq!(state(h.app()), State::Playing);
    h.press("n");
    assert_eq!(heard(h.app()).as_deref(), Some("Haydi Söyle"));
}

#[test]
fn repeating_all_begins_the_queue_again_once_it_has_been_heard() {
    let scratch = Scratch::new("play-repeat-all");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    // Chosen before any track, it holds for the queue the track then starts.
    h.press("r");
    h.press("down");
    h.press("enter");
    wait_for(&mut h, |music| heard(music).as_deref() == Some("Haydi Söyle"));
    wait_for(&mut h, |music| heard(music).as_deref() == Some("Aşk İçinde"));
    assert_eq!(state(h.app()), State::Playing);
    click_icon(&mut h, "media-repeat");
    assert_eq!(h.app().queue.repeat(), crate::queue::Repeat::One, "the button goes round too");
    assert!(h.screen().contains(&h.env().icons().glyph("media-repeat-once").into_owned()), "{}", h.screen());
}

#[test]
fn shuffle_plays_every_track_still_to_come_once_in_another_order() {
    let scratch = Scratch::new("play-shuffle");
    for number in 1..=12_u32 {
        let path = scratch.path(&format!("music/{number:02}.wav"));
        tagged_wav(&path, 3.0, &format!("Song {number:02}"), "Kalben", "Sonsuz", number);
    }
    let folder = scratch.path("music");
    let mut h = open(&scratch, &folder);
    h.press("enter");
    click_icon(&mut h, "media-shuffle");
    let mut order = Vec::new();
    for _ in 1..12 {
        h.press("n");
        order.push(heard(h.app()).expect("a track"));
    }
    let listed: Vec<String> = (2..=12).map(|number| format!("Song {number:02}")).collect();
    assert_ne!(order, listed, "shuffled, not in the order of the list");
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(sorted, listed, "each track still to come once");
    assert_eq!(h.press("n").app().status().state, State::Playing, "the last of them is heard");
}

#[test]
fn the_queue_comes_back_held_at_the_track_and_moment_it_was_left_at() {
    let scratch = Scratch::new("play-take-up");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    // Adamlar's long track is the first row.
    h.press("enter");
    wait_for(&mut h, |music| music.status().position >= Duration::from_secs(1));
    h.press("p");
    h.press("r");
    h.press("-");
    let held = h.app().status().position;
    h.press("q");
    assert!(h.quit_requested(), "q still quits");
    drop(h);

    let mut h = open(&scratch, &folder);
    wait_for(&mut h, |music| music.status().position >= held.saturating_sub(Duration::from_millis(50)));
    assert_eq!(state(h.app()), State::Paused, "taken up held, not heard");
    assert_eq!(heard(h.app()).as_deref(), Some("Uzun Yol"));
    assert!(h.app().status().position < held + Duration::from_millis(100), "{:?} against {held:?}", h.app().status());
    assert!(h.screen().contains("Uzun Yol · Adamlar"), "{}", h.screen());
    assert_eq!(h.app().queue.repeat(), crate::queue::Repeat::All, "repeat is kept too");
    assert!(h.screen().contains(" 95"), "and the volume:\n{}", h.screen());
    assert!((h.app().player.gain() - 0.857_375).abs() < 1e-4, "heard at it too: {}", h.app().player.gain());
    h.press("p");
    h.advance(super::super::TICK);
    let from = h.app().status().position;
    assert!(from + Duration::from_millis(100) >= held, "goes on from where it was left: {from:?} against {held:?}");
    wait_for(&mut h, |music| state(music) == State::Playing && music.status().position > held);
}

#[test]
fn the_next_track_of_the_queue_follows_with_no_stop_between() {
    let scratch = Scratch::new("play-gapless");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    h.press("down");
    h.press("enter");
    let stopped = std::cell::Cell::new(false);
    wait_for(&mut h, |music| {
        stopped.set(stopped.get() || state(music) != State::Playing);
        heard(music).as_deref() == Some("Haydi Söyle")
    });
    assert!(!stopped.get(), "the player never stood still between the two");
    assert_eq!(h.app().player.plays(), 1, "the second followed in the same sound; it was not started anew");
    assert!(h.screen().contains("Haydi Söyle · Kalben"), "{}", h.screen());
    wait_for(&mut h, |music| state(music) == State::Ended);
}

#[test]
fn space_on_the_tracks_holds_the_track_heard_and_goes_on_without_starting_it_anew() {
    let scratch = Scratch::new("play-space");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    h.press("enter");
    wait_for(&mut h, |music| state(music) == State::Playing && music.status().position > Duration::ZERO);
    assert!(h.is_focused("tracks"), "the table holds the focus");
    h.press("down");
    h.press("space");
    settle(&mut h);
    assert_eq!(state(h.app()), State::Paused, "space holds the sound rather than playing the row:\n{}", h.screen());
    assert_eq!(h.app().current().map(|track| track.title.as_str()), Some("Uzun Yol"));
    h.press("space");
    wait_for(&mut h, |music| state(music) == State::Playing);
    assert_eq!(h.app().current().map(|track| track.title.as_str()), Some("Uzun Yol"));
    assert_eq!(h.app().player.plays(), 1, "it went on; the row under the cursor was not started");
}

/// The row of the player bar and the cells its seek bar spans, between the two times.
fn seek_bar(h: &Harness<Music>, total: &str) -> (i32, std::ops::Range<i32>) {
    let screen = h.screen();
    let lines: Vec<&str> = screen.lines().collect();
    let row = lines
        .iter()
        .rposition(|line| line.contains("0:00") && line.contains(total))
        .unwrap_or_else(|| panic!("no bar:\n{screen}"));
    let line = lines[row];
    let start = line[..line.find("0:00").expect("the time")].chars().count() + "0:00".len() + 1;
    let end = line[..line.rfind(total).expect("the length")].chars().count() - 1;
    let to = |cells: usize| i32::try_from(cells).expect("cells");
    (to(row), to(start)..to(end))
}

#[test]
fn a_press_on_the_bar_moves_the_track_to_where_it_landed_and_the_pointer_names_the_time() {
    let scratch = Scratch::new("play-seek-bar");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    h.press("enter");
    wait_for(&mut h, |music| state(music) == State::Playing);
    h.press("p");
    settle(&mut h);
    h.press("home");
    let (row, cells) = seek_bar(&h, "0:03");
    let middle = (cells.start + cells.end) / 2;
    h.hover(middle, row);
    h.advance(super::super::TICK);
    assert!(h.screen().contains("0:01"), "the pointer's place is named as a time:\n{}", h.screen());
    let three_quarters = cells.start + (cells.end - cells.start) * 3 / 4;
    h.click(three_quarters, row);
    settle(&mut h);
    let at = h.app().status().position;
    assert!(
        at >= Duration::from_millis(1800) && at < Duration::from_secs(3),
        "the press three quarters along a three-second track lands past its middle: {at:?}"
    );
    assert_eq!(state(h.app()), State::Paused, "a seek keeps the sound held");
}
