//! Every rule of the queue, asked of it the way the program asks: what is heard now, what plays
//! next, and what is still there when qmus starts again.
//!
//! Nothing here plays. The queue is a model of paths, so the tests are about order, and the only
//! files made are the short sounds a queue written and read back has to find where it left them.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Queue, Repeat, random::Random};
use crate::testing::{Scratch, sine_wav};

/// The three modes, in the order the key walks through them.
const MODES: [Repeat; 3] = [Repeat::Off, Repeat::All, Repeat::One];

/// The name of a track, so that a queue can be written down and read back in a failure.
fn name(track: &Path) -> String {
    track.display().to_string()
}

/// The queue as it is shown, the track heard first.
fn shown(queue: &Queue) -> Vec<String> {
    queue.upcoming().map(name).collect()
}

/// The tracks already heard, oldest first.
fn heard_before(queue: &Queue) -> Vec<String> {
    queue.history().map(name).collect()
}

/// A queue of the named tracks, playing the first of them.
fn queue_of(names: &[&str]) -> Queue {
    Queue::from(names.iter().map(Path::new), 0)
}

/// A folder of short sounds under the names asked for, so a queue written and read back finds its
/// tracks where it left them.
fn music(scratch: &Scratch, names: &[&str]) -> Vec<PathBuf> {
    names
        .iter()
        .map(|asked| {
            let path = scratch.path(asked);
            sine_wav(&path, 8_000, 1, 0.1, 440.0);
            path
        })
        .collect()
}

/// One thing a person can do to a queue, taken at random, so a long run of them is not the same few
/// moves over and over.
fn move_queue(queue: &mut Queue, random: &mut Random, turn: u64) {
    let rows = queue.upcoming().count();
    match random.place(8) {
        0 => {
            queue.advance();
        }
        1 => {
            queue.previous(Duration::from_millis(200 + 500 * turn));
        }
        2 => {
            queue.set_shuffle(!turn.is_multiple_of(3), turn);
        }
        3 => {
            queue.set_repeat(MODES[random.place(MODES.len())]);
        }
        4 if rows > 1 => {
            queue.move_item(random.place(rows), random.place(rows));
        }
        5 if rows > 0 => {
            queue.remove(random.place(rows));
        }
        6 => {
            queue.play_next([format!("front-{turn}.wav")]);
        }
        _ => {
            queue.append([format!("back-{turn}.wav")]);
        }
    }
}

#[test]
fn the_list_plays_from_the_track_it_starts_at_and_shows_the_rest_after_it() {
    let queue = Queue::from(["a.wav", "b.wav", "c.wav"], 1);
    assert_eq!(queue.current(), Some(Path::new("b.wav")));
    assert_eq!(shown(&queue), ["b.wav", "c.wav"]);
    assert_eq!(heard_before(&queue), Vec::<String>::new());
}

#[test]
fn a_start_past_the_last_track_plays_the_list_from_its_beginning() {
    let queue = Queue::from(["a.wav", "b.wav"], 9);
    assert_eq!(queue.current(), Some(Path::new("a.wav")));
    assert_eq!(shown(&queue), ["a.wav", "b.wav"]);
}

#[test]
fn an_empty_queue_holds_nothing_and_has_nothing_after_it() {
    let mut queue = Queue::from(Vec::<String>::new(), 0);
    assert_eq!(queue.current(), None);
    assert_eq!(queue.advance(), None);
    assert_eq!(queue.peek_next(), None);
    assert_eq!(shown(&queue), Vec::<String>::new());
    assert!(!queue.remove(0));
    assert!(!queue.clear_upcoming());
}

#[test]
fn the_tracks_heard_before_are_kept_in_the_order_they_were_heard_in() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav", "d.wav"]);
    queue.advance();
    queue.advance();
    assert_eq!(shown(&queue), ["c.wav", "d.wav"]);
    assert_eq!(heard_before(&queue), ["a.wav", "b.wav"]);
    assert_eq!(queue.current(), Some(Path::new("c.wav")));
}

#[test]
fn the_list_ends_and_the_last_track_stays_heard_when_nothing_is_repeated() {
    let mut queue = queue_of(&["a.wav", "b.wav"]);
    assert_eq!(queue.repeat(), Repeat::Off);
    assert_eq!(queue.advance().map(name), Some("b.wav".to_owned()));
    assert_eq!(queue.advance(), None);
    assert_eq!(queue.current(), Some(Path::new("b.wav")), "the last track went on being heard");
    assert_eq!(queue.peek_next(), None);
}

