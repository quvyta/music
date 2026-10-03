//! The playlists page: the person's playlists, the tracks of the one opened, the queue kept as a
//! new playlist, and a playlist removed once the person has said so.

use std::io;
use std::sync::Arc;

use qframe::prelude::*;
use qframe::widgets::{
    Button, Column, ColumnWidth, EmptyState, Field, Modal, Table, TableCell, TableRow, TextInput, Toast,
};

use super::{Msg, Music, Page};
use crate::library;
use crate::playlist::{self, Entry};

/// The table of the playlists.
pub(super) const PLAYLISTS: &str = "playlists";

/// The name box of the dialog that keeps the queue.
const NAME: &str = "playlist-name";

/// How wide the playlists' dialogs are.
const DIALOG: u16 = 52;

/// What happens on the playlists page and in its dialogs.
#[derive(Debug, Clone)]
pub enum ListMsg {
    /// The cursor moved to this row of the playlists table.
    Select(usize),
    /// This row of the playlists table was chosen: its tracks open.
    Open(usize),
    /// `ctrl+s`: the dialog that keeps the queue as a playlist opens.
    SaveAsk,
    /// The name box holds this now.
    Typed(String),
    /// The queue is kept under the name typed.
    Save,
    /// Delete on a playlist: the dialog that asks whether to remove it opens.
    RemoveAsk,
    /// The person said yes: the playlist's file goes.
    Remove,
    /// The dialog closes with nothing done.
    Close,
}

/// A dialog of the playlists page.
#[derive(Debug, Clone)]
pub(super) enum Dialog {
    /// Keeping the queue as a playlist: the name typed, and why it cannot be used when it cannot.
    Save { name: String, problem: Option<String> },
    /// Asking whether to remove this playlist.
    Remove(Entry),
}

/// The person's playlists as the page lists them: each with how many tracks it names.
#[derive(Debug, Clone, Default)]
pub(super) struct Shelf {
    /// Every playlist in the folder, with its number of tracks.
    pub(super) all: Vec<(Entry, usize)>,
    /// The places in `all` of those the search finds, and their table rows.
    pub(super) shown: (Vec<usize>, Arc<[TableRow]>),
    /// The row under the cursor.
    pub(super) cursor: Option<usize>,
    /// The playlist opened, with its tracks.
    pub(super) open: Option<playlist::Playlist>,
}

impl Music {
    /// Reads the playlists folder again.
    pub(super) fn read_playlists(&mut self) {
        let Some(folder) = &self.machine.playlists else { return };
        self.shelf.all = playlist::list(folder)
            .into_iter()
            .map(|entry| {
                let count = playlist::read(&entry.path).map_or(0, |list| list.items.len());
                (entry, count)
            })
            .collect();
    }

    /// Narrows the playlists to those the search finds, keeping the cursor within them.
    pub(super) fn refresh_shelf(&mut self) {
        let shown: Vec<usize> =
            (0..self.shelf.all.len()).filter(|at| library::found(&self.shelf.all[*at].0.name, &self.query)).collect();
        let rows = shown
            .iter()
            .map(|at| {
                let (entry, count) = &self.shelf.all[*at];
                TableRow::new([TableCell::new(entry.name.clone()), TableCell::new(count.to_string())])
            })
            .collect();
        self.shelf.cursor = match self.shelf.cursor {
            _ if shown.is_empty() => None,
            Some(at) => Some(at.min(shown.len() - 1)),
            None => Some(0),
        };
        self.shelf.shown = (shown, rows);
    }

    /// The rows of the tracks of the playlist opened that are in the folder shown, in its order.
    pub(super) fn playlist_rows(&self) -> Vec<usize> {
        let Some(open) = &self.shelf.open else { return Vec::new() };
        open.items.iter().filter_map(|item| self.places.get(&item.path).copied()).collect()
    }

    /// The name the opened playlist is shown under, with how many of its tracks are elsewhere.
    pub(super) fn playlist_title(&self) -> Option<String> {
        let open = self.shelf.open.as_ref()?;
        let away = open.items.len() - self.playlist_rows().len();
        if away == 0 {
            return Some(open.name.clone());
        }
        let away = u32::try_from(away).unwrap_or(u32::MAX);
        Some(format!("{} · {}", open.name, t!("music.playlists.away", n = away)))
    }

    /// Carries out `msg`.
    pub(super) fn list_msg(&mut self, msg: ListMsg) -> Command<Msg> {
        match msg {
            ListMsg::Select(at) => self.shelf.cursor = Some(at),
            ListMsg::Open(at) => {
                self.shelf.cursor = Some(at);
                let entry = self.shelf.shown.0.get(at).and_then(|at| self.shelf.all.get(*at)).map(|(entry, _)| entry);
                let Some(entry) = entry else { return Command::none() };
                match playlist::read(&entry.path) {
                    Ok(open) => self.shelf.open = Some(open),
                    Err(error) => {
                        return Command::toast(
                            Toast::danger(t!("music.playlists.unreadable", name = entry.name.as_str()))
                                .body(error.to_string())
                                .key("playlist"),
                        );
                    }
                }
                self.cursor = None;
                return self.show_page(Page::Playlists);
            }
            ListMsg::SaveAsk => {
                if self.machine.playlists.is_none() || self.queue.upcoming().next().is_none() {
                    return Command::none();
                }
                let name = self.current().map(|track| track.album.clone()).unwrap_or_default();
                self.dialog = Some(Dialog::Save { name, problem: None });
                return Command::focus(NAME);
            }
            ListMsg::Typed(typed) => {
                if let Some(Dialog::Save { name, problem }) = &mut self.dialog {
                    *name = typed;
                    *problem = None;
                }
            }
            ListMsg::Save => return self.save_queue(),
            ListMsg::RemoveAsk => {
                let entry =
                    self.shelf.cursor.and_then(|at| self.shelf.shown.0.get(at)).map(|at| &self.shelf.all[*at].0);
                if let Some(entry) = entry {
                    self.dialog = Some(Dialog::Remove(entry.clone()));
                }
            }
            ListMsg::Remove => return self.remove_playlist(),
            ListMsg::Close => self.dialog = None,
        }
        Command::none()
    }

