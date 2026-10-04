//! Drawing the screen: the top strip, the tracks and the player bar.

use std::sync::Arc;

use qframe::prelude::*;
use qframe::widgets::{
    BigText, Column, ColumnWidth, EmptyState, HelpLayer, IconButton, Image, LevelBars, Panel, ProgressBar, Table,
    TableCell, TableRow,
};

use super::{Msg, Music, Page, clock};
use crate::audio::{Problem, State};
use crate::library::Track;
use crate::queue::Repeat;

/// The name of the table of tracks, which takes the keyboard whenever the folder has been read.
pub(super) const TRACKS: &str = "tracks";

/// The side bar's width.
const SIDEBAR: u16 = 20;

/// Below this width the side bar folds away; `ctrl+b` opens it over the body.
const SIDEBAR_BELOW: u16 = 100;

/// Below this size nothing useful fits.
const SMALLEST: Size = Size { width: 24, height: 6 };

/// The width of the progress bar in the player bar.
const PROGRESS: u16 = 24;

/// The cells of the visualizer in the player bar; the bands are merged into them.
const VISUALIZER: u16 = 8;

/// The characters the large title is drawn with; a title with any other is written in bold.
const BIG: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 :.%-";

/// Below this width the top strip gives way and the player bar takes two rows, so the track's
/// name and both times still fit.
const WIDE: u16 = 60;

/// From this width the player bar takes one row; below it how far the track has got takes a row
/// of its own, so the track's name keeps its room.
const ONE_ROW: u16 = 80;

/// From this width the player bar also has shuffle, repeat and the volume; below it their keys
/// still work.
const FULL: u16 = 100;

/// The rows of `tracks`, made once when the folder is read rather than on every frame.
pub(super) fn rows(tracks: &[Track]) -> Arc<[TableRow]> {
    tracks
        .iter()
        .map(|track| {
            TableRow::new([
                TableCell::new(track.number.map(|number| number.to_string()).unwrap_or_default()),
                TableCell::new(track.title.clone()),
                TableCell::new(track.artist.clone()),
                TableCell::new(track.album.clone()),
                TableCell::new(track.duration.map(clock).unwrap_or_default()),
            ])
            .faint(!track.playable)
        })
        .collect()
}

impl Music {
    /// The whole screen at the size it is drawn in.
    pub(super) fn screen(&self, ui: &mut View<'_, Msg>) {
        let calm = ui.env().reduced_motion() && !self.choices.live;
        self.set_beat(!calm && self.choices.style != super::settings::Style::Off);
        let size = ui.size();
        if size.width < SMALLEST.width || size.height < SMALLEST.height {
            ui.add(EmptyState::new(t!("music.too-small"))).fill();
            return;
        }
        let wide = size.width >= WIDE;
        let collapsed = size.width < SIDEBAR_BELOW;
        let shell = AppShell::new()
            .sidebar_width(SIDEBAR)
            .collapse_below(SIDEBAR_BELOW)
            .sidebar_open(collapsed && self.sidebar_open)
            .sidebar(|ui| self.sidebar(ui));
        let shell = if wide { shell.header(|ui| self.top_strip(ui)) } else { shell };
        shell
            .body(|ui| {
                if self.settings_open {
                    self.settings_page(ui);
                } else {
                    match self.page {
                        Page::NowPlaying => self.now_playing(ui),
                        Page::Queue => self.queue_page(ui),
                        Page::Albums if self.album_open.is_none() => self.albums_page(ui),
                        Page::Artists if self.artist_open.is_none() => self.artists_page(ui),
                        Page::Playlists if self.shelf.open.is_none() => self.playlists_page(ui),
                        Page::Tracks | Page::Albums | Page::Artists | Page::Playlists => self.body(ui),
                    }
                }
            })
            .footer(|ui| self.player_bar(ui, size.width))
            .show(ui);
        self.playlist_dialog(ui);
        self.folder_picker(ui);
        if self.help_open {
            ui.add(self.help());
        }
    }

