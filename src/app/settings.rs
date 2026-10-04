//! qmus's settings page: the folders the library is read from, how playing starts, how the
//! visualizer is drawn, the framework's shared appearance rows and the Quvyta-wide update notice.
//!
//! A value is written only while it differs from the default: a file of defaults would pin them,
//! and a later change of default would never reach the person.

use std::path::{Path, PathBuf};

use qframe::prelude::*;
use qframe::storage::{Setting, Settings};
use qframe::widgets::{
    Button, FileBrowser, FilePicker, FilePickerMsg, Modal, PickMode, ScrollView, Segmented, SettingRow, SettingsList,
    Switch, Text as Words, Toast,
};

use super::{Msg, Music};

/// The key of whether the queue of the last run comes back at start.
pub(super) const RESUME: &str = "resume-queue";

/// The key of how the visualizer is drawn: `bars`, `mirror` or `off`.
pub(super) const VISUALIZER: &str = "visualizer";

/// The key of whether the visualizer moves even when motion is reduced.
pub(super) const LIVE: &str = "visualizer-live";

/// The key of the folders the person added to the library, beside the one qmus opens with.
pub(super) const FOLDERS: &str = "library-folders";

/// The folders the person added to the library, in the order added; a value that is not a list
/// of paths adds none.
pub(super) fn added_folders(settings: &Settings) -> Vec<PathBuf> {
    settings.get::<Vec<String>>(FOLDERS).unwrap_or_default().into_iter().map(PathBuf::from).collect()
}

/// How the visualizer is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Columns rising from the bottom.
    Bars,
    /// Columns growing up and down from the middle.
    Mirror,
    /// Not drawn, and the sound is not measured for it.
    Off,
}

/// The styles in the order the picker shows them, with the names the file keeps them under.
const STYLES: [(Style, &str); 3] = [(Style::Bars, "bars"), (Style::Mirror, "mirror"), (Style::Off, "off")];

/// What qmus's own settings say, read once at start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choices {
    /// Whether the queue of the last run comes back at start.
    pub resume: bool,
    /// How the visualizer is drawn.
    pub style: Style,
    /// Whether the visualizer moves even when motion is reduced.
    pub live: bool,
}

impl Choices {
    /// The choices `settings` keeps, each its default where the file says nothing it understands.
    pub(super) fn of(settings: &Settings) -> Self {
        let style = settings
            .get::<String>(VISUALIZER)
            .and_then(|name| STYLES.iter().find(|(_, named)| *named == name).map(|(style, _)| *style))
            .unwrap_or(Style::Bars);
        Self { resume: settings.get_or(RESUME, true), style, live: settings.get_or(LIVE, false) }
    }
}

/// A change on the settings page.
#[derive(Debug, Clone)]
pub enum SettingMsg {
    /// The queue of the last run comes back at start, or not.
    Resume(bool),
    /// The visualizer is drawn in the style at this place of the picker.
    Style(usize),
    /// The visualizer moves even when motion is reduced, or follows it.
    Live(bool),
    /// The folder picker opens, to add a folder to the library.
    AddFolder,
    /// Something in the folder picker; a folder chosen is added.
    Picker(FilePickerMsg),
    /// The folder picker closes with nothing added.
    PickerClose,
    /// The added folder at this place leaves the library; its music stays where it is.
    RemoveFolder(usize),
    /// The folders are read again, for music added or changed since.
    Rescan,
    /// The settings file was written, or why it was not.
    Saved(Result<(), String>),
}

/// The widest the rows grow: beyond it a label and its control drift too far apart.
const SECTION: u16 = 76;

/// The folder picker's size: wide enough for a long path, tall enough for a screenful of folders.
const PICKER_WIDTH: u16 = 72;
const PICKER_ROWS: u16 = 16;

impl Music {
    /// Keeps `value` under `key`, or nothing while it is `default`, and writes the file off the
    /// drawing thread.
    fn store<T: Setting + PartialEq>(&mut self, key: &str, value: T, default: &T) -> Command<Msg> {
        let changed = if value == *default { self.settings.remove(key) } else { self.settings.set(key, value) };
        if !changed {
            return Command::none();
        }
        self.settings.save_command(|saved| Msg::Setting(SettingMsg::Saved(saved)))
    }

    /// Carries out `msg`.
    pub(super) fn setting_msg(&mut self, msg: SettingMsg) -> Command<Msg> {
        match msg {
            SettingMsg::Resume(on) => {
                self.choices.resume = on;
                self.store(RESUME, on, &true)
            }
            SettingMsg::Style(at) => {
                let Some((style, name)) = STYLES.get(at) else { return Command::none() };
                self.choices.style = *style;
                self.store(VISUALIZER, (*name).to_owned(), &"bars".to_owned())
            }
            SettingMsg::Live(on) => {
                self.choices.live = on;
                self.store(LIVE, on, &false)
            }
            SettingMsg::AddFolder => {
                // From above the folder qmus opens with, where other music folders usually sit.
                let start = self.folder.parent().map_or_else(|| self.folder.clone(), Path::to_path_buf);
                let mut browser = FileBrowser::new(start.clone(), PickMode::Folders);
                let open = browser.open(start, |msg| Msg::Setting(SettingMsg::Picker(msg)));
                self.picker = Some(browser);
                open
            }
            SettingMsg::Picker(FilePickerMsg::Chosen(folder)) => {
                self.picker = None;
                self.add_folder(folder)
            }
            SettingMsg::Picker(msg) => {
                let Some(browser) = &mut self.picker else { return Command::none() };
                browser.update(msg, |msg| Msg::Setting(SettingMsg::Picker(msg)))
            }
            SettingMsg::PickerClose => {
                self.picker = None;
                Command::none()
            }
            SettingMsg::RemoveFolder(at) => {
                if at >= self.added.len() {
                    return Command::none();
                }
                self.added.remove(at);
                let saved = self.store_folders();
                Command::batch([saved, self.scan()])
            }
            SettingMsg::Rescan => self.scan(),
            SettingMsg::Saved(Ok(())) => Command::none(),
            SettingMsg::Saved(Err(reason)) => {
                Command::toast(Toast::warning(t!("music.settings.not-saved")).body(reason).key("saved"))
            }
        }
    }