    /// Keeps the queue, from the track heard on, as a playlist under the name typed.
    fn save_queue(&mut self) -> Command<Msg> {
        let (Some(Dialog::Save { name, .. }), Some(folder)) = (&self.dialog, &self.machine.playlists) else {
            return Command::none();
        };
        let items: Vec<playlist::Track> = self
            .queue
            .upcoming()
            .map(|path| {
                let track = self.places.get(path).and_then(|row| self.tracks().get(*row));
                playlist::Track {
                    path: path.to_path_buf(),
                    title: track.map(|track| {
                        if track.artist.is_empty() {
                            track.title.clone()
                        } else {
                            format!("{} - {}", track.artist, track.title)
                        }
                    }),
                    duration: track.and_then(|track| track.duration),
                }
            })
            .collect();
        match playlist::write(folder, name, &items) {
            Ok(path) => {
                let saved = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
                self.dialog = None;
                self.read_playlists();
                self.refresh_shelf();
                Command::toast(Toast::success(t!("music.playlists.saved", name = saved.as_str())).key("playlist"))
            }
            Err(error) => {
                let problem = match error.kind() {
                    io::ErrorKind::AlreadyExists => t!("music.playlists.taken"),
                    io::ErrorKind::InvalidInput => t!("music.playlists.unnamed"),
                    _ => error.to_string(),
                };
                if let Some(Dialog::Save { problem: shown, .. }) = &mut self.dialog {
                    *shown = Some(problem);
                }
                Command::none()
            }
        }
    }

    /// Removes the playlist the person said yes to: its file only, never the music it lists.
    fn remove_playlist(&mut self) -> Command<Msg> {
        let (Some(Dialog::Remove(entry)), Some(folder)) = (self.dialog.take(), self.machine.playlists.clone()) else {
            return Command::none();
        };
        let told = match playlist::delete(&folder, &entry.path) {
            Ok(()) => Command::none(),
            Err(error) => Command::toast(
                Toast::danger(t!("music.playlists.unremoved", name = entry.name.as_str()))
                    .body(error.to_string())
                    .key("playlist"),
            ),
        };
        self.read_playlists();
        self.refresh_shelf();
        told
    }

    /// The playlists page: every playlist the search finds.
    pub(super) fn playlists_page(&self, ui: &mut View<'_, Msg>) {
        if self.shelf.all.is_empty() {
            ui.add(
                EmptyState::new(t!("music.playlists.none"))
                    .icon("music-playlist")
                    .message(t!("music.playlists.none-text")),
            )
            .fill();
            return;
        }
        if self.shelf.shown.0.is_empty() {
            ui.add(EmptyState::new(t!("music.search.none")).icon("search").message(t!("music.search.none-text")))
                .fill();
            return;
        }
        let columns = [
            Column::new(t!("music.column.playlist")).width(ColumnWidth::Fill(3)).min(12),
            Column::new(t!("music.column.tracks")).width(ColumnWidth::Fit).align(Align::End),
        ];
        ui.add(
            Table::new(columns, Arc::clone(&self.shelf.shown.1))
                .selected(self.shelf.cursor)
                .on_select(|at| Msg::Lists(ListMsg::Select(at)))
                .on_activate(|at| Msg::Lists(ListMsg::Open(at))),
        )
        .fill()
        .id(PLAYLISTS);
    }

    /// The dialog open over the screen, when one is.
    pub(super) fn playlist_dialog(&self, ui: &mut View<'_, Msg>) {
        match &self.dialog {
            None => {}
            Some(Dialog::Save { name, problem }) => {
                let modal = Modal::new()
                    .title(t!("music.playlists.save"))
                    .width(DIALOG)
                    .on_close(Msg::Lists(ListMsg::Close))
                    .action(Button::new(t!("music.playlists.cancel")).on_press(Msg::Lists(ListMsg::Close)))
                    .action(
                        Button::new(t!("music.playlists.keep")).variant("primary").on_press(Msg::Lists(ListMsg::Save)),
                    );
                ui.add_with(modal, |ui| {
                    ui.add_with(Field::new(t!("music.playlists.name")).required(true).error(problem.clone()), |ui| {
                        ui.add(
                            TextInput::new(name.clone())
                                .on_change(|typed| Msg::Lists(ListMsg::Typed(typed)))
                                .on_submit(|_| Msg::Lists(ListMsg::Save)),
                        )
                        .fill_width()
                        .id(NAME);
                    })
                    .fill_width();
                });
            }
            Some(Dialog::Remove(entry)) => {
                let modal = Modal::new()
                    .title(t!("music.playlists.remove-title", name = entry.name.as_str()))
                    .variant("danger")
                    .width(DIALOG)
                    .on_close(Msg::Lists(ListMsg::Close))
                    .action(Button::new(t!("music.playlists.cancel")).on_press(Msg::Lists(ListMsg::Close)))
                    .action(
                        Button::new(t!("music.playlists.remove-button"))
                            .variant("danger")
                            .on_press(Msg::Lists(ListMsg::Remove)),
                    );
                ui.add_with(modal, |ui| {
                    ui.add(Text::new(t!("music.playlists.remove-text"))).fill_width();
                });
            }
        }
    }
}
