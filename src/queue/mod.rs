//! The play queue: the track heard now, the tracks that follow it, and the tracks heard before.
//!
//! The queue is a model of its own. It names tracks by where they are played from and answers
//! what plays next, so the engine can open the next track early for a change that is not heard and
//! the screen can draw the queue, and neither of them decides anything about the order.
//!
//! The order the tracks were given in is kept apart from the order they play in. That is what lets
//! shuffling rearrange only what is still to come, and what lets turning it off put the tracks
//! back the way they came without the track being heard moving.

use crate::library::Location;
use std::time::Duration;

mod memory;
mod random;

/// How far a track has to be heard before asking for the one before it starts that one again.
const RESTART_AFTER: Duration = Duration::from_secs(3);

/// What the queue does when the last track has been heard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    /// The list ends and stays there.
    Off,
    /// The list begins again.
    All,
    /// The track heard plays again.
    One,
}

/// The tracks to play, the one playing now, and how the queue goes on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queue {
    /// The tracks the queue holds, in the order they were given. Shuffling never touches this, so
    /// turning shuffle off finds the order again.
    order: Vec<Location>,
    /// The place in `order` of each track, in the order they play. A permutation of `order`: the
    /// queue holds every track exactly once, however it is arranged.
    play: Vec<usize>,
    /// The place in `play` of the track heard now. It can be one past the end, which is a queue
    /// that has run out with a track still going.
    cursor: usize,
    /// A track that has left the queue but is still being heard. The queue has already moved on to
    /// the track that plays next, and this one is reported until the engine asks for that next one.
    still_playing: Option<Location>,
    /// Whether the tracks after the one heard are in a shuffled order.
    shuffled: bool,
    /// What happens when the last track has been heard.
    repeat: Repeat,
}

/// What a call to [`Queue::advance`] does to the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// The queue moves to this place in the play order.
    Move(usize),
    /// The track heard plays again, whatever has become of the queue.
    Stay,
}

impl Queue {
    /// A queue of `tracks` that plays from the one at `start` to the end of the list. A `start`
    /// past the last track names no track, so the whole list plays.
    #[must_use]
    pub fn from(tracks: impl IntoIterator<Item = impl Into<Location>>, start: usize) -> Self {
        let order: Vec<Location> = tracks.into_iter().map(Into::into).collect();
        let start = if start < order.len() { start } else { 0 };
        let play: Vec<usize> = (start..order.len()).collect();
        Self { order, play, cursor: 0, still_playing: None, shuffled: false, repeat: Repeat::Off }
    }

    /// The track heard now, or `None` when the queue has come to its end and nothing plays.
    #[must_use]
    pub fn current(&self) -> Option<&Location> {
        self.still_playing.as_ref().or_else(|| self.at(self.cursor))
    }

    /// The queue as it is shown: the track heard now first, then the ones that follow it in the
    /// order they will play. An empty queue shows nothing.
    pub fn upcoming(&self) -> impl Iterator<Item = &Location> {
        let after = self.play.iter().skip(self.cursor).map(|&held| &self.order[held]);
        self.still_playing.iter().chain(after)
    }

    /// The tracks already heard, oldest first, and nothing at all when the queue has only just
    /// been made.
    pub fn history(&self) -> impl Iterator<Item = &Location> {
        self.play.iter().take(self.cursor).map(|&held| &self.order[held])
    }

    /// Whether the tracks after the one heard are in a shuffled order.
    #[must_use]
    pub fn is_shuffled(&self) -> bool {
        self.shuffled
    }

    /// What the queue does when the last track has been heard.
    #[must_use]
    pub fn repeat(&self) -> Repeat {
        self.repeat
    }

    /// Sets what the queue does when the last track has been heard.
    pub fn set_repeat(&mut self, repeat: Repeat) {
        self.repeat = repeat;
    }

