//! The now-playing page: the track heard, large, with the visualizer across the page.

use super::*;

/// The screen showing `folder` at `width` × `height`, motion on.
fn open_large(scratch: &Scratch, folder: &Path, width: u16, height: u16) -> Harness<Music> {
    let opening = Opening::new(machine(scratch), folder.to_path_buf());
    let mut h = Harness::with_env(opening.music, crate::locales::env(), width, height);
    h.set_locale("en").set_glyph_mode(GlyphMode::Unicode).set_reduced_motion(false);
    settle(&mut h);
    h
}

/// Lets real time and the player's beat pass for `long`.
fn pass(h: &mut Harness<Music>, long: Duration) {
    let start = Instant::now();
    while start.elapsed() < long {
        std::thread::sleep(Duration::from_millis(20));
        h.advance(super::super::TICK);
    }
}

/// The rows between the line naming the artist and the line with the times: the large visualizer.
fn large_bars(h: &Harness<Music>) -> Vec<String> {
    let screen = h.screen();
    let lines: Vec<&str> = screen.lines().collect();
    let top = lines.iter().position(|line| line.contains("Adamlar · Eski")).expect("the artist line");
    let bottom = lines.iter().skip(top).position(|line| line.contains("0:0")).expect("the times") + top;
    // The page's text column starts where the artist line does; a card in place of a cover may
    // stand to its left on the same rows.
    let from = lines[top].chars().position(|letter| letter == 'A').expect("the artist");
    lines[top + 1..bottom].iter().map(|line| line.chars().skip(from).collect()).collect()
}

#[test]
fn alt_1_shows_the_track_heard_large_and_esc_goes_back_to_the_tracks() {
    let scratch = Scratch::new("np-open");
    let folder = albums(&scratch);
    let mut h = open_large(&scratch, &folder, 100, 30);
    h.press("alt+1");
    assert!(h.screen().contains("Nothing is playing"), "{}", h.screen());
    h.press("esc");
    h.press("enter");
    h.press("alt+1");
    let screen = h.screen();
    assert!(screen.contains("Adamlar · Eski"), "{screen}");
    // The title is drawn large, three rows of half blocks, not written.
    assert!(!screen.lines().skip(1).take(6).any(|line| line.contains("Uzun Yol")), "{screen}");
    assert!(screen.lines().skip(1).take(6).filter(|line| line.contains('▀')).count() >= 2, "{screen}");
    assert!(!screen.contains("Aşk İçinde"), "the table is gone:\n{screen}");
    h.press("esc");
    assert!(h.screen().contains("Aşk İçinde"), "{}", h.screen());
    assert!(h.is_focused("tracks"), "back on the tracks");
}

#[test]
fn the_side_bar_opens_the_page_and_a_turkish_title_is_drawn_large() {
    let scratch = Scratch::new("np-button");
    let folder = albums(&scratch);
    // Wide enough for the ten letters large beside the card.
    let mut h = open_large(&scratch, &folder, 160, 30);
    h.press("down");
    h.press("enter");
    click_icon(&mut h, "music-note");
    let screen = h.screen();
    assert!(screen.contains("Kalben · Sonsuz"), "{screen}");
    // The player bar names it; the page draws it in half blocks, its own letters included.
    assert!(!screen.lines().skip(1).take(6).any(|line| line.contains("Aşk İçinde")), "{screen}");
    assert!(screen.lines().skip(1).take(6).filter(|line| line.contains('▀')).count() >= 2, "{screen}");
}

#[test]
fn a_title_the_large_letters_lack_is_written() {
    let scratch = Scratch::new("np-written");
    tagged_wav(&scratch.path("music/1.wav"), 0.4, "Звезда по имени Солнце", "Кино", "Звезда", 1);
    let mut h = open_large(&scratch, &scratch.path("music"), 100, 30);
    h.press("enter");
    h.press("alt+1");
    let screen = h.screen();
    assert!(
        screen.lines().skip(1).take(4).any(|line| line.contains("Звезда по имени Солнце")),
        "written in bold, not drawn with gaps:\n{screen}"
    );
}

