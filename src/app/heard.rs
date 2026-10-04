//! What an account's track heard asks of its account: its album's cover, and, when the person
//! leaves it on, being told the track is heard, for the account's own play counts.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use qframe::prelude::*;

use super::{Art, COVER, Msg, Music};
use crate::audio::State;
use crate::library::Location;
use crate::sources::Client;

/// How long a track is heard before it counts as heard, when half of it is longer: the rule of the
/// services that keep play counts.
const COUNTED: Duration = Duration::from_secs(4 * 60);

impl Music {
    /// The account `key` logged in to in this run, to ask things of.
    pub(super) fn client_of(&self, key: &str) -> Option<Client> {
        self.accounts.iter().any(|account| account.key == key).then(|| self.logins.get(key).cloned()).flatten()
    }

    /// Reads the cover of the account's track loaded, as [`read_art`](Music::read_art) reads a
    /// file's: once for its album, kept for the desktop, nothing while the account is not logged in.
    pub(super) fn read_remote_art(&mut self) -> Command<Msg> {
        let Some(track) = self.current() else { return Command::none() };
        let Location::Remote { source, .. } = &track.location else { return Command::none() };
        // The account stands in for the folder, so two accounts' albums of one name are two.
        let key = (PathBuf::from(format!("account:{}", source.0)), track.album.clone());
        let (cover, client) = (self.remote_covers.get(&track.location).cloned(), self.client_of(&source.0));
        if self.art.as_ref().is_some_and(|shown| shown.key == key) {
            return Command::none();
        }
        if let Some(at) = self.covers.iter().position(|kept| kept.key == key) {
            let kept = self.covers.remove(at);
            self.art = Some(kept.clone());
            self.covers.push(kept);
            return Command::none();
        }
        let (Some(cover), Some(client)) = (cover, client) else {
            // Read as having none, so the card stands in its place.
            self.art = Some(Art { key, read: true, image: None, file: None });
            return Command::none();
        };
        let cache = self.machine.covers.clone();
        self.art = Some(Art { key: key.clone(), read: false, image: None, file: None });
        Command::perform(move || {
            let bytes = client.cover(&cover, COVER.0.max(COVER.1)).ok();
            let image = bytes
                .as_ref()
                .and_then(|bytes| qframe::widgets::ImageData::decode_bytes(bytes, COVER).ok())
                .map(Arc::new);
            let name = crate::art::name_of(&key.0, &key.1);
            let file = bytes.zip(cache).and_then(|(bytes, cache)| crate::art::keep_bytes(&bytes, &cache, &name));
            Msg::Art(Art { key, read: true, image, file })
        })
    }

    /// Tells the account of the track heard that it has begun, and once half of it or four
    /// minutes have been heard, that it has been heard; each once a play, and only while the
    /// person leaves reporting on.
    pub(super) fn report_heard(&mut self) -> Command<Msg> {
        let Some(heard) = self.status.track.clone() else { return Command::none() };
        let Location::Remote { source, id } = heard.clone() else { return Command::none() };
        if self.reported.0.as_ref() != Some(&heard) {
            self.reported = (Some(heard.clone()), false, false);
        }
        if self.status.state == State::Ended {
            return self.heard_through();
        }
        let position = self.status.position;
        let length = self.places.get(&heard).and_then(|row| self.tracks().get(*row)).and_then(|track| track.duration);
        let counted = length.map_or(COUNTED, |length| (length / 2).min(COUNTED));
        let done = position >= counted && !self.reported.2;
        let began = position > Duration::ZERO && !self.reported.1;
        if !(began || done) {
            return Command::none();
        }
        self.reported.1 = true;
        self.reported.2 |= done;
        self.tell(&source.0, id, began, done)
    }

    /// The track that was heard has been heard to its end, short of the share that counts or not:
    /// its account is told, unless it was already.
    pub(super) fn heard_through(&mut self) -> Command<Msg> {
        let (Some(Location::Remote { source, id }), began, false) = self.reported.clone() else {
            return Command::none();
        };
        self.reported.1 = true;
        self.reported.2 = true;
        self.tell(&source.0, id, !began, true)
    }

    /// Tells the account `key` its track `id` began, was heard, or both, while the person leaves
    /// reporting on for it.
    fn tell(&self, key: &str, id: String, began: bool, done: bool) -> Command<Msg> {
        let wanted = self.accounts.iter().any(|account| account.key == key && account.scrobble);
        let Some(client) = self.client_of(key).filter(|_| wanted) else { return Command::none() };
        Command::perform(move || {
            // A report that does not arrive is not worth a word: the music goes on all the same.
            if began {
                let _ = client.scrobble(&id, false);
            }
            if done {
                let _ = client.scrobble(&id, true);
            }
            Msg::Reported
        })
    }
}