#[test]
fn repeating_all_begins_the_list_again_from_where_it_was_heard() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav"]);
    queue.set_repeat(Repeat::All);
    assert_eq!(queue.repeat(), Repeat::All);
    queue.advance();
    queue.advance();
    assert_eq!(queue.current(), Some(Path::new("c.wav")));
    assert_eq!(queue.advance().map(name), Some("a.wav".to_owned()));
    assert_eq!(queue.peek_next().map(name), Some("b.wav".to_owned()));
}

#[test]
fn repeating_all_begins_the_shuffled_list_at_the_track_it_was_shuffled_from() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav", "d.wav", "e.wav"]);
    queue.set_repeat(Repeat::All);
    queue.set_shuffle(true, 7);
    let first = name(queue.current().expect("a track plays"));
    while shown(&queue).len() > 1 {
        queue.advance().expect("the list repeats");
    }
    assert_eq!(queue.advance().map(name), Some(first), "the shuffled list began somewhere else");
}

#[test]
fn repeating_one_plays_the_same_track_and_leaves_the_queue_where_it_is() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav"]);
    queue.advance();
    queue.set_repeat(Repeat::One);
    assert_eq!(queue.repeat(), Repeat::One);
    assert_eq!(queue.current(), Some(Path::new("b.wav")));
    assert_eq!(queue.advance().map(name), Some("b.wav".to_owned()));
    assert_eq!(queue.advance().map(name), Some("b.wav".to_owned()));
    assert_eq!(shown(&queue), ["b.wav", "c.wav"], "the queue did not move");
}

#[test]
fn what_plays_next_is_said_before_the_queue_moves_to_it() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav"]);
    assert_eq!(queue.peek_next().map(name), Some("b.wav".to_owned()));
    assert_eq!(queue.current(), Some(Path::new("a.wav")), "saying it moved the queue");
    assert_eq!(queue.advance().map(name), Some("b.wav".to_owned()));
}

#[test]
fn what_plays_next_is_always_what_the_next_call_hands_over() {
    let mut random = Random::seeded(20_260_925);
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav", "d.wav"]);
    for turn in 0..1000 {
        move_queue(&mut queue, &mut random, turn);
        let before = shown(&queue);
        let said = queue.peek_next().map(name);
        let gave = queue.advance().map(name);
        assert_eq!(said, gave, "turn {turn}, the queue was {before:?}");
    }
}

#[test]
fn a_track_heard_for_three_seconds_starts_again_and_a_shorter_one_goes_back() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav"]);
    queue.advance();
    assert_eq!(queue.current(), Some(Path::new("b.wav")));
    assert_eq!(queue.previous(Duration::from_millis(3100)).map(name), Some("b.wav".to_owned()));
    assert_eq!(queue.current(), Some(Path::new("b.wav")), "the track restarted");
    assert_eq!(queue.previous(Duration::from_millis(2900)).map(name), Some("a.wav".to_owned()));
    assert_eq!(queue.current(), Some(Path::new("a.wav")));
}

#[test]
fn the_first_track_starts_again_when_there_is_none_before_it() {
    let mut queue = queue_of(&["a.wav", "b.wav"]);
    assert_eq!(queue.previous(Duration::from_millis(2900)).map(name), Some("a.wav".to_owned()));
    assert_eq!(queue.previous(Duration::from_millis(3100)).map(name), Some("a.wav".to_owned()));
    assert_eq!(shown(&queue), ["a.wav", "b.wav"]);
}

#[test]
fn going_back_to_a_track_that_is_no_longer_there_starts_the_one_heard_again() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav"]);
    queue.advance();
    queue.remove(0);
    assert_eq!(queue.current(), Some(Path::new("b.wav")));
    assert_eq!(queue.previous(Duration::from_millis(2900)).map(name), Some("a.wav".to_owned()));
}

