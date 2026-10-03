//! The menu of a track: right-click a row, or press the menu key on it, to play it next, put it at
//! the end of the queue, add it to a playlist or go to its album or artist.

use std::path::PathBuf;

use qframe::prelude::*;
use qframe::widgets::{ContextItem, Toast};

use super::{Msg, Music, Page};
use crate::playlist;

/// What the menu of a track asks for, the track named by its row.
#[derive(Debug, Clone)]
pub enum TrackMenu {
    /// Play it right after the track heard.
    Next(usize),
    /// Put it at the end of the queue.
    Append(usize),
    /// Add it to the end of the playlist at this path.
    AddTo(usize, PathBuf),
    /// Open its album.
    Album(usize),
    /// Open its artist.
    Artist(usize),
}

impl Music {
    /// The entries of the menu of row `at` of the track table, built for a table that outlives
    /// this frame: everything it needs is copied in.
    pub(super) fn track_menu(&self) -> impl Fn(usize) -> Vec<ContextItem<Msg>> + 'static {
        let list = self.list.clone();
        let playlists: Vec<(String, PathBuf)> =
            self.shelf.all.iter().map(|(entry, _)| (entry.name.clone(), entry.path.clone())).collect();
        let labels = [
            t!("music.menu.play"),
            t!("music.menu.next"),
            t!("music.menu.append"),
            t!("music.menu.playlist"),
            t!("music.menu.album"),
            t!("music.menu.artist"),
        ];
        move |at| {
            let Some(row) = list.get(at).copied() else { return Vec::new() };
            let [play, next, append, add, album, artist] = labels.clone();
            let mut items = vec![
                ContextItem::new(play, Msg::Play(at)).icon("media-play"),
                ContextItem::new(next, Msg::Menu(TrackMenu::Next(row))).icon("music-queue"),
                ContextItem::new(append, Msg::Menu(TrackMenu::Append(row))),
            ];
            if !playlists.is_empty() {
                let lists = playlists
                    .iter()
                    .map(|(name, path)| ContextItem::new(name.clone(), Msg::Menu(TrackMenu::AddTo(row, path.clone()))));
                items.push(ContextItem::submenu(add, lists).icon("music-playlist"));
            }
            items.push(ContextItem::new(album, Msg::Menu(TrackMenu::Album(row))).icon("music-album"));
            items.push(ContextItem::new(artist, Msg::Menu(TrackMenu::Artist(row))).icon("music-artist"));
            items
        }
    }

    /// Carries out `msg`.
    pub(super) fn menu_msg(&mut self, msg: TrackMenu) -> Command<Msg> {
        match msg {
            TrackMenu::Next(row) | TrackMenu::Append(row) if self.current.is_none() => self.play(row),
            TrackMenu::Next(row) => {
                let Some(track) = self.tracks().get(row) else { return Command::none() };
                let (path, title) = (track.path.clone(), track.title.clone());
                self.queue.play_next([path]);
                self.follow();
                self.refresh_lists();
                Command::toast(Toast::info(t!("music.menu.next-done", name = title.as_str())).key("queue"))
            }
            TrackMenu::Append(row) => {
                let Some(track) = self.tracks().get(row) else { return Command::none() };
                let (path, title) = (track.path.clone(), track.title.clone());
                self.queue.append([path]);
                self.follow();
                self.refresh_lists();
                Command::toast(Toast::info(t!("music.menu.append-done", name = title.as_str())).key("queue"))
            }
            TrackMenu::AddTo(row, list) => self.add_to_playlist(row, &list),
            TrackMenu::Album(row) => {
                self.album_open = self.albums.iter().position(|album| album.tracks.contains(&row));
                self.cursor = None;
                self.query.clear();
                self.show_page(Page::Albums)
            }
            TrackMenu::Artist(row) => {
                self.artist_open = self.artists.iter().position(|artist| artist.tracks.contains(&row));
                self.cursor = None;
                self.query.clear();
                self.show_page(Page::Artists)
            }
        }
    }

    /// Adds the track of `row` to the end of the playlist at `list`, keeping what the playlist
    /// already says of its other tracks.
    fn add_to_playlist(&mut self, row: usize, list: &std::path::Path) -> Command<Msg> {
        let Some(track) = self.tracks().get(row) else { return Command::none() };
        let added = playlist::Track {
            path: track.path.clone(),
            title: Some(if track.artist.is_empty() {
                track.title.clone()
            } else {
                format!("{} - {}", track.artist, track.title)
            }),
            duration: track.duration,
        };
        let title = track.title.clone();
        let written = playlist::read(list).and_then(|read| {
            let mut items: Vec<playlist::Track> = read
                .items
                .into_iter()
                .map(|item| playlist::Track { path: item.path, title: item.title, duration: item.duration })
                .collect();
            items.push(added);
            playlist::replace(list, &items).map(|()| read.name)
        });
        self.read_playlists();
        self.refresh_shelf();
        match written {
            Ok(name) => Command::toast(
                Toast::success(t!("music.menu.added", name = title.as_str(), playlist = name.as_str())).key("playlist"),
            ),
            Err(error) => Command::toast(
                Toast::danger(t!("music.menu.not-added", name = title.as_str()))
                    .body(error.to_string())
                    .key("playlist"),
            ),
        }
    }
}