    /// Adds `folder` to the library and reads it, unless its music is already read from a folder
    /// of the library.
    fn add_folder(&mut self, folder: PathBuf) -> Command<Msg> {
        let inside = std::iter::once(&self.folder).chain(&self.added).find(|source| folder.starts_with(source));
        if let Some(source) = inside {
            let shown = qframe::storage::display_home_with(source, self.machine.home.as_deref());
            return Command::toast(Toast::info(t!("music.library.already", folder = shown.as_str())).key("library"));
        }
        // A folder that holds added ones takes their place, so no track is listed twice.
        self.added.retain(|source| !source.starts_with(&folder));
        self.added.push(folder);
        let saved = self.store_folders();
        Command::batch([saved, self.scan()])
    }

    /// Keeps the added folders, or nothing once there are none.
    fn store_folders(&mut self) -> Command<Msg> {
        let names: Vec<String> = self.added.iter().map(|folder| folder.to_string_lossy().into_owned()).collect();
        self.store(FOLDERS, names, &Vec::new())
    }

    /// The library's rows: the folder qmus opens with, each added folder with its way out, a way
    /// to add one and a way to read them all again.
    fn library_rows(&self, list: &mut qframe::widgets::SettingsRows<'_, Msg>) {
        let home = self.machine.home.as_deref();
        list.heading(t!("music.library.title"));
        let main = qframe::storage::display_home_with(&self.folder, home);
        list.row(SettingRow::new(main).description(t!("music.library.main-text")), |_| {});
        for (at, folder) in self.added.iter().enumerate() {
            let shown = qframe::storage::display_home_with(folder, home);
            list.row(SettingRow::new(shown), |ui| {
                ui.add(Button::new(t!("music.library.remove")).on_press(Msg::Setting(SettingMsg::RemoveFolder(at))))
                    .id(format!("remove-folder-{at}"));
            });
        }
        list.row(SettingRow::new(t!("music.library.add")).description(t!("music.library.add-text")), |ui| {
            ui.add(
                Button::new(t!("music.library.add-button"))
                    .icon("folder")
                    .on_press(Msg::Setting(SettingMsg::AddFolder)),
            )
            .id("add-folder");
        });
        list.row(SettingRow::new(t!("music.library.rescan")).description(t!("music.library.rescan-text")), |ui| {
            ui.add(
                Button::new(t!("music.library.rescan-button"))
                    .loading(self.reading)
                    .disabled(self.reading)
                    .on_press(Msg::Setting(SettingMsg::Rescan)),
            )
            .id("rescan");
        });
    }

    /// The folder picker, while it is open.
    pub(super) fn folder_picker(&self, ui: &mut View<'_, Msg>) {
        let Some(browser) = &self.picker else { return };
        let modal = Modal::new()
            .title(t!("music.library.pick-title"))
            .width(PICKER_WIDTH)
            .on_close(Msg::Setting(SettingMsg::PickerClose));
        ui.add_with(modal, |ui| {
            FilePicker::new(browser, |msg| Msg::Setting(SettingMsg::Picker(msg)))
                .show(ui)
                .height(Length::Cells(PICKER_ROWS))
                .fill_width();
        });
    }

    /// The settings page, in the place of the tracks.
    pub(super) fn settings_page(&self, ui: &mut View<'_, Msg>) {
        let page = |ui: &mut View<'_, Msg>| {
            ui.column(|ui| {
                ui.add(Words::new(t!("music.settings.title")).role("title"));
                SettingsList::show(ui, |list| {
                    self.library_rows(list);
                    list.heading(t!("music.settings.playing"));
                    list.row(
                        SettingRow::new(t!("music.settings.resume")).description(t!("music.settings.resume-text")),
                        |ui| {
                            ui.add(
                                Switch::new(self.choices.resume).on_toggle(|on| Msg::Setting(SettingMsg::Resume(on))),
                            )
                            .id("resume");
                        },
                    );
                    list.heading(t!("music.settings.visualizer"));
                    let styles: Vec<String> =
                        STYLES.iter().map(|(_, name)| t!(&format!("music.settings.style-{name}"))).collect();
                    let at = STYLES.iter().position(|(style, _)| *style == self.choices.style).unwrap_or_default();
                    list.row(SettingRow::new(t!("music.settings.style")), |ui| {
                        ui.add(Segmented::new(styles).selected(at).on_select(|at| Msg::Setting(SettingMsg::Style(at))))
                            .id("style");
                    });
                    list.row(
                        SettingRow::new(t!("music.settings.live")).description(t!("music.settings.live-text")),
                        |ui| {
                            ui.add(Switch::new(self.choices.live).on_toggle(|on| Msg::Setting(SettingMsg::Live(on))))
                                .id("live");
                        },
                    );
                    self.appearance.section(list, Msg::Appearance);
                    // The switch is Quvyta-wide; without the folders that keep it nothing is asked,
                    // and a switch there would change nothing.
                    if self.machine.updates.is_some() {
                        self.appearance.updates(list, Msg::Appearance);
                    }
                })
                .width(Length::Cells(SECTION))
                .id("settings-rows");
            })
            .padding(Padding { top: 1, right: 2, bottom: 1, left: 2 })
            .fill_width();
        };
        ui.add_with(ScrollView::new(), page).fill().id("settings");
    }
}