#[test]
fn shuffling_keeps_the_track_heard_where_it_is_and_the_same_tracks_after_it() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav", "d.wav", "e.wav"]);
    queue.advance();
    let heard = queue.current().map(name);
    let before = heard_before(&queue);
    queue.set_shuffle(true, 7);
    assert!(queue.is_shuffled());
    assert_eq!(queue.current().map(name), heard, "the track heard was moved");
    assert_eq!(heard_before(&queue), before, "the tracks heard before were shuffled");
    let mut after = shown(&queue);
    after.sort();
    assert_eq!(after, ["b.wav", "c.wav", "d.wav", "e.wav"], "a track was lost or gained");
}

#[test]
fn shuffling_really_rearranges_what_is_after_the_track_heard() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav", "d.wav", "e.wav"]);
    let before = shown(&queue);
    queue.set_shuffle(true, 7);
    assert_ne!(shown(&queue), before, "the shuffle left the order as it was");
}

#[test]
fn the_same_seed_shuffles_the_queue_the_same_way_again() {
    let names = ["a.wav", "b.wav", "c.wav", "d.wav", "e.wav", "f.wav"];
    let mut one = queue_of(&names);
    let mut other = queue_of(&names);
    one.set_shuffle(true, 4242);
    other.set_shuffle(true, 4242);
    assert_eq!(shown(&one), shown(&other));
    other.set_shuffle(true, 99);
    assert_ne!(shown(&one), shown(&other), "another seed gave the same order");
}

#[test]
fn turning_shuffle_off_puts_the_tracks_back_the_way_they_came() {
    let names = ["a.wav", "b.wav", "c.wav", "d.wav", "e.wav"];
    let mut queue = queue_of(&names);
    queue.set_shuffle(true, 7);
    let shuffled = shown(&queue);
    queue.set_shuffle(false, 0);
    assert!(!queue.is_shuffled());
    assert_eq!(shown(&queue), names);
    assert_ne!(shown(&queue), shuffled);
}

#[test]
fn turning_shuffle_off_carries_the_track_heard_to_its_own_place_in_the_list() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav", "d.wav", "e.wav"]);
    queue.advance();
    queue.advance();
    assert_eq!(queue.current(), Some(Path::new("c.wav")));
    queue.set_shuffle(true, 7);
    assert_eq!(queue.current(), Some(Path::new("c.wav")), "the track heard was moved");
    queue.set_shuffle(false, 0);
    assert_eq!(queue.current(), Some(Path::new("c.wav")));
    assert_eq!(shown(&queue), ["c.wav", "d.wav", "e.wav"]);
    assert_eq!(heard_before(&queue), ["a.wav", "b.wav"]);
}

#[test]
fn turning_shuffle_off_leaves_a_track_put_in_front_at_the_end_of_the_list() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav", "d.wav"]);
    queue.set_shuffle(true, 7);
    assert!(queue.play_next(["x.wav"]));
    queue.set_shuffle(false, 0);
    assert_eq!(queue.current(), Some(Path::new("a.wav")));
    assert_eq!(shown(&queue), ["a.wav", "b.wav", "c.wav", "d.wav", "x.wav"]);
}

#[test]
fn tracks_put_next_play_before_the_rest_and_the_rest_still_play_after_them() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav"]);
    queue.advance();
    assert!(queue.play_next(["x.wav", "y.wav"]));
    assert_eq!(shown(&queue), ["b.wav", "x.wav", "y.wav", "c.wav"]);
    assert_eq!(queue.advance().map(name), Some("x.wav".to_owned()));
    assert_eq!(queue.advance().map(name), Some("y.wav".to_owned()));
    assert_eq!(queue.advance().map(name), Some("c.wav".to_owned()));
    assert!(!queue.play_next(Vec::<String>::new()), "no track was put in");
}

#[test]
fn tracks_appended_play_after_everything_already_waiting() {
    let mut queue = queue_of(&["a.wav", "b.wav"]);
    assert!(queue.append(["x.wav", "y.wav"]));
    assert_eq!(shown(&queue), ["a.wav", "b.wav", "x.wav", "y.wav"]);
    assert!(!queue.append(Vec::<String>::new()), "no track was added");
}

#[test]
fn a_track_taken_out_of_the_queue_is_gone_from_the_rest_of_it() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav", "d.wav"]);
    queue.advance();
    assert!(queue.remove(2));
    assert_eq!(shown(&queue), ["b.wav", "c.wav"]);
    assert!(queue.remove(0));
    assert_eq!(shown(&queue), ["b.wav", "c.wav"], "a row that is not there took nothing");
}