    /// The top strip: the folder shown, how many tracks it holds, and the settings.
    fn top_strip(&self, ui: &mut View<'_, Msg>) {
        ui.row(|ui| {
            ui.add(Text::new("qmus").role("title"));
            self.search_box(ui);
            ui.add(Text::new(self.folder_shown()).role("secondary").no_wrap()).fill_width();
            if let Some(tracks) = &self.tracks {
                let count = u32::try_from(tracks.len()).unwrap_or(u32::MAX);
                ui.add(Text::new(t!("music.count", n = count)).role("secondary"));
            }
            ui.add(
                IconButton::new("settings")
                    .tooltip(t!("music.settings.title"))
                    .on_press(Msg::Settings(!self.settings_open)),
            )
            .id("settings-button");
        })
        .gap(2)
        .padding(Padding::symmetric(0, 1))
        .fill_width();
    }

    /// The tracks shown, or what stands in their place while there are none: all of them, those
    /// the search finds, or those of the album or artist opened, under its name.
    fn body(&self, ui: &mut View<'_, Msg>) {
        let Some(tracks) = &self.tracks else {
            // The first reading of a folder is quick; nothing is shown until it is back.
            return;
        };
        if tracks.is_empty() {
            let folder = self.folder_shown();
            ui.add(
                EmptyState::new(t!("music.empty.title"))
                    .icon("music-note")
                    .message(t!("music.empty.text", folder = folder.as_str())),
            )
            .fill();
            return;
        }
        let opened = match self.page {
            Page::Albums => self.album_open.and_then(|at| self.albums.get(at)).map(|album| {
                let by = if album.artist.is_empty() { String::new() } else { format!(" · {}", album.artist) };
                format!("{}{by}", album.title)
            }),
            Page::Artists => self.artist_open.and_then(|at| self.artists.get(at)).map(|artist| artist.name.clone()),
            Page::Playlists => self.playlist_title(),
            _ => None,
        };
        if self.list.is_empty() {
            ui.add(EmptyState::new(t!("music.search.none")).icon("search").message(t!("music.search.none-text")))
                .fill();
            return;
        }
        ui.column(|ui| {
            if let Some(name) = opened {
                ui.row(|ui| {
                    ui.add(IconButton::new("arrow-left").tooltip(t!("music.back")).on_press(Msg::Cancel)).id("back");
                    ui.add(Text::new(name).role("title").no_wrap()).fill_width();
                })
                .gap(1)
                .padding(Padding::symmetric(0, 1))
                .fill_width();
            }
            self.track_table(ui);
        })
        .fill();
    }

    /// The table of the tracks in `list`.
    fn track_table(&self, ui: &mut View<'_, Msg>) {
        let columns = [
            Column::new(t!("music.column.number")).width(ColumnWidth::Fit).align(Align::End),
            Column::new(t!("music.column.title")).width(ColumnWidth::Fill(3)).min(12),
            Column::new(t!("music.column.artist")).width(ColumnWidth::Fill(2)).min(10),
            Column::new(t!("music.column.album")).width(ColumnWidth::Fill(2)).min(10),
            Column::new(t!("music.column.time")).width(ColumnWidth::Fit).align(Align::End),
        ];
        ui.add(
            Table::new(columns, Arc::clone(&self.list_rows))
                .selected(self.cursor)
                .on_select(Msg::Select)
                .on_activate(Msg::Play)
                .context_menu(self.track_menu()),
        )
        .fill()
        .id(TRACKS);
    }

    /// The now-playing page: the album's cover on the left when it has one, then the track's name
    /// large, who and what it is from, the visualizer across the page and how far the track has
    /// got.
    fn now_playing(&self, ui: &mut View<'_, Msg>) {
        if self.current().is_none() {
            ui.add(EmptyState::new(t!("music.page.nothing")).icon("music-note").message(t!("music.player.idle")))
                .fill();
            return;
        }
        let art = self.art.as_ref();
        let size = ui.size();
        // A cell is about twice as tall as it is wide, so a square cover is twice its rows across;
        // it never takes more than two fifths of the page.
        let rows = size.height.saturating_sub(6);
        let across = (rows * 2).min(size.width * 2 / 5);
        let shown = art.filter(|art| art.read && across >= 8);
        match shown {
            Some(art) => {
                ui.row(|ui| {
                    match &art.image {
                        Some(cover) => {
                            ui.add(Image::new(cover)).width(Length::Cells(across)).fill_height().id("cover");
                        }
                        None => self.cover_card(ui, across, &art.key.1),
                    }
                    self.now_playing_text(ui);
                })
                .gap(2)
                .padding(Padding::symmetric(1, 2))
                .fill();
            }
            None => {
                ui.row(|ui| self.now_playing_text(ui)).padding(Padding::symmetric(1, 2)).fill();
            }
        }
    }

