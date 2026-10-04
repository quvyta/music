//! The pages beside the tracks: the queue, the albums and the artists, the side bar that leads to
//! them and the search that narrows them.

use std::sync::Arc;

use qframe::prelude::*;
use qframe::widgets::{
    Column, ColumnWidth, EmptyState, Menu, MenuGroup, MenuItem, Segmented, Table, TableCell, TableRow, TextInput,
};

use super::{Msg, Music, Page, clock, view};
use crate::library::{self, Location};
use crate::queue::Repeat;

/// The search box.
pub(super) const SEARCH: &str = "search";

/// The source picker.
pub(super) const SOURCES: &str = "sources";

/// The table of the albums.
pub(super) const ALBUMS: &str = "albums";

/// The table of the artists.
pub(super) const ARTISTS: &str = "artists";

/// The table of the queue.
pub(super) const QUEUE: &str = "queue";

/// The most rows of the queue the queue page shows: enough to see what comes, few enough to draw
/// on every frame of a queue of a whole library.
const QUEUE_SHOWN: usize = 200;

/// The pages of the side bar, in its order, with their keys and icons.
const PAGES: [(Page, &str, &str); 6] = [
    (Page::NowPlaying, "now-playing", "music-note"),
    (Page::Queue, "queue", "music-queue"),
    (Page::Tracks, "tracks", "file-audio"),
    (Page::Albums, "albums", "music-album"),
    (Page::Artists, "artists", "music-artist"),
    (Page::Playlists, "playlists", "music-playlist"),
];

/// What happens on the pages beside the tracks.
#[derive(Debug, Clone)]
pub enum PageMsg {
    /// `/`: the search box takes the keyboard.
    SearchFocus,
    /// The search box holds this now.
    Search(String),
    /// Enter in the search box: the list takes the keyboard.
    SearchDone,
    /// Esc in the search box: it empties.
    SearchClear,
    /// The side bar opens over the body, or closes.
    Sidebar(bool),
    /// The cursor moved to this row of the albums table.
    AlbumSelect(usize),
    /// This row of the albums table was chosen: its tracks open.
    AlbumOpen(usize),
    /// The cursor moved to this row of the artists table.
    ArtistSelect(usize),
    /// This row of the artists table was chosen: their tracks open.
    ArtistOpen(usize),
    /// The cursor moved to this row of the queue.
    QueueSelect(usize),
    /// This row of the queue was chosen: the queue goes on from it.
    QueuePlay(usize),
    /// Delete: the row under the queue's cursor leaves the queue.
    QueueRemove,
    /// The source picker's option at this place was chosen.
    Source(usize),
}

/// Whose music the tracks, albums and artists pages show; chosen for this run only.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) enum Shown {
    /// Every source's.
    #[default]
    All,
    /// This computer's.
    Local,
    /// The account's with this key.
    Account(String),
}

impl Shown {
    /// The source `track` is from.
    pub(super) fn of(track: &library::Track) -> Self {
        track.location.source().map_or(Self::Local, |source| Self::Account(source.0.clone()))
    }

    /// Whether `track` is among the music shown.
    pub(super) fn holds(&self, track: &library::Track) -> bool {
        match self {
            Self::All => true,
            Self::Local => track.location.source().is_none(),
            Self::Account(key) => track.location.source().is_some_and(|source| source.0 == *key),
        }
    }
}

