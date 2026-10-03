//! The visualizer in the player bar: drawn from the sound that goes out, still when it stops.

use super::*;

/// The eight cells of the visualizer, at the start of the player bar.
fn bars(h: &Harness<Music>) -> String {
    let screen = h.screen();
    let bar = screen.lines().last().expect("the player bar");
    bar.chars().skip(1).take(8).collect()
}

/// Whether some column stands above the lowest line.
fn risen(bars: &str) -> bool {
    bars.chars().any(|cell| "▂▃▄▅▆▇█".contains(cell))
}

/// Lets real time and the player's beat pass for `long`.
fn pass(h: &mut Harness<Music>, long: Duration) {
    let start = Instant::now();
    while start.elapsed() < long {
        std::thread::sleep(Duration::from_millis(20));
        h.advance(super::super::TICK);
    }
}

#[test]
fn the_visualizer_rises_with_the_sound_and_comes_down_to_rest_when_it_is_held() {
    let scratch = Scratch::new("vis-moves");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    h.set_reduced_motion(false);
    assert_eq!(bars(&h), "▁".repeat(8), "silent before anything plays:\n{}", h.screen());
    // Adamlar's long track is the first row.
    h.press("enter");
    let start = Instant::now();
    while !risen(&bars(&h)) {
        assert!(start.elapsed() < GENEROUS, "the columns never rose:\n{}", h.screen());
        pass(&mut h, Duration::from_millis(40));
    }
    h.press("p");
    pass(&mut h, Duration::from_millis(1_500));
    assert_eq!(state(h.app()), State::Paused);
    assert_eq!(bars(&h), "▁".repeat(8), "down to rest once held:\n{}", h.screen());
    assert!(h.app().ticker.is_none(), "and nothing more is read while it rests");
}

#[test]
fn with_motion_reduced_the_visualizer_still_follows_the_sound() {
    let scratch = Scratch::new("vis-calm");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    h.press("enter");
    let start = Instant::now();
    while !risen(&bars(&h)) {
        assert!(start.elapsed() < GENEROUS, "the calm reading never rose:\n{}", h.screen());
        pass(&mut h, Duration::from_millis(40));
    }
}

/// How many times the player is read over one second of the screen's clock while a track plays.
fn reads_in_a_second(reduced: bool, conf: &str, name: &str) -> usize {
    let scratch = Scratch::new(name);
    std::fs::create_dir_all(scratch.path("config")).expect("folder");
    std::fs::write(scratch.path("config/music.conf"), conf).expect("settings");
    let folder = albums(&scratch);
    let mut h = open(&scratch, &folder);
    h.set_reduced_motion(reduced);
    h.press("enter");
    settle(&mut h);
    let before = h.app().reads;
    for _ in 0..30 {
        h.advance(super::super::TICK);
    }
    h.app().reads - before
}

#[test]
fn with_motion_reduced_the_player_is_read_four_times_a_second_not_thirty() {
    let moving = reads_in_a_second(false, "", "beat-moving");
    let calm = reads_in_a_second(true, "", "beat-calm");
    let off = reads_in_a_second(false, "visualizer = \"off\"\n", "beat-off");
    let live = reads_in_a_second(true, "visualizer-live = true\n", "beat-live");
    assert!(moving >= 25, "the visualizer moves thirty times a second: {moving}");
    assert!(calm <= 5, "calm, four times: {calm}");
    assert!(off <= 5, "with no visualizer, four times: {off}");
    assert!(live >= 25, "kept moving, thirty times even with motion reduced: {live}");
}