#[test]
fn taking_out_a_track_makes_the_queue_hold_it_no_more() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav", "d.wav"]);
    queue.advance();
    assert!(queue.remove(2));
    assert!(!queue.remove(2), "the row was taken already");
    assert!(!queue.remove(99), "there was no such row");
    assert_eq!(shown(&queue), ["b.wav", "c.wav"]);
}

#[test]
fn a_track_taken_out_of_what_is_to_come_leaves_the_track_heard_and_the_ones_before_it() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav", "d.wav", "e.wav"]);
    queue.advance();
    queue.advance();
    assert_eq!(queue.current(), Some(Path::new("c.wav")));
    assert_eq!(shown(&queue), ["c.wav", "d.wav", "e.wav"]);
    assert!(queue.remove(2), "the third row");
    assert_eq!(queue.current(), Some(Path::new("c.wav")));
    assert_eq!(shown(&queue), ["c.wav", "d.wav"]);
    assert_eq!(queue.advance().map(name), Some("d.wav".to_owned()));
    assert_eq!(shown(&queue), ["d.wav"]);
    assert_eq!(heard_before(&queue), ["a.wav", "b.wav", "c.wav"]);
}

#[test]
fn the_track_heard_goes_on_being_reported_until_the_engine_asks_what_plays_next() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav"]);
    queue.advance();
    assert!(queue.remove(0));
    assert_eq!(queue.current(), Some(Path::new("b.wav")), "what is heard changed at once");
    assert_eq!(queue.peek_next().map(name), Some("c.wav".to_owned()));
    assert_eq!(queue.advance().map(name), Some("c.wav".to_owned()));
    assert_eq!(shown(&queue), ["c.wav"]);
}

#[test]
fn the_last_track_heard_can_be_taken_out_and_then_the_queue_ends() {
    let mut queue = queue_of(&["a.wav", "b.wav"]);
    queue.advance();
    assert!(queue.remove(0));
    assert_eq!(queue.current(), Some(Path::new("b.wav")));
    assert_eq!(queue.advance(), None);
    assert_eq!(queue.current(), None, "nothing plays once the queue has ended");
    assert_eq!(queue.peek_next(), None);
}

#[test]
fn a_track_heard_after_leaving_the_queue_cannot_be_taken_out_of_the_queue_again() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav"]);
    queue.advance();
    assert!(queue.remove(0));
    assert_eq!(shown(&queue), ["b.wav", "c.wav"]);
    assert!(!queue.remove(0), "it had left the queue already");
    assert_eq!(queue.current(), Some(Path::new("b.wav")), "what is heard changed at once");
    assert_eq!(shown(&queue), ["b.wav", "c.wav"]);
}

#[test]
fn a_track_moved_takes_the_place_shown_where_it_was_dropped() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav", "d.wav"]);
    assert!(queue.move_item(3, 1));
    assert_eq!(shown(&queue), ["a.wav", "d.wav", "b.wav", "c.wav"]);
    assert!(queue.move_item(1, 3));
    assert_eq!(shown(&queue), ["a.wav", "b.wav", "c.wav", "d.wav"]);
}

#[test]
fn a_track_moved_earlier_leaves_the_track_heard_where_it_is() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav", "d.wav"]);
    queue.advance();
    assert_eq!(queue.current(), Some(Path::new("b.wav")));
    assert_eq!(shown(&queue), ["b.wav", "c.wav", "d.wav"]);
    assert!(queue.move_item(2, 1));
    assert_eq!(queue.current(), Some(Path::new("b.wav")));
    assert_eq!(shown(&queue), ["b.wav", "d.wav", "c.wav"]);
    assert_eq!(queue.advance().map(name), Some("d.wav".to_owned()));
}

#[test]
fn the_track_heard_is_not_a_track_that_can_be_moved() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav"]);
    assert!(!queue.move_item(0, 2), "the track heard was moved");
    assert!(!queue.move_item(2, 0), "another track was put in its place");
    assert_eq!(shown(&queue), ["a.wav", "b.wav", "c.wav"]);
}

#[test]
fn turning_shuffle_off_keeps_a_move_that_was_made_before_shuffling() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav", "d.wav"]);
    assert!(queue.move_item(3, 1));
    assert_eq!(shown(&queue), ["a.wav", "d.wav", "b.wav", "c.wav"]);
    queue.set_shuffle(true, 7);
    queue.set_shuffle(false, 0);
    assert_eq!(shown(&queue), ["a.wav", "d.wav", "b.wav", "c.wav"]);
    assert_eq!(queue.current(), Some(Path::new("a.wav")));
}