impl Music {
    /// Makes the lists the pages show again: the tracks, albums and artists the search finds, or
    /// the tracks of the album or artist opened.
    pub(super) fn refresh_lists(&mut self) {
        let Some(tracks) = self.tracks.clone() else { return };
        let query = self.query.clone();
        let shown = self.shown.clone();
        let albums: Vec<usize> = (0..self.albums.len())
            .filter(|at| {
                let album = &self.albums[*at];
                shown.holds(&tracks[album.tracks[0]])
                    && library::found(&format!("{} {}", album.title, album.artist), &query)
            })
            .collect();
        let album_rows = albums
            .iter()
            .map(|at| {
                let album = &self.albums[*at];
                TableRow::new([
                    TableCell::new(album.title.clone()),
                    TableCell::new(album.artist.clone()),
                    TableCell::new(album.tracks.len().to_string()),
                    TableCell::new(clock(album.tracks.iter().filter_map(|row| tracks[*row].duration).sum())),
                ])
            })
            .collect();
        let artists: Vec<usize> = (0..self.artists.len())
            .filter(|at| {
                let artist = &self.artists[*at];
                artist.tracks.iter().any(|row| shown.holds(&tracks[*row])) && library::found(&artist.name, &query)
            })
            .collect();
        let artist_rows = artists
            .iter()
            .map(|at| {
                let artist = &self.artists[*at];
                TableRow::new([
                    TableCell::new(artist.name.clone()),
                    TableCell::new(artist.albums.to_string()),
                    TableCell::new(artist.tracks.len().to_string()),
                ])
            })
            .collect();
        // Each table's cursor stays within it, and stands on its first row from the start.
        let within = |cursor: Option<usize>, rows: usize| match cursor {
            _ if rows == 0 => None,
            Some(at) => Some(at.min(rows - 1)),
            None => Some(0),
        };
        self.group_cursor = (within(self.group_cursor.0, albums.len()), within(self.group_cursor.1, artists.len()));
        self.queue_cursor = within(self.queue_cursor, self.queue.upcoming().count().min(QUEUE_SHOWN));
        self.album_list = (albums, album_rows);
        self.artist_list = (artists, artist_rows);
        self.refresh_shelf();
        let mut list: Vec<usize> = match (self.page, self.album_open, self.artist_open) {
            (Page::Playlists, ..) => self.playlist_rows(),
            (Page::Albums, Some(at), _) => self.albums.get(at).map(|album| album.tracks.clone()).unwrap_or_default(),
            (Page::Artists, _, Some(at)) => {
                self.artists.get(at).map(|artist| artist.tracks.clone()).unwrap_or_default()
            }
            _ => (0..tracks.len()).filter(|row| library::matches(&tracks[*row], &query)).collect(),
        };
        if self.page != Page::Playlists {
            list.retain(|row| shown.holds(&tracks[*row]));
            // What the search finds is grouped by where it is from: this computer's first, then
            // each account's in the order they were added.
            if !query.is_empty() {
                list.sort_by_key(|row| self.source_place(&tracks[*row]));
            }
        }
        self.list_rows = if list.len() == tracks.len() {
            Arc::clone(&self.rows)
        } else {
            list.iter().map(|row| self.rows[*row].clone()).collect()
        };
        // The cursor stays on the track it was on when the list still holds it.
        let under = self.cursor.and_then(|at| self.list.get(at)).copied();
        self.cursor =
            under.and_then(|row| list.iter().position(|shown| *shown == row)).or((!list.is_empty()).then_some(0));
        self.list = list;
    }

    /// Where the source of `track` stands among the sources: this computer first, then the
    /// accounts in the order they were added.
    fn source_place(&self, track: &library::Track) -> usize {
        track.location.source().map_or(0, |source| {
            self.accounts.iter().position(|account| account.key == source.0).map_or(usize::MAX, |at| at + 1)
        })
    }

    /// The source picker's choices: every source, this computer, then each account, with what
    /// the search finds in each while it holds a word.
    fn source_options(&self) -> Vec<(Shown, String)> {
        let mut options = vec![(Shown::All, t!("music.source.all")), (Shown::Local, t!("music.source.local"))];
        options.extend(self.accounts.iter().map(|account| (Shown::Account(account.key.clone()), account.name.clone())));
        if self.query.is_empty() {
            return options;
        }
        let tracks = self.tracks();
        options
            .into_iter()
            .map(|(shown, label)| {
                let found =
                    tracks.iter().filter(|track| shown.holds(track) && library::matches(track, &self.query)).count();
                // The dot sets the name and the count apart in every language alike.
                (shown, format!("{label} · {found}"))
            })
            .collect()
    }

    /// The source picker over the tracks, albums and artists, while there is an account to
    /// choose between.
    pub(super) fn source_picker(&self, ui: &mut View<'_, Msg>) {
        if self.accounts.is_empty() {
            return;
        }
        let options = self.source_options();
        let selected = options.iter().position(|(shown, _)| *shown == self.shown).unwrap_or(0);
        ui.row(|ui| {
            ui.add(
                Segmented::new(options.into_iter().map(|(_, label)| label))
                    .selected(selected)
                    .on_select(|at| Msg::Pages(PageMsg::Source(at))),
            )
            .id(SOURCES);
        })
        .padding(Padding::symmetric(0, 1))
        .fill_width();
    }

    /// Shows `page`, closing the side bar over the body, and gives its list the keyboard.
    pub(super) fn show_page(&mut self, page: Page) -> Command<Msg> {
        if page == Page::Playlists && self.page != Page::Playlists {
            self.shelf.open = None;
        }
        let asked = if page == Page::Playlists && self.shelf.open.is_none() {
            self.read_playlists();
            self.ask_playlists()
        } else {
            Command::none()
        };
        self.page = page;
        self.sidebar_open = false;
        self.refresh_lists();
        Command::batch([asked, Command::focus(self.list_of_page())])
    }