    /// The track that plays once the one heard has been heard, and the queue moves to it. `None`
    /// when the list ends and nothing plays then, in which case the queue stays where it is.
    pub fn advance(&mut self) -> Option<&Location> {
        let step = self.step();
        // A track that has left the queue is heard until the engine asks what plays next, and the
        // asking forgets it whether or not anything plays next.
        let heard = self.still_playing.take();
        match step? {
            Step::Stay => {
                self.still_playing = heard;
                self.current()
            }
            Step::Move(place) => {
                self.cursor = place;
                self.at(place)
            }
        }
    }

    /// The track [`Queue::advance`] would hand over, said without moving the queue: the engine opens
    /// it a while before it is wanted so that the change from one to the next is not heard.
    #[must_use]
    pub fn peek_next(&self) -> Option<&Location> {
        match self.step()? {
            Step::Stay => self.current(),
            Step::Move(place) => self.at(place),
        }
    }

    /// The track to play when the person asks for the one before the one heard: a track heard for
    /// three seconds or more starts again, one heard for less takes the queue back a track, and one
    /// heard for less with nothing before it starts again.
    pub fn previous(&mut self, position: Duration) -> Option<&Location> {
        if position < RESTART_AFTER && self.cursor > 0 {
            // A track that has already left the queue is the one heard while the queue has moved
            // on, so letting go of it and going back a track together land on the track before it.
            self.still_playing = None;
            self.cursor -= 1;
        }
        self.current()
    }

    /// Shuffles the tracks after the one heard, or puts them back the way they were given. The
    /// track heard keeps its place and goes on playing either way. The same `seed` always shuffles
    /// the same way, so a shuffle that has been seen can be told from a new one.
    pub fn set_shuffle(&mut self, on: bool, seed: u64) {
        if on {
            let from = self.after_current();
            random::shuffle(&mut self.play[from..], seed);
        } else {
            self.restore_order();
        }
        self.shuffled = on;
    }

    /// Puts tracks in the queue so that they play right after the one heard, before the tracks
    /// that were already waiting.
    pub fn play_next(&mut self, tracks: impl IntoIterator<Item = impl Into<Location>>) -> bool {
        let added = self.add(tracks);
        if added == 0 {
            return false;
        }
        let from = self.order.len() - added;
        let at = self.after_current();
        for (ahead, place) in (from..self.order.len()).enumerate() {
            self.play.insert(at + ahead, place);
        }
        true
    }

    /// Puts tracks at the end of the queue, to play after everything already waiting in it.
    pub fn append(&mut self, tracks: impl IntoIterator<Item = impl Into<Location>>) -> bool {
        let added = self.add(tracks);
        if added == 0 {
            return false;
        }
        self.play.extend(self.order.len() - added..self.order.len());
        true
    }

    /// Takes the track shown at `row` out of the queue, `row` counting the one heard as the first.
    /// Taking out the one heard lets the queue move on to the next track without changing what is
    /// heard: the track goes on being reported until the engine asks what plays next. Taking it out
    /// again is refused, because it left the queue the first time.
    pub fn remove(&mut self, row: usize) -> bool {
        if row == 0 {
            return self.remove_heard();
        }
        let Some(place) = self.play_place(row) else { return false };
        if place >= self.play.len() {
            return false;
        }
        self.remove_at(place);
        true
    }

    /// Moves the track shown at `from` to the place shown at `to`, both counting the one heard as
    /// the first. The one heard is left alone, so a track can be put at any place but the first
    /// and cannot be taken from it; the queue moves the track in the list too, so that turning
    /// shuffle off afterwards leaves the tracks that were not moved where they were.
    pub fn move_item(&mut self, from: usize, to: usize) -> bool {
        if from == 0 || to == 0 {
            // The first row is the track heard, and it goes on playing where it is.
            return false;
        }
        let (Some(was), Some(becomes)) = (self.play_place(from), self.play_place(to)) else {
            return false;
        };
        if was >= self.play.len() || becomes >= self.play.len() || was == becomes {
            return false;
        }
        let (held, over) = (self.play[was], self.play[becomes]);
        self.reorder(held, over, was < becomes);
        // Moving the track in the list leaves the queue in the same order, so the row it was shown
        // in still holds it and only the order of the rows changes.
        let moved = self.play[was];
        self.play.remove(was);
        self.play.insert(becomes, moved);
        true
    }