#[test]
fn the_large_visualizer_rises_with_the_sound_and_rests_when_it_is_held() {
    let scratch = Scratch::new("np-bars");
    let folder = albums(&scratch);
    let mut h = open_large(&scratch, &folder, 100, 30);
    h.press("enter");
    h.press("alt+1");
    let start = Instant::now();
    while large_bars(&h).iter().filter(|line| line.contains('█')).count() < 3 {
        assert!(start.elapsed() < GENEROUS, "the columns never rose:\n{}", h.screen());
        pass(&mut h, Duration::from_millis(40));
    }
    h.press("p");
    pass(&mut h, Duration::from_millis(2_500));
    assert!(large_bars(&h).iter().all(|line| !line.contains('█')), "down to rest:\n{}", h.screen());
}

#[test]
fn at_every_width_the_player_bar_keeps_the_name_both_times_and_the_buttons() {
    let scratch = Scratch::new("np-widths");
    let folder = albums(&scratch);
    for width in [60, 72, 80, 90, 100, 120] {
        let mut h = open_large(&scratch, &folder, width, 8);
        h.press("enter");
        settle(&mut h);
        let screen = h.screen();
        let bar: Vec<&str> = screen.lines().rev().take(2).collect();
        let bar = bar.join("\n");
        assert!(bar.contains("Uzun Yol · Adamlar"), "{width}:\n{screen}");
        assert!(bar.contains("0:03"), "{width}:\n{screen}");
        assert!(bar.contains(&h.env().icons().glyph("media-pause").into_owned()), "{width}:\n{screen}");
        assert_eq!(bar.contains("100"), width >= 100, "the volume where there is room, {width}:\n{screen}");
    }
}

/// Whether some cell of the page's left part is drawn in `colour`.
fn shows(h: &Harness<Music>, colour: qframe::color::Rgb) -> bool {
    (1..24).any(|row| (0..40).any(|column| h.fg(column, row) == Some(colour) || h.bg(column, row) == Some(colour)))
}

#[test]
fn the_album_cover_stands_beside_the_name_and_goes_with_its_album() {
    let scratch = Scratch::new("np-cover");
    let folder = albums(&scratch);
    std::fs::write(scratch.path("music/eski/cover.png"), crate::testing::solid_png([255, 0, 0])).expect("cover");
    let red = qframe::color::Rgb::new(255, 0, 0);
    let mut h = open_large(&scratch, &folder, 100, 30);
    // Adamlar's long track, of the album with the cover, is the first row.
    h.press("enter");
    h.press("alt+1");
    settle(&mut h);
    assert!(shows(&h, red), "the cover is drawn:\n{}", h.screen());
    assert!(h.screen().contains("Adamlar · Eski"), "{}", h.screen());
    h.press("n");
    settle(&mut h);
    assert!(h.screen().contains("Kalben · Sonsuz"), "{}", h.screen());
    assert!(!shows(&h, red), "an album without a cover shows none:\n{}", h.screen());
}

/// Whether the card in the cover's place names `album` on a line of its own, apart from the line
/// that names the artist and the album together.
fn card_names(h: &Harness<Music>, album: &str) -> bool {
    h.screen().lines().any(|line| line.contains(album) && !line.contains('·'))
}

#[test]
fn an_album_without_a_cover_has_a_card_with_its_name_in_the_covers_place() {
    let scratch = Scratch::new("np-card");
    let folder = albums(&scratch);
    let mut h = open_large(&scratch, &folder, 100, 30);
    h.press("down");
    h.press("enter");
    h.press("alt+1");
    settle(&mut h);
    assert!(h.screen().contains("Kalben · Sonsuz"), "{}", h.screen());
    assert!(card_names(&h, "Sonsuz"), "the card names the album:\n{}", h.screen());
}

