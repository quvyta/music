//! The accounts' music in the library: what each account lists, beside the files of this computer.
//!
//! What an account listed the last time is kept in qmus's cache and shown at once on the next
//! start, before any login; once the account is logged in to it is asked again, and what it says
//! replaces the old listing. An account that cannot be reached keeps its tracks on screen, faint.

use std::sync::Arc;

use qframe::prelude::*;

use super::accounts::Standing;
use super::{Msg, Music};
use crate::library::{self, Location, SourceKey, Track};
use crate::sources::{RemoteTrack, SourceError, cache};

impl Music {
    /// Reads what each account listed the last time, off the drawing thread.
    pub(super) fn read_listings(&self) -> Command<Msg> {
        let Some(folder) = self.machine.listings.clone() else { return Command::none() };
        let commands = self.accounts.iter().map(|account| {
            let (key, file) = (account.key.clone(), cache::file_of(&folder, &account.key));
            Command::perform(move || Msg::Listed(key, cache::read(&file)))
        });
        Command::batch(commands)
    }

    /// Asks the account `key`, logged in to just now, for everything it has, off the drawing
    /// thread, and keeps what it says for the next start.
    pub(super) fn fetch_catalogue(&self, key: &str) -> Command<Msg> {
        let Some(client) = self.client_of(key) else { return Command::none() };
        let file = self.machine.listings.as_ref().map(|folder| cache::file_of(folder, key));
        let key = key.to_owned();
        Command::perform(move || {
            let answer = client.catalogue();
            if let (Ok(tracks), Some(file)) = (&answer, file) {
                cache::write(&file, tracks);
            }
            Msg::Catalogue(key, answer)
        })
    }

    /// What the cache kept of the account `key`: shown unless the account has answered already.
    pub(super) fn listed(&mut self, key: String, tracks: Vec<RemoteTrack>) -> Command<Msg> {
        if tracks.is_empty() || self.listings.contains_key(&key) || !self.has_account(&key) {
            return Command::none();
        }
        self.take_listing(key, tracks);
        self.relist()
    }

    /// What the account `key` answered when asked for everything it has.
    pub(super) fn catalogue(&mut self, key: String, answer: Result<Vec<RemoteTrack>, SourceError>) -> Command<Msg> {
        // An account removed while it was asked is not brought back.
        if !self.has_account(&key) {
            return Command::none();
        }
        match answer {
            Ok(tracks) => self.take_listing(key, tracks),
            // What was listed before stays, faint, with the reason on the account's row.
            Err(error) => {
                self.standing.insert(key, Standing::Failed(error));
            }
        }
        self.relist()
    }

    /// Forgets what the account `key` listed, on screen and in the cache.
    pub(super) fn forget_listing(&mut self, key: &str) -> Command<Msg> {
        if let Some(folder) = &self.machine.listings {
            cache::forget(&cache::file_of(folder, key));
        }
        self.listings.remove(key);
        self.remote_covers.retain(|location, _| location.source().is_none_or(|source| source.0 != key));
        // The marks of where each track is from go with the last account.
        self.relist()
    }

    /// Shows the library again: the files of this computer and every account's tracks, in one order.
    pub(super) fn relist(&mut self) -> Command<Msg> {
        let mut all: Vec<Track> = self.local.as_deref().unwrap_or_default().to_vec();
        for tracks in self.listings.values() {
            all.extend(tracks.iter().cloned());
        }
        library::sort(&mut all);
        self.list_tracks(Arc::from(all))
    }

    /// Whether the account whose tracks these are cannot be reached now, so its tracks are faint.
    pub(super) fn unreachable(&self, track: &Track) -> bool {
        track.location.source().is_some_and(|source| matches!(self.standing.get(&source.0), Some(Standing::Failed(_))))
    }

    /// Keeps what the account `key` listed as its tracks, with the cover each one names.
    fn take_listing(&mut self, key: String, listed: Vec<RemoteTrack>) {
        for track in &listed {
            if let Some(cover) = &track.cover {
                let location = Location::Remote { source: SourceKey(key.clone()), id: track.id.clone() };
                self.remote_covers.insert(location, cover.clone());
            }
        }
        let tracks = tracks_of(&key, listed);
        self.listings.insert(key, tracks);
    }

    /// Whether `key` is one of the person's accounts.
    fn has_account(&self, key: &str) -> bool {
        self.accounts.iter().any(|account| account.key == key)
    }
}

/// The tracks the account `key` listed, as tracks of the library.
fn tracks_of(key: &str, listed: Vec<RemoteTrack>) -> Vec<Track> {
    listed
        .into_iter()
        .map(|track| Track {
            location: Location::Remote { source: SourceKey(key.to_owned()), id: track.id },
            title: track.title,
            artist: track.artist,
            album: track.album,
            number: track.number,
            duration: track.duration,
            playable: true,
        })
        .collect()
}