    /// The name of the list that takes the keyboard on the page shown.
    fn list_of_page(&self) -> &'static str {
        match self.page {
            Page::Albums if self.album_open.is_none() => ALBUMS,
            Page::Artists if self.artist_open.is_none() => ARTISTS,
            Page::Playlists if self.shelf.open.is_none() => super::playlists::PLAYLISTS,
            Page::Queue => QUEUE,
            _ => view::TRACKS,
        }
    }

    /// Esc: closes the album or artist opened, empties the search, or goes back from the
    /// now-playing page and the queue to the tracks.
    pub(super) fn back(&mut self) -> Command<Msg> {
        match self.page {
            Page::Albums if self.album_open.is_some() => {
                self.album_open = None;
                self.show_page(Page::Albums)
            }
            Page::Artists if self.artist_open.is_some() => {
                self.artist_open = None;
                self.show_page(Page::Artists)
            }
            Page::Playlists if self.shelf.open.is_some() => {
                self.shelf.open = None;
                self.show_page(Page::Playlists)
            }
            Page::NowPlaying | Page::Queue => self.show_page(Page::Tracks),
            _ if !self.query.is_empty() => {
                self.query.clear();
                self.refresh_lists();
                Command::focus(self.list_of_page())
            }
            _ => Command::none(),
        }
    }

    /// Carries out `msg`.
    pub(super) fn page_msg(&mut self, msg: PageMsg) -> Command<Msg> {
        match msg {
            PageMsg::Source(at) => {
                let Some((shown, _)) = self.source_options().into_iter().nth(at) else { return Command::none() };
                self.shown = shown;
                self.refresh_lists();
                // The choice made, the list chosen takes the keyboard again.
                return Command::focus(self.list_of_page());
            }
            PageMsg::SearchFocus => {
                // The search narrows lists; the pages without one give way to the tracks.
                if matches!(self.page, Page::NowPlaying | Page::Queue) {
                    self.page = Page::Tracks;
                    self.refresh_lists();
                }
                return Command::focus(SEARCH);
            }
            PageMsg::Search(query) => {
                self.query = query;
                self.album_open = None;
                self.artist_open = None;
                self.refresh_lists();
            }
            PageMsg::SearchDone => return Command::focus(self.list_of_page()),
            PageMsg::SearchClear => {
                self.query.clear();
                self.refresh_lists();
                return Command::focus(self.list_of_page());
            }
            PageMsg::Sidebar(open) => self.sidebar_open = open,
            PageMsg::AlbumSelect(at) => self.group_cursor.0 = Some(at),
            PageMsg::AlbumOpen(at) => {
                self.group_cursor.0 = Some(at);
                self.album_open = self.album_list.0.get(at).copied();
                self.cursor = None;
                return self.show_page(Page::Albums);
            }
            PageMsg::ArtistSelect(at) => self.group_cursor.1 = Some(at),
            PageMsg::ArtistOpen(at) => {
                self.group_cursor.1 = Some(at);
                self.artist_open = self.artist_list.0.get(at).copied();
                self.cursor = None;
                return self.show_page(Page::Artists);
            }
            PageMsg::QueueSelect(at) => self.queue_cursor = Some(at),
            PageMsg::QueuePlay(at) => return self.queue_play(at),
            PageMsg::QueueRemove => {
                let Some(at) = self.queue_cursor else { return Command::none() };
                if self.queue.remove(at) {
                    self.follow();
                    self.refresh_lists();
                }
            }
        }
        Command::none()
    }

    /// Goes on from row `at` of the queue, the track heard being the first row.
    fn queue_play(&mut self, at: usize) -> Command<Msg> {
        self.queue_cursor = Some(at);
        let repeat = self.queue.repeat();
        // Walking forward through the rows shown, never round to the start or on the spot.
        self.queue.set_repeat(Repeat::Off);
        for _ in 0..at {
            self.queue.advance();
        }
        self.queue.set_repeat(repeat);
        let Some(index) = self.queue.current().cloned().and_then(|path| self.playable_row(&path)) else {
            return Command::none();
        };
        self.queue_cursor = Some(0);
        self.play_row(index)
    }

    /// The side bar: the pages, the one shown marked.
    pub(super) fn sidebar(&self, ui: &mut View<'_, Msg>) {
        let items =
            PAGES.iter().map(|(_, key, icon)| MenuItem::new(*key, t!(&format!("music.page.{key}"))).icon(*icon, None));
        let selected = PAGES.iter().find(|(page, _, _)| *page == self.page).map(|(_, key, _)| *key);
        ui.add(Menu::new([MenuGroup::new("pages", items)]).selected(selected).on_select(|key| {
            let page = PAGES.iter().find(|(_, shown, _)| *shown == key).map_or(Page::Tracks, |(page, _, _)| *page);
            Msg::Page(page)
        }))
        .fill()
        .id("pages");
    }

    /// The search box of the top strip.
    pub(super) fn search_box(&self, ui: &mut View<'_, Msg>) {
        ui.add(
            TextInput::new(self.query.clone())
                .placeholder(t!("music.search.placeholder"))
                .on_change(|query| Msg::Pages(PageMsg::Search(query)))
                .on_submit(|_| Msg::Pages(PageMsg::SearchDone))
                .on_cancel(Msg::Pages(PageMsg::SearchClear)),
        )
        .width(Length::Cells(28))
        .id(SEARCH);
    }

    /// The queue page: the track heard, then what plays after it.
    pub(super) fn queue_page(&self, ui: &mut View<'_, Msg>) {
        let upcoming: Vec<&Location> = self.queue.upcoming().take(QUEUE_SHOWN).collect();
        if upcoming.is_empty() {
            ui.add(EmptyState::new(t!("music.queue.empty")).icon("music-queue").message(t!("music.player.idle")))
                .fill();
            return;
        }
        let rows: Vec<TableRow> = upcoming
            .iter()
            .enumerate()
            .map(|(at, path)| {
                let track = self.places.get(*path).and_then(|row| self.tracks().get(*row));
                let (title, artist, time) = track.map_or_else(
                    || {
                        (
                            path.file().map(|file| file.display().to_string()).unwrap_or_default(),
                            String::new(),
                            String::new(),
                        )
                    },
                    |track| (track.title.clone(), track.artist.clone(), track.duration.map(clock).unwrap_or_default()),
                );
                // The track heard is marked with the note the player bar's name stands beside.
                let mark = if at == 0 { "♪".to_owned() } else { at.to_string() };
                TableRow::new([
                    TableCell::new(mark),
                    TableCell::new(title),
                    TableCell::new(artist),
                    TableCell::new(time),
                ])
                .faint(track.is_none_or(|track| !track.playable))
            })
            .collect();
        let columns = [
            Column::new("").width(ColumnWidth::Fit).align(Align::End),
            Column::new(t!("music.column.title")).width(ColumnWidth::Fill(3)).min(12),
            Column::new(t!("music.column.artist")).width(ColumnWidth::Fill(2)).min(10),
            Column::new(t!("music.column.time")).width(ColumnWidth::Fit).align(Align::End),
        ];
        ui.add(
            Table::new(columns, Arc::from(rows))
                .selected(self.queue_cursor)
                .on_select(|at| Msg::Pages(PageMsg::QueueSelect(at)))
                .on_activate(|at| Msg::Pages(PageMsg::QueuePlay(at)))
                .space_activates(false),
        )
        .fill()
        .id(QUEUE);
    }

    /// The albums page: every album the search finds.
    pub(super) fn albums_page(&self, ui: &mut View<'_, Msg>) {
        if self.album_list.0.is_empty() {
            ui.add(EmptyState::new(t!("music.search.none")).icon("search").message(t!("music.search.none-text")))
                .fill();
            return;
        }
        let columns = [
            Column::new(t!("music.column.album")).width(ColumnWidth::Fill(3)).min(12),
            Column::new(t!("music.column.artist")).width(ColumnWidth::Fill(2)).min(10),
            Column::new(t!("music.column.tracks")).width(ColumnWidth::Fit).align(Align::End),
            Column::new(t!("music.column.time")).width(ColumnWidth::Fit).align(Align::End),
        ];
        ui.add(
            Table::new(columns, Arc::clone(&self.album_list.1))
                .selected(self.group_cursor.0)
                .on_select(|at| Msg::Pages(PageMsg::AlbumSelect(at)))
                .on_activate(|at| Msg::Pages(PageMsg::AlbumOpen(at)))
                .space_activates(false),
        )
        .fill()
        .id(ALBUMS);
    }

    /// The artists page: every artist the search finds.
    pub(super) fn artists_page(&self, ui: &mut View<'_, Msg>) {
        if self.artist_list.0.is_empty() {
            ui.add(EmptyState::new(t!("music.search.none")).icon("search").message(t!("music.search.none-text")))
                .fill();
            return;
        }
        let columns = [
            Column::new(t!("music.column.artist")).width(ColumnWidth::Fill(3)).min(12),
            Column::new(t!("music.column.albums")).width(ColumnWidth::Fit).align(Align::End),
            Column::new(t!("music.column.tracks")).width(ColumnWidth::Fit).align(Align::End),
        ];
        ui.add(
            Table::new(columns, Arc::clone(&self.artist_list.1))
                .selected(self.group_cursor.1)
                .on_select(|at| Msg::Pages(PageMsg::ArtistSelect(at)))
                .on_activate(|at| Msg::Pages(PageMsg::ArtistOpen(at)))
                .space_activates(false),
        )
        .fill()
        .id(ARTISTS);
    }
}