#[test]
fn a_cover_read_once_comes_back_from_memory_and_is_kept_for_the_desktop() {
    let scratch = Scratch::new("np-cover-kept");
    let folder = albums(&scratch);
    let cover = scratch.path("music/eski/cover.png");
    let png = crate::testing::solid_png([255, 0, 0]);
    std::fs::write(&cover, &png).expect("cover");
    let red = qframe::color::Rgb::new(255, 0, 0);
    let mut h = open_large(&scratch, &folder, 100, 30);
    h.press("enter");
    h.press("alt+1");
    settle(&mut h);
    assert!(shows(&h, red), "the cover is drawn:\n{}", h.screen());
    let kept = h.app().art.as_ref().and_then(|art| art.file.clone()).expect("a copy for the desktop");
    assert!(kept.starts_with(scratch.path("cache/art")), "{}", kept.display());
    assert_eq!(std::fs::read(&kept).expect("the copy"), png, "the cover's own bytes");
    // The cover leaves the folder; the album heard again still shows it, from memory.
    std::fs::remove_file(&cover).expect("removed");
    h.press("n");
    settle(&mut h);
    assert!(!shows(&h, red), "{}", h.screen());
    h.press("b");
    settle(&mut h);
    assert!(h.screen().contains("Adamlar · Eski"), "{}", h.screen());
    assert!(shows(&h, red), "the cover comes back from memory:\n{}", h.screen());
}

/// The screen showing `folder` large with `music.conf` saying `conf`.
fn open_with(scratch: &Scratch, folder: &Path, conf: &str, reduced: bool) -> Harness<Music> {
    std::fs::create_dir_all(scratch.path("config")).expect("folder");
    std::fs::write(scratch.path("config/music.conf"), conf).expect("settings");
    let mut h = open_large(scratch, folder, 100, 30);
    h.set_reduced_motion(reduced);
    h
}

/// The row of the large visualizer where the resting columns lie, counted from its top, and how
/// many rows it has.
fn resting_row(h: &Harness<Music>) -> (usize, usize) {
    let bars = large_bars(h);
    let row = bars.iter().position(|line| line.matches('▁').count() > 10).expect("the resting columns");
    (row, bars.len())
}

#[test]
fn a_mirrored_visualizer_rests_on_its_middle_row_and_plain_bars_on_the_bottom() {
    let scratch = Scratch::new("np-mirror");
    let folder = albums(&scratch);
    let mut h = open_with(&scratch, &folder, "", false);
    h.press("enter");
    h.press("p");
    h.press("alt+1");
    pass(&mut h, Duration::from_millis(300));
    let (row, rows) = resting_row(&h);
    assert!(row + 3 >= rows, "plain columns rest at the bottom: row {row} of {rows}\n{}", h.screen());
    let mirrored = Scratch::new("np-mirror-2");
    let folder = albums(&mirrored);
    let mut h = open_with(&mirrored, &folder, "visualizer = \"mirror\"\n", false);
    h.press("enter");
    h.press("p");
    h.press("alt+1");
    pass(&mut h, Duration::from_millis(300));
    let (row, rows) = resting_row(&h);
    assert!(row + 3 < rows && row > 1, "mirrored ones rest in the middle: row {row} of {rows}\n{}", h.screen());
}

#[test]
fn keeping_it_moving_draws_the_sound_as_it_is_even_with_motion_reduced() {
    // With motion reduced the visualizer shows a calm reading taken every quarter second, so
    // between two readings the picture stands still while the sound goes on; kept moving, it
    // follows the sound from beat to beat.
    let changes_between_readings = |conf: &str, name: &str| {
        let scratch = Scratch::new(name);
        let folder = albums(&scratch);
        let mut h = open_with(&scratch, &folder, conf, true);
        h.press("enter");
        h.press("alt+1");
        let mut changed = false;
        let mut before = (large_bars(&h), h.app().calm_age);
        for _ in 0..24 {
            std::thread::sleep(Duration::from_millis(20));
            h.advance(super::super::TICK);
            let now = (large_bars(&h), h.app().calm_age);
            // A beat that took a new calm reading may change the picture in either case.
            if now.1 > before.1 && now.0 != before.0 {
                changed = true;
            }
            before = now;
        }
        changed
    };
    assert!(!changes_between_readings("", "np-calm"), "calm between its readings");
    assert!(changes_between_readings("visualizer-live = true\n", "np-live"), "kept moving, it follows the sound");
}