    /// What stands in place of a cover for an album that has none: a raised square with the
    /// album's first letter large in the accent and its name under it, so the page keeps its shape.
    fn cover_card(&self, ui: &mut View<'_, Msg>, across: u16, album: &str) {
        let initial = album.chars().find(|letter| !letter.is_whitespace()).and_then(|letter| {
            let upper: String = letter.to_uppercase().collect();
            upper.chars().all(|letter| BIG.contains(letter)).then_some(upper)
        });
        ui.add_with(Panel::new(), |ui| {
            ui.column(|ui| {
                match initial {
                    Some(letter) => {
                        ui.add(BigText::new(letter).variant("accent"));
                    }
                    None => {
                        let note = ui.env().icons().glyph("music-note").into_owned();
                        ui.add(Text::new(note).color("accent"));
                    }
                }
                if !album.is_empty() {
                    ui.add(Text::new(album.to_owned()).role("secondary").no_wrap());
                }
            })
            .gap(1)
            .justify(Align::Center)
            .align(Align::Center)
            .fill();
        })
        .width(Length::Cells(across))
        .fill_height()
        .id("cover");
    }

    /// The now-playing page beside the cover: the name, who and what it is from, the visualizer and
    /// how far the track has got.
    fn now_playing_text(&self, ui: &mut View<'_, Msg>) {
        let Some(track) = self.current() else { return };
        ui.column(|ui| {
            if track.title.chars().all(|letter| BIG.contains(letter)) {
                ui.add(BigText::new(track.title.clone()).variant("accent")).id("title");
            } else {
                ui.add(Text::new(track.title.clone()).role("title").bold().no_wrap()).id("title");
            }
            let about: Vec<&str> =
                [track.artist.as_str(), track.album.as_str()].into_iter().filter(|part| !part.is_empty()).collect();
            ui.add(Text::new(about.join(" · ")).role("secondary").no_wrap());
            self.visualizer_in(ui, true);
            ui.row(|ui| self.progress(ui, Length::Fill(1))).gap(1).fill_width();
        })
        .gap(1)
        .fill();
    }

    /// The player bar, `width` wide: the visualizer, the steps through the queue, the track heard
    /// and how far it has got, then shuffle, repeat and the volume where there is room; on a
    /// narrow terminal how far the track has got takes a row of its own.
    fn player_bar(&self, ui: &mut View<'_, Msg>, width: u16) {
        if width >= ONE_ROW {
            ui.row(|ui| {
                self.visualizer(ui);
                self.controls(ui);
                self.progress(ui, Length::Cells(PROGRESS));
                if width >= FULL {
                    self.order(ui);
                    self.volume(ui);
                }
            })
            .gap(1)
            .padding(Padding::symmetric(0, 1))
            .fill_width();
        } else {
            ui.column(|ui| {
                ui.row(|ui| {
                    if width >= WIDE {
                        self.visualizer(ui);
                    }
                    self.controls(ui);
                })
                .gap(1)
                .fill_width();
                if self.current().is_some() {
                    ui.row(|ui| self.progress(ui, Length::Fill(1))).gap(1).fill_width();
                }
            })
            .padding(Padding::symmetric(0, 1))
            .fill_width();
        }
    }

    /// The sound heard as columns, one per band, rising and falling with it; with motion reduced,
    /// a calm reading four times a second with no caps. Silence leaves the lowest line, so the
    /// place stays visible.
    fn visualizer(&self, ui: &mut View<'_, Msg>) {
        self.visualizer_in(ui, false);
    }

    /// The visualizer, across the page when `large`, otherwise the player bar's few cells.
    fn visualizer_in(&self, ui: &mut View<'_, Msg>, large: bool) {
        if self.choices.style == super::settings::Style::Off {
            return;
        }
        // The thinnest a column can be drawn is an eighth of a cell; the large one rests lower.
        let lowest = if large { 1.0 / 128.0 } else { 1.0 / 8.0 };
        let floor = |levels: &[f32]| levels.iter().map(|level| level.max(lowest)).collect::<Vec<_>>();
        let bars = if ui.env().reduced_motion() && !self.choices.live {
            LevelBars::new(floor(&self.calm))
        } else {
            LevelBars::new(floor(self.spectrum.levels())).peaks(self.spectrum.peaks().to_vec())
        };
        if large {
            let mirror = self.choices.style == super::settings::Style::Mirror;
            ui.add(bars.gap(1).gradient(true).mirror(mirror)).fill().id("visualizer-large");
        } else {
            ui.add(bars.gap(0).gradient(true)).width(Length::Cells(VISUALIZER)).id("visualizer");
        }
    }