#[test]
fn a_move_made_while_shuffled_is_still_a_move_when_shuffle_is_turned_off() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav", "d.wav"]);
    queue.set_shuffle(true, 7);
    let before = shown(&queue);
    let (moved, target) = (before[1].clone(), before[3].clone());
    assert!(queue.move_item(1, 3));
    assert_eq!(shown(&queue), [before[0].clone(), before[2].clone(), target.clone(), moved.clone()]);

    queue.set_shuffle(false, 0);
    let list: Vec<String> = queue.order.iter().map(|track| name(track)).collect();
    assert_eq!(shown(&queue), list, "turning shuffle off did not give the list back");
    let was_at = list.iter().position(|track| *track == moved).expect("in the list");
    let now_at = list.iter().position(|track| *track == target).expect("in the list");
    assert_eq!(now_at + 1, was_at, "the track was not put after the one it was dropped on: {list:?}");
    assert_eq!(queue.current().map(name), Some(before[0].clone()));
}

#[test]
fn clearing_the_queue_leaves_the_track_heard_and_nothing_after_it() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav", "d.wav"]);
    queue.advance();
    assert!(queue.clear_upcoming());
    assert_eq!(shown(&queue), ["b.wav"]);
    assert_eq!(heard_before(&queue), ["a.wav"]);
    assert_eq!(queue.advance(), None);
    assert!(!queue.clear_upcoming(), "there was nothing left to clear");
}

#[test]
fn clearing_the_queue_leaves_a_track_heard_after_leaving_it_where_it_is() {
    let mut queue = queue_of(&["a.wav", "b.wav", "c.wav", "d.wav"]);
    queue.advance();
    queue.remove(0);
    assert!(queue.clear_upcoming());
    assert_eq!(shown(&queue), ["b.wav"]);
    assert_eq!(queue.advance(), None, "the queue came to its end");
}

#[test]
fn a_queue_saved_comes_back_with_the_same_tracks_the_same_order_and_the_same_moment() {
    let scratch = Scratch::new("queue-save");
    let tracks = music(&scratch, &["a.wav", "b.wav", "c.wav", "d.wav"]);
    let file = scratch.path("state/queue");
    let mut queue = Queue::from(&tracks, 0);
    queue.set_repeat(Repeat::All);
    queue.set_shuffle(true, 7);
    queue.advance();
    let moment = Duration::from_millis(61_500);
    queue.save(&file, moment).expect("queue written");

    let (back, position) = Queue::load(&file).expect("the queue comes back");
    assert_eq!(position, moment);
    assert_eq!(back.repeat(), Repeat::All);
    assert!(back.is_shuffled());
    assert_eq!(back.current(), queue.current());
    assert_eq!(shown(&back), shown(&queue));
    assert_eq!(heard_before(&back), heard_before(&queue));
}