    /// Takes every track out of the queue but the one heard, which goes on playing.
    pub fn clear_upcoming(&mut self) -> bool {
        let keep = self.after_current();
        if keep >= self.play.len() {
            return false;
        }
        self.play.truncate(keep);
        true
    }

    /// The track at the given place of the play order.
    fn at(&self, place: usize) -> Option<&Location> {
        self.play.get(place).map(|&held| &self.order[held])
    }

    /// Where the play order stands when the next call to [`Queue::advance`] arrives, said without
    /// moving. Both [`Queue::advance`] and [`Queue::peek_next`] are answered from this, so what the
    /// engine opens early and what it is later handed are always the same track.
    fn step(&self) -> Option<Step> {
        let after = self.after_current();
        match self.repeat {
            Repeat::One => Some(Step::Stay),
            Repeat::Off => self.play.get(after).map(|_| Step::Move(after)),
            Repeat::All => match self.play.get(after) {
                Some(_) => Some(Step::Move(after)),
                None if self.play.is_empty() => None,
                None => Some(Step::Move(0)),
            },
        }
    }

    /// The place in the play order where the tracks after the one heard begin.
    fn after_current(&self) -> usize {
        let after = self.cursor + usize::from(self.still_playing.is_none());
        after.min(self.play.len())
    }

    /// The place in the play order of the track shown at `row`, which is `None` for the track that
    /// has left the queue but is still heard: it is shown, but there is no place for it.
    fn play_place(&self, row: usize) -> Option<usize> {
        match self.still_playing {
            Some(_) => row.checked_sub(1).map(|ahead| ahead + self.cursor),
            None => Some(row + self.cursor),
        }
    }

    /// Puts the tracks at the end of the list and says how many there were. They join the queue at
    /// their own places, so nothing already waiting moves.
    fn add(&mut self, tracks: impl IntoIterator<Item = impl Into<Location>>) -> usize {
        let before = self.order.len();
        self.order.extend(tracks.into_iter().map(Into::into));
        self.order.len() - before
    }

    /// Takes the track heard out of the queue: the queue moves on to the next track and the track
    /// goes on being reported until the engine asks what plays next.
    fn remove_heard(&mut self) -> bool {
        if self.still_playing.is_some() {
            // It left the queue the first time it was taken out; there is nothing left to take out,
            // and saying so keeps what is heard from changing under the person.
            return false;
        }
        let Some(&held) = self.play.get(self.cursor) else { return false };
        self.still_playing = Some(self.order[held].clone());
        self.remove_at(self.cursor);
        true
    }

    /// Takes the track at the given place out of the list and the play order, and carries the
    /// places of the tracks after it along.
    fn remove_at(&mut self, place: usize) {
        let held = self.play[place];
        self.order.remove(held);
        self.play.remove(place);
        for other in &mut self.play {
            if *other > held {
                *other -= 1;
            }
        }
        if place < self.cursor {
            self.cursor -= 1;
        }
    }

    /// Moves the track held at place `held` of the list next to the track at `over`: `after` it
    /// when the play order moved it down past that track, before it when up. The play order is
    /// carried along so that it still names the same tracks in the same order.
    fn reorder(&mut self, held: usize, over: usize, after: bool) {
        let mut places: Vec<usize> = (0..self.order.len()).collect();
        let moved = places.remove(held);
        let beside = places.iter().position(|place| *place == over).unwrap_or(places.len());
        places.insert((beside + usize::from(after)).min(places.len()), moved);
        let mut now = vec![0; places.len()];
        for (new, old) in places.iter().enumerate() {
            now[*old] = new;
        }
        self.order = places.iter().map(|old| self.order[*old].clone()).collect();
        for place in &mut self.play {
            *place = now[*place];
        }
    }

    /// Puts the tracks back the way they were given and carries the track heard over to the place
    /// it has there.
    fn restore_order(&mut self) {
        let heard = self.play.get(self.cursor).copied();
        self.play = (0..self.order.len()).collect();
        self.cursor = heard.unwrap_or(self.cursor).min(self.order.len().saturating_sub(1));
    }
}

#[cfg(test)]
mod tests;