    /// The buttons, and the track heard or what stands in its place.
    fn controls(&self, ui: &mut View<'_, Msg>) {
        let loaded = self.current();
        let playing = self.status.state == State::Playing;
        ui.add(
            IconButton::new("media-previous")
                .tooltip(t!("music.player.previous"))
                .disabled(loaded.is_none())
                .on_press(Msg::Previous),
        )
        .id("previous");
        let (icon, label) =
            if playing { ("media-pause", t!("music.player.pause")) } else { ("media-play", t!("music.player.play")) };
        let nothing = self.tracks.as_ref().is_none_or(|tracks| tracks.is_empty());
        ui.add(IconButton::new(icon).tooltip(label).disabled(nothing).on_press(Msg::PlayPause)).id("play-pause");
        ui.add(
            IconButton::new("media-next")
                .tooltip(t!("music.player.next"))
                .disabled(loaded.is_none())
                .on_press(Msg::Next),
        )
        .id("next");
        if matches!(self.status.problem, Some(Problem::Output(_))) {
            ui.add(Text::new(t!("music.output.lost")).role("danger").no_wrap()).id("no-output");
        }
        match loaded {
            Some(track) => {
                let heard = if track.artist.is_empty() {
                    track.title.clone()
                } else {
                    // The dot sets the two names apart in every language alike.
                    format!("{} · {}", track.title, track.artist)
                };
                ui.add(Text::new(heard).no_wrap()).fill_width().id("now-playing");
            }
            None => {
                ui.add(Text::new(t!("music.player.idle")).role("secondary").no_wrap()).fill_width().id("now-playing");
            }
        }
    }

    /// How far the track heard has got: the time, the bar `width` wide, and the whole length.
    fn progress(&self, ui: &mut View<'_, Msg>, width: Length) {
        let Some(track) = self.current() else { return };
        let total = track.duration.unwrap_or_default();
        let position = self.status.position.min(total);
        ui.add(Text::new(clock(position)).role("secondary"));
        let done = if total.is_zero() { 0.0 } else { position.as_secs_f32() / total.as_secs_f32() };
        ui.add(ProgressBar::new(done).percent(false)).width(width);
        ui.add(Text::new(clock(total)).role("secondary"));
    }

    /// Shuffle and repeat, lit while on.
    fn order(&self, ui: &mut View<'_, Msg>) {
        let shuffled = self.queue.is_shuffled();
        ui.add(
            IconButton::new("media-shuffle")
                .tooltip(t!("music.player.shuffle"))
                .selected(shuffled)
                .on_press(Msg::Shuffle),
        )
        .id("shuffle");
        let (icon, label) = match self.queue.repeat() {
            Repeat::Off => ("media-repeat", t!("music.player.repeat-off")),
            Repeat::All => ("media-repeat", t!("music.player.repeat-all")),
            Repeat::One => ("media-repeat-once", t!("music.player.repeat-one")),
        };
        ui.add(IconButton::new(icon).tooltip(label).selected(self.queue.repeat() != Repeat::Off).on_press(Msg::Repeat))
            .id("repeat");
    }

    /// The volume: a button that silences the sound and brings it back, and the level.
    fn volume(&self, ui: &mut View<'_, Msg>) {
        let (icon, label) = if self.muted {
            ("media-muted", t!("music.player.unmute"))
        } else {
            ("media-volume", t!("music.player.mute"))
        };
        ui.add(IconButton::new(icon).tooltip(label).on_press(Msg::Mute)).id("mute");
        let level = if self.muted { 0 } else { self.volume };
        ui.add(Text::new(level.to_string()).role("secondary")).width(Length::Cells(3)).id("volume");
    }

    /// The key overview. qmus's own keys come from the keymap under the application's group, so a
    /// key the person binds anew is the key shown; the table writes its own.
    fn help(&self) -> HelpLayer<Msg> {
        HelpLayer::new(Msg::Help(false))
    }
}
