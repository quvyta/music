//! qmus's settings page: how playing starts, how the visualizer is drawn, the framework's shared
//! appearance rows and the Quvyta-wide update notice.
//!
//! A value is written only while it differs from the default: a file of defaults would pin them,
//! and a later change of default would never reach the person.

use qframe::prelude::*;
use qframe::storage::{Setting, Settings};
use qframe::widgets::{ScrollView, Segmented, SettingRow, SettingsList, Switch, Text as Words, Toast};

use super::{Msg, Music};

/// The key of whether the queue of the last run comes back at start.
pub(super) const RESUME: &str = "resume-queue";

/// The key of how the visualizer is drawn: `bars`, `mirror` or `off`.
pub(super) const VISUALIZER: &str = "visualizer";

/// The key of whether the visualizer moves even when motion is reduced.
pub(super) const LIVE: &str = "visualizer-live";

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
    /// The settings file was written, or why it was not.
    Saved(Result<(), String>),
}

/// The widest the rows grow: beyond it a label and its control drift too far apart.
const SECTION: u16 = 76;

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
            SettingMsg::Saved(Ok(())) => Command::none(),
            SettingMsg::Saved(Err(reason)) => {
                Command::toast(Toast::warning(t!("music.settings.not-saved")).body(reason).key("saved"))
            }
        }
    }

    /// The settings page, in the place of the tracks.
    pub(super) fn settings_page(&self, ui: &mut View<'_, Msg>) {
        let page = |ui: &mut View<'_, Msg>| {
            ui.column(|ui| {
                ui.add(Words::new(t!("music.settings.title")).role("title"));
                SettingsList::show(ui, |list| {
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