#[test]
fn a_queue_written_leaves_its_folder_holding_only_the_file_itself() {
    let scratch = Scratch::new("queue-whole");
    let tracks = music(&scratch, &["a.wav"]);
    let file = scratch.path("state/deeper/queue");
    let queue = Queue::from(&tracks, 0);
    queue.save(&file, Duration::ZERO).expect("queue written");
    let held: Vec<String> = std::fs::read_dir(file.parent().expect("a folder"))
        .expect("readable")
        .map(|entry| entry.expect("an entry").file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(held, ["queue"], "something was left beside the file: {held:?}");
}

#[test]
fn a_queue_saved_while_a_track_leaving_it_plays_comes_back_with_that_track_heard() {
    let scratch = Scratch::new("queue-heard");
    let tracks = music(&scratch, &["a.wav", "b.wav", "c.wav"]);
    let file = scratch.path("state/queue");
    let mut queue = Queue::from(&tracks, 0);
    queue.advance();
    queue.remove(0);
    let moment = Duration::from_millis(2_000);
    queue.save(&file, moment).expect("queue written");

    let (back, position) = Queue::load(&file).expect("the queue comes back");
    assert_eq!(position, moment);
    assert_eq!(back.current(), Some(tracks[1].as_path()));
    assert_eq!(shown(&back), [name(&tracks[1]), name(&tracks[2])]);
}

#[test]
fn tracks_that_are_no_longer_there_are_left_out_and_the_one_heard_moves_on() {
    let scratch = Scratch::new("queue-missing");
    let tracks = music(&scratch, &["a.wav", "b.wav", "c.wav", "d.wav"]);
    let file = scratch.path("state/queue");
    let mut queue = Queue::from(&tracks, 0);
    queue.advance();
    queue.advance();
    queue.save(&file, Duration::from_millis(5_000)).expect("queue written");

    std::fs::remove_file(&tracks[1]).expect("removed");
    std::fs::remove_file(&tracks[2]).expect("removed");
    let (back, position) = Queue::load(&file).expect("the queue comes back");
    assert_eq!(position, Duration::ZERO, "the moment stayed with a track that is gone");
    assert_eq!(back.current(), Some(tracks[3].as_path()));
    assert_eq!(shown(&back), [name(&tracks[3])]);
}

#[test]
fn the_moment_is_kept_when_the_track_heard_is_still_there() {
    let scratch = Scratch::new("queue-kept");
    let tracks = music(&scratch, &["a.wav", "b.wav", "c.wav"]);
    let file = scratch.path("state/queue");
    let mut queue = Queue::from(&tracks, 0);
    queue.advance();
    let moment = Duration::from_millis(2_250);
    queue.save(&file, moment).expect("queue written");

    std::fs::remove_file(&tracks[2]).expect("removed");
    let (back, position) = Queue::load(&file).expect("the queue comes back");
    assert_eq!(position, moment);
    assert_eq!(back.current(), Some(tracks[1].as_path()));
    assert_eq!(shown(&back), [name(&tracks[1])]);
}

#[test]
fn a_queue_of_nothing_saved_comes_back_empty() {
    let scratch = Scratch::new("queue-empty");
    let file = scratch.path("state/queue");
    let queue = Queue::from(Vec::<String>::new(), 0);
    queue.save(&file, Duration::from_millis(9_000)).expect("queue written");
    let (back, position) = Queue::load(&file).expect("the queue comes back");
    assert_eq!(position, Duration::ZERO, "a moment belongs to a track, and there is none");
    assert_eq!(back.current(), None);
    assert_eq!(shown(&back), Vec::<String>::new());
}

#[test]
fn a_file_that_does_not_hold_a_queue_is_none_and_never_a_panic() {
    let scratch = Scratch::new("queue-corrupt");
    let file = scratch.path("state/queue");
    let broken = [
        "",
        "not a queue at all\n",
        "qmus queue 1\n",
        "qmus queue 1\nrepeat maybe\n",
        "qmus queue 1\norder 2\na.wav\nb.wav\nplay 0 0\nrepeat off\nshuffle off\nposition 0 0\nat 0\n",
        "qmus queue 1\norder 2\na.wav\nb.wav\nplay 0\nrepeat off\nshuffle off\nposition 0 0\nat 0\n",
        "qmus queue 1\norder 2\na.wav\nb.wav\nplay 0 1\nat 9\nrepeat off\nshuffle off\nposition 0 0\n",
        "qmus queue 1\nrepeat off\nshuffle off\nposition 0 1000000000\nat 0\norder 0\nplay\n",
        "qmus queue 1\nrepeat off\nshuffle off\nposition 0\nat 0\norder 0\nplay\n",
        "qmus queue 1\nrepeat off\nshuffle maybe\nposition 0 0\nat 0\norder 0\nplay\n",
        "qmus queue 1\nsurprise yes\nrepeat off\nshuffle off\nposition 0 0\nat 0\norder 0\nplay\n",
        "qmus queue 1\nrepeat off\nshuffle off\nposition 0 0\nat 0\norder 9\na.wav\nplay 0\n",
        "qmus queue 9\nrepeat off\nshuffle off\nposition 0 0\nat 0\norder 0\nplay\n",
    ];
    for text in broken {
        std::fs::write(&file, text).expect("file written");
        assert_eq!(Queue::load(&file), None, "this was taken for a queue:\n{text}");
    }
}

#[test]
fn a_queue_that_was_never_written_is_none() {
    let scratch = Scratch::new("queue-nowhere");
    assert_eq!(Queue::load(&scratch.path("state/queue")), None);
    assert_eq!(Queue::load(&scratch.path("no-such-folder/queue")), None);
}
