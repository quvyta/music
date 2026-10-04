//! The screen: the music of a folder in a table, and the player bar at the bottom with the track
//! heard, its time and the buttons that hold and step through it.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use qframe::prelude::*;
use qframe::runtime::{Task, TaskId, Update, UpdateCheck};
use qframe::storage::{Ecosystem, Preferences, Settings, UserDir, user_dir};
use qframe::widgets::{Appearance, AppearanceChange, TableRow, Toast};

use crate::audio::{AudioOut, Player, Problem, State, Status};
use crate::library::{self, Album, Artist, Track};
use crate::queue::{Queue, Repeat};
use crate::vis::{Spectrum, WINDOW};

pub use bus::Bus;

mod bus;
mod menu;
mod pages;
mod playlists;
mod settings;
mod view;

/// qmus's name among the Quvyta apps: its settings file is `music.conf` in the shared Quvyta
/// folder.
pub const APP: &str = "music";

/// How often the screen reads the player while a track is heard: thirty times a second, so the
/// visualizer moves with the sound.
const TICK: Duration = Duration::from_millis(33);

/// The bands of the visualizer: as many as the now-playing page shows at its widest; the player
/// bar's small one draws them merged.
const BANDS: usize = 48;

/// How far a seek key moves the track.
const SEEK: Duration = Duration::from_secs(5);

/// How much a volume key changes the volume, out of 100.
const VOLUME_STEP: u8 = 5;

/// How often the calm visualizer, shown when motion is reduced, takes a new reading.
const CALM: Duration = Duration::from_millis(250);

/// What qmus knows about the machine it runs on: where things are and where the sound goes.
///
/// Everything the screen reads from the environment is here, so a test hands it a machine made of
/// temporary folders and a silent output, and nothing reaches the person's own files or speakers.
#[derive(Debug, Clone)]
pub struct Machine {
    /// The shared Quvyta folder, which holds `music.conf` and the Quvyta-wide switches; `None`
    /// writes nothing and keeps every change in memory, though the shared Quvyta look is still
    /// read from where this platform keeps it, when it keeps one.
    pub config: Option<PathBuf>,
    /// Where the update notice is read and the last question remembered; `None` asks nothing.
    pub updates: Option<UpdateFolders>,
    /// Where the sound goes.
    pub audio: AudioOut,
    /// The person's home folder, which paths on screen begin with `~` for.
    pub home: Option<PathBuf>,
    /// qmus's state folder, where the queue is kept between runs; `None` keeps nothing.
    pub state: Option<PathBuf>,
    /// Where qmus offers itself to the desktop's media keys.
    pub bus: Bus,
    /// Where copies of album covers are kept for the desktop's "now playing" corner; `None`
    /// keeps none.
    pub covers: Option<PathBuf>,
    /// Where the person's playlists are kept; `None` keeps none.
    pub playlists: Option<PathBuf>,
}

impl Machine {
    /// This machine, as the environment describes it, with the sound on its default device.
    #[must_use]
    pub fn here() -> Self {
        Self {
            config: Ecosystem::QUVYTA.config_dir(),
            updates: UpdateFolders::here(),
            audio: AudioOut::Device,
            home: std::env::var_os("HOME").map(PathBuf::from),
            state: Ecosystem::QUVYTA.state_dir(APP),
            bus: Bus::Session,
            covers: Ecosystem::QUVYTA.cache_dir(APP).map(|cache| cache.join("art")),
            playlists: Ecosystem::QUVYTA.data_dir(APP).map(|data| data.join("playlists")),
        }
    }

    /// The person's Music folder: the one `user-dirs.dirs` names, or `~/Music`.
    #[must_use]
    pub fn music_folder() -> PathBuf {
        user_dir(UserDir::Music).unwrap_or_else(|| {
            std::env::var_os("HOME").map_or_else(|| PathBuf::from("Music"), |home| PathBuf::from(home).join("Music"))
        })
    }
}

/// Where the Quvyta-wide update notice is kept and where qmus remembers when it last asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateFolders {
    /// The shared Quvyta configuration folder, whose shared file holds the switch.
    pub config: PathBuf,
    /// qmus's state folder under the Quvyta one, which remembers when the question was last asked.
    pub state: PathBuf,
}

impl UpdateFolders {
    /// This machine's folders, or `None` without a home folder, where nothing could remember the
    /// switch or the last question and so nothing is asked.
    #[must_use]
    pub fn here() -> Option<Self> {
        let ecosystem = Ecosystem::QUVYTA;
        ecosystem.config_dir().zip(ecosystem.state_dir(APP)).map(|(config, state)| Self { config, state })
    }
}

/// How large a cover is decoded: enough for the now-playing page on a large terminal with pixel
/// graphics, small enough to decode in a moment.
const COVER: (u32, u32) = (600, 600);

/// How many albums' covers the screen keeps decoded, so going back to one is instant.
const COVERS_KEPT: usize = 64;

/// An album's cover as the screen has it: which album, the picture once decoded and the copy
/// kept for the desktop.
#[derive(Clone)]
pub struct Art {
    /// The track's folder and album, which every track of the album shares.
    key: (PathBuf, String),
    /// Whether the cover has been looked for; until then nothing stands in its place.
    read: bool,
    /// The picture; `None` when the album has none, or it could not be read.
    image: Option<Arc<qframe::widgets::ImageData>>,
    /// The copy kept for the desktop's "now playing" corner.
    file: Option<PathBuf>,
}

impl std::fmt::Debug for Art {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Art")
            .field("key", &self.key)
            .field("read", &self.read)
            .field("image", &self.image.is_some())
            .field("file", &self.file)
            .finish()
    }
}

/// The page the body shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// The track heard, large: its name, the visualizer across the page and how far it has got.
    NowPlaying,
    /// What plays and in what order, the track heard first.
    Queue,
    /// The tracks of the folder in a table.
    Tracks,
    /// The albums, and the tracks of the one opened.
    Albums,
    /// The artists, and the tracks of the one opened.
    Artists,
    /// The person's playlists, and the tracks of the one opened.
    Playlists,
}

/// What the screen starts with: qmus itself, its settings file and the shared Quvyta look the
/// runtime opens in.
pub struct Opening {
    /// The screen.
    pub music: Music,
    /// `music.conf`, for the runtime's saved look.
    pub settings: Settings,
    /// The shared Quvyta language, theme and icons, in force from the first frame.
    pub preferences: Preferences,
}

impl Opening {
    /// The screen on `machine`, showing the music of `folder`.
    #[must_use]
    pub fn new(machine: Machine, folder: PathBuf) -> Self {
        let ecosystem = Ecosystem::QUVYTA;
        let i18n = crate::locales::i18n();
        let (settings, preferences) = match &machine.config {
            Some(config) => (
                Settings::open(config.join(format!("{APP}.conf"))).member_of(&ecosystem),
                ecosystem.preferences_in(config, APP, &i18n),
            ),
            None => (Settings::in_memory(), ecosystem.preferences_without_saving(APP, &i18n)),
        };
        let appearance = match &machine.config {
            Some(config) => Appearance::new(ecosystem, APP, preferences.clone()).in_folder(config),
            None => Appearance::new(ecosystem, APP, preferences.clone()).without_saving(),
        };
        let music = Music::new(machine, folder, settings.clone(), appearance);
        Self { music, settings, preferences }
    }
}

/// The application's state.
pub struct Music {
    machine: Machine,
    folder: PathBuf,
    /// The tracks of the folder, once read.
    tracks: Option<Arc<[Track]>>,
    /// The table's rows, made once per reading of the folder.
    rows: Arc<[TableRow]>,
    /// The rows of the tracks the track table shows now: all of them, those the search finds, or
    /// those of the album or artist opened.
    list: Vec<usize>,
    /// The table rows of `list`.
    list_rows: Arc<[TableRow]>,
    /// The albums of the folder.
    albums: Arc<[Album]>,
    /// The artists of the folder.
    artists: Arc<[Artist]>,
    /// The albums the search finds, and their table rows.
    album_list: (Vec<usize>, Arc<[TableRow]>),
    /// The artists the search finds, and their table rows.
    artist_list: (Vec<usize>, Arc<[TableRow]>),
    /// The album opened on the albums page.
    album_open: Option<usize>,
    /// The artist opened on the artists page.
    artist_open: Option<usize>,
    /// The row under the cursor of the albums and of the artists table.
    group_cursor: (Option<usize>, Option<usize>),
    /// The row under the cursor of the queue.
    queue_cursor: Option<usize>,
    /// What the search box holds.
    query: String,
    /// Whether the side bar is open over the body on a narrow terminal.
    sidebar_open: bool,
    /// The row of each track, by its path.
    places: HashMap<PathBuf, usize>,
    /// What plays and in what order: the list from the row chosen on, shuffled or not.
    queue: Queue,
    /// The row the cursor is on in the track table, a place in `list`.
    cursor: Option<usize>,
    /// The row of the track loaded into the player.
    current: Option<usize>,
    player: Player,
    status: Status,
    /// The task that reads the player while a track is heard.
    ticker: Option<TaskId>,
    /// How long the task waits between two readings, in milliseconds: [`TICK`] while the
    /// visualizer moves with the sound, [`CALM`] while it is calm or not drawn. The drawing knows
    /// which, so it sets it, and the task reads it before every wait.
    beat: Arc<AtomicU64>,
    /// How many times the player has been read, for a test that counts the beats.
    #[cfg(test)]
    reads: usize,
    /// Whether the reading goes on only until the visualizer has come down to rest.
    settling: bool,
    /// How loud each band of the sound heard is.
    spectrum: Spectrum,
    /// The bands as the calm visualizer shows them, read afresh every [`CALM`].
    calm: Vec<f32>,
    /// How long the calm reading has stood.
    calm_age: Duration,
    /// The last of the sound heard, read from the player on each beat.
    heard: Vec<f32>,
    /// How loud the sound goes out, from 0 to 100.
    volume: u8,
    /// Whether the sound is silenced, keeping the volume to go back to.
    muted: bool,
    settings: Settings,
    appearance: Appearance,
    settings_open: bool,
    help_open: bool,
    /// The page the body shows.
    page: Page,
    /// The cover of the track loaded's album.
    art: Option<Art>,
    /// The covers read lately, the most recent last, at most [`COVERS_KEPT`].
    covers: Vec<Art>,
    /// The person's playlists.
    shelf: playlists::Shelf,
    /// The playlists' dialog open over the screen.
    dialog: Option<playlists::Dialog>,
    /// What qmus's own settings say.
    choices: settings::Choices,
    /// The folders the person added to the library, beside `folder`.
    added: Vec<PathBuf>,
    /// The folder picker that adds one, while it is open.
    picker: Option<qframe::widgets::FileBrowser>,
    /// Whether the folders are being read.
    reading: bool,
    /// Whether a list of tracks has been shown, so the queue of the last run is taken up once.
    listed: bool,
    /// qmus on the session bus, once it has taken its place there.
    mpris: Option<Arc<crate::mpris::Server>>,
}

impl std::fmt::Debug for Music {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Music").field("folder", &self.folder).field("status", &self.status).finish_non_exhaustive()
    }
}

/// Everything that can happen on this screen.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The folders have been read.
    Scanned(Arc<[Track]>),
    /// The index of the last run has been read.
    Indexed(Arc<[Track]>),
    /// The cursor moved to this row.
    Select(usize),
    /// This row was chosen: its track plays.
    Play(usize),
    /// The track heard is held, or goes on.
    PlayPause,
    /// The next track of the list.
    Next,
    /// The track before, or the start of this one once it has played a few seconds.
    Previous,
    /// The track heard moves a few seconds forward (`true`) or back.
    Seek(bool),
    /// What is still to come is shuffled, or put back in the order of the list.
    Shuffle,
    /// Repeat goes round: off, the whole queue, the track heard.
    Repeat,
    /// The sound gets louder (`true`) or quieter by a step.
    Volume(bool),
    /// The sound is silenced, or comes back at the volume it had.
    Mute,
    /// Time to read the player again.
    Tick,
    /// The settings page opens or closes.
    Settings(bool),
    /// The key overview opens or closes.
    Help(bool),
    /// qmus closes, keeping the queue for the next run.
    Quit,
    /// The body shows this page.
    Page(Page),
    /// Something on the queue, albums or artists page, the search or the side bar.
    Pages(pages::PageMsg),
    /// Something on the playlists page or in its dialogs.
    Lists(playlists::ListMsg),
    /// A change on the settings page.
    Setting(settings::SettingMsg),
    /// An entry of a track's menu was chosen.
    Menu(menu::TrackMenu),
    /// An album's cover has been read.
    Art(Art),
    /// Esc: closes whatever is open on top.
    Cancel,
    /// A change on the settings page's appearance rows.
    Appearance(AppearanceChange),
    /// Another Quvyta application changed the shared language, theme, icons or reduced motion.
    Preferences(Preferences),
    /// A newer version of qmus is out.
    NewVersion(Update),
    /// qmus has taken its place on the session bus.
    Served(Arc<crate::mpris::Server>),
    /// A client on the bus asks for something.
    Asked(crate::mpris::Request),
}

impl Music {
    fn new(machine: Machine, folder: PathBuf, settings: Settings, appearance: Appearance) -> Self {
        let player = Player::start(machine.audio);
        // The volume of the last run, when it kept one; a file that says anything else is passed over.
        let volume = machine
            .state
            .as_ref()
            .and_then(|state| fs::read_to_string(state.join("volume")).ok())
            .and_then(|text| text.trim().parse::<u8>().ok())
            .map_or(100, |level| level.min(100));
        player.set_volume(volume);
        let choices = settings::Choices::of(&settings);
        let added = settings::added_folders(&settings);
        Self {
            machine,
            folder,
            tracks: None,
            rows: Arc::from(Vec::new()),
            list: Vec::new(),
            list_rows: Arc::from(Vec::new()),
            albums: Arc::from(Vec::new()),
            artists: Arc::from(Vec::new()),
            album_list: (Vec::new(), Arc::from(Vec::new())),
            artist_list: (Vec::new(), Arc::from(Vec::new())),
            album_open: None,
            artist_open: None,
            group_cursor: (None, None),
            queue_cursor: None,
            query: String::new(),
            sidebar_open: false,
            places: HashMap::new(),
            queue: Queue::from(Vec::<PathBuf>::new(), 0),
            cursor: None,
            current: None,
            player,
            status: Status::default(),
            ticker: None,
            beat: Arc::new(AtomicU64::new(millis(TICK))),
            #[cfg(test)]
            reads: 0,
            settling: false,
            spectrum: Spectrum::new(BANDS),
            calm: vec![0.0; BANDS],
            calm_age: Duration::ZERO,
            heard: vec![0.0; WINDOW],
            volume,
            muted: false,
            settings,
            appearance,
            settings_open: false,
            help_open: false,
            page: Page::Tracks,
            art: None,
            covers: Vec::new(),
            choices,
            added,
            picker: None,
            reading: false,
            listed: false,
            shelf: playlists::Shelf::default(),
            dialog: None,
            mpris: None,
        }
    }

    /// The folder whose music is shown.
    #[must_use]
    pub fn folder(&self) -> &PathBuf {
        &self.folder
    }

    /// The folder whose music is shown, as the screen writes it: under the home folder, from `~`,
    /// and how many folders were added beside it.
    fn folder_shown(&self) -> String {
        let folder = qframe::storage::display_home_with(&self.folder, self.machine.home.as_deref());
        if self.added.is_empty() { folder } else { format!("{folder} +{}", self.added.len()) }
    }

    /// The folders the library is read from: the one qmus opens with, then those added.
    fn sources(&self) -> Vec<PathBuf> {
        std::iter::once(self.folder.clone()).chain(self.added.iter().cloned()).collect()
    }

    /// Where the library's index is kept between runs; `None` keeps none.
    fn index_file(&self) -> Option<PathBuf> {
        self.machine.state.as_ref().map(|state| state.join("library.index"))
    }

    /// Where the player stood when the screen last read it.
    #[must_use]
    pub fn status(&self) -> &Status {
        &self.status
    }

    /// The tracks of the folder, in the order listed; none until the folder has been read.
    #[must_use]
    pub fn tracks(&self) -> &[Track] {
        self.tracks.as_deref().unwrap_or_default()
    }

    /// The track loaded into the player, when there is one.
    #[must_use]
    pub fn current(&self) -> Option<&Track> {
        self.tracks.as_ref()?.get(self.current?)
    }

    /// The question for a newer version of qmus, when the Quvyta-wide update notice is on.
    fn ask_for_update(&self) -> Command<Msg> {
        let Some(folders) = &self.machine.updates else { return Command::none() };
        if !Ecosystem::QUVYTA.update_notice_in(&folders.config) {
            return Command::none();
        }
        let check = UpdateCheck::new(
            Ecosystem::QUVYTA,
            APP,
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            Msg::NewVersion,
        )
        .in_folders(folders.config.clone(), folders.state.clone());
        Command::check_for_update(check)
    }

    /// Reads the folders off the drawing thread, the tags only of files new or changed since the
    /// index was written.
    fn scan(&mut self) -> Command<Msg> {
        self.reading = true;
        let sources = self.sources();
        let index = self.index_file();
        Command::perform(move || {
            let tracks = match index {
                Some(index) => library::scan_with_index(&sources, &index),
                None => library::scan_all(&sources),
            };
            Msg::Scanned(tracks.into())
        })
    }

    /// The tracks the index of the last run holds, shown while the folders are read again.
    fn read_index(&self) -> Command<Msg> {
        let Some(index) = self.index_file() else { return Command::none() };
        Command::perform(move || Msg::Indexed(library::read_index(&index).into()))
    }

    /// Shows `tracks` as the library, keeping the track heard, the row under the cursor and the
    /// album or artist opened where they are still found.
    fn list_tracks(&mut self, tracks: Arc<[Track]>) -> Command<Msg> {
        let heard = self.current().map(|track| track.path.clone());
        let under = self.cursor.and_then(|at| self.list.get(at)).and_then(|row| self.tracks().get(*row));
        let under = under.map(|track| track.path.clone());
        let album =
            self.album_open.and_then(|at| self.albums.get(at)).map(|album| (album.title.clone(), album.artist.clone()));
        let artist = self.artist_open.and_then(|at| self.artists.get(at)).map(|artist| artist.name.clone());
        self.rows = view::rows(&tracks);
        self.places = tracks.iter().enumerate().map(|(index, track)| (track.path.clone(), index)).collect();
        self.albums = library::albums(&tracks).into();
        self.artists = library::artists(&tracks).into();
        self.tracks = Some(tracks);
        self.current = heard.and_then(|path| self.places.get(&path).copied());
        self.album_open = album
            .and_then(|(title, by)| self.albums.iter().position(|album| album.title == title && album.artist == by));
        self.artist_open = artist.and_then(|name| self.artists.iter().position(|artist| artist.name == name));
        // The playlists are known from the start, for the menu that adds a track to one.
        self.read_playlists();
        self.refresh_lists();
        let row = under.and_then(|path| self.places.get(&path).copied());
        self.cursor = row
            .and_then(|row| self.list.iter().position(|at| *at == row))
            .or_else(|| (!self.list.is_empty()).then_some(0));
        if std::mem::replace(&mut self.listed, true) {
            return Command::none();
        }
        let art = if self.current.is_none() && self.choices.resume { self.take_up_queue() } else { Command::none() };
        Command::batch([Command::focus(view::TRACKS), art])
    }

    /// Plays the track of row `index`, with the rest of the list after it in the queue, and starts
    /// reading the player; a track qmus cannot play yet says so and leaves the player as it was.
    fn play(&mut self, index: usize) -> Command<Msg> {
        let Some(track) = self.tracks.as_ref().and_then(|tracks| tracks.get(index)) else { return Command::none() };
        if !track.playable {
            return Command::toast(Toast::warning(t!("music.later.title")).body(t!("music.later.text")).key("play"));
        }
        // The queue is the list shown, when the track is in it, from the track on.
        let (rows, at) = match self.list.iter().position(|row| *row == index) {
            Some(at) => (self.list.clone(), at),
            None => ((0..self.tracks().len()).collect(), index),
        };
        let tracks = self.tracks();
        let mut queue = Queue::from(rows.iter().map(|row| tracks[*row].path.as_path()), at);
        queue.set_repeat(self.queue.repeat());
        if self.queue.is_shuffled() {
            queue.set_shuffle(true, seed());
        }
        self.queue = queue;
        self.play_row(index)
    }

    /// Plays the track of row `index` as the queue has it now.
    fn play_row(&mut self, index: usize) -> Command<Msg> {
        let Some(track) = self.tracks.as_ref().and_then(|tracks| tracks.get(index)) else { return Command::none() };
        self.player.play(track.path.clone());
        self.current = Some(index);
        self.status = self.player.status();
        self.follow();
        Command::batch([self.tick(), self.read_art()])
    }

    /// Reads the cover of the track loaded's album off the drawing thread, unless it is the album
    /// shown already or one read lately.
    fn read_art(&mut self) -> Command<Msg> {
        let Some(track) = self.current() else { return Command::none() };
        let folder = track.path.parent().map(Path::to_path_buf).unwrap_or_default();
        let key = (folder, track.album.clone());
        if self.art.as_ref().is_some_and(|shown| shown.key == key) {
            return Command::none();
        }
        if let Some(at) = self.covers.iter().position(|kept| kept.key == key) {
            let kept = self.covers.remove(at);
            self.art = Some(kept.clone());
            self.covers.push(kept);
            return Command::none();
        }
        let path = track.path.clone();
        let cache = self.machine.covers.clone();
        self.art = Some(Art { key: key.clone(), read: false, image: None, file: None });
        Command::perform(move || {
            let image = crate::art::load(&path, COVER).map(Arc::new);
            let file = cache.and_then(|cache| crate::art::keep(&path, &cache, &crate::art::name_of(&key.0, &key.1)));
            Msg::Art(Art { key, read: true, image, file })
        })
    }

    /// Keeps `art` among the covers read lately, letting the oldest go past [`COVERS_KEPT`].
    fn keep_cover(&mut self, art: Art) {
        self.covers.retain(|kept| kept.key != art.key);
        self.covers.push(art);
        if self.covers.len() > COVERS_KEPT {
            self.covers.remove(0);
        }
    }

    /// Tells the player what the queue plays after the track loaded, so it follows with no gap;
    /// a track qmus cannot play is not followed into, the queue passes it over after a stop.
    fn follow(&self) {
        let next = self.queue.peek_next().filter(|path| self.playable_row(path).is_some());
        self.player.follow_with(next.map(Path::to_path_buf));
    }

    /// The player went on into the track the queue named next: the queue moves on with it.
    fn went_on(&mut self, heard: &Path) -> Command<Msg> {
        let index = match self.following() {
            Some(index) if self.tracks()[index].path == heard => index,
            _ => match self.places.get(heard) {
                Some(index) => *index,
                None => return Command::none(),
            },
        };
        self.current = Some(index);
        self.follow();
        self.read_art()
    }

    /// Holds the track heard, or goes on with it; with nothing loaded, plays the row under the cursor.
    fn play_pause(&mut self) -> Command<Msg> {
        match self.status.state {
            State::Playing => {
                self.player.pause();
                self.status.state = State::Paused;
                Command::none()
            }
            State::Paused => {
                self.player.resume();
                self.status.state = State::Playing;
                self.tick()
            }
            State::Stopped | State::Ended => {
                let under = self.cursor.and_then(|at| self.list.get(at).copied());
                self.play(self.current.or(under).unwrap_or(0))
            }
        }
    }

    /// The next track of the queue that qmus can play, when there is one. Asked for by the
    /// person, it goes on to the following track even when one track repeats.
    fn next(&mut self) -> Command<Msg> {
        if self.current.is_none() {
            return Command::none();
        }
        let repeat = self.queue.repeat();
        if repeat == Repeat::One {
            self.queue.set_repeat(Repeat::All);
        }
        let next = self.following();
        self.queue.set_repeat(repeat);
        next.map_or_else(Command::none, |index| self.play_row(index))
    }

    /// Moves the queue on to the next track qmus can play and gives its row; `None` when the queue
    /// ends first. Each track is looked at once at most, so a queue of nothing playable ends.
    fn following(&mut self) -> Option<usize> {
        for _ in 0..=self.tracks().len() {
            let path = self.queue.advance()?.to_path_buf();
            if let Some(index) = self.playable_row(&path) {
                return Some(index);
            }
        }
        None
    }

    /// The row of the track at `path`, when qmus can play it.
    fn playable_row(&self, path: &Path) -> Option<usize> {
        let index = *self.places.get(path)?;
        self.tracks().get(index).is_some_and(|track| track.playable).then_some(index)
    }

    /// The queue's track before that qmus can play, or this track over from its start once it has
    /// played a few seconds.
    fn previous(&mut self) -> Command<Msg> {
        let Some(current) = self.current else { return Command::none() };
        let mut position = self.status.position;
        for _ in 0..=self.tracks().len() {
            let Some(path) = self.queue.previous(position).map(Path::to_path_buf) else { break };
            match self.playable_row(&path) {
                Some(index) => return self.play_row(index),
                None if self.queue.history().next().is_some() => position = Duration::ZERO,
                // Back at the start of the queue on a track qmus cannot play: on to the first it can.
                None => return self.following().map_or_else(Command::none, |index| self.play_row(index)),
            }
        }
        self.play_row(current)
    }

    /// Moves the track heard [`SEEK`] forward or back, within its length.
    fn seek(&mut self, forward: bool) {
        if !matches!(self.status.state, State::Playing | State::Paused) {
            return;
        }
        let Some(track) = self.current() else { return };
        let position = self.status.position;
        let at = if forward { position + SEEK } else { position.saturating_sub(SEEK) };
        let at = track.duration.map_or(at, |total| at.min(total));
        self.player.seek(at);
        self.status = self.player.status();
        self.announce_seek();
    }

    /// Where the queue is kept between runs.
    fn queue_file(&self) -> Option<PathBuf> {
        self.machine.state.as_ref().map(|state| state.join("queue"))
    }

    /// The queue of the last run, held at the track and the moment it was left at, when its
    /// track is among the tracks shown.
    fn take_up_queue(&mut self) -> Command<Msg> {
        let Some((queue, at)) = self.queue_file().and_then(|file| Queue::load(&file)) else { return Command::none() };
        let Some(index) = queue.current().and_then(|path| self.playable_row(path)) else { return Command::none() };
        self.queue = queue;
        self.current = Some(index);
        self.cursor = self.list.iter().position(|row| *row == index);
        self.player.cue(self.tracks()[index].path.clone(), at);
        self.status = self.player.status();
        self.follow();
        self.read_art()
    }

    /// Keeps the queue and the moment heard for the next run. A queue that cannot be written is
    /// lost, never the reason qmus does not close.
    fn keep_queue(&self) {
        if let Some(file) = self.queue_file() {
            let _ = self.queue.save(&file, self.player.status().position);
        }
        if let Some(state) = &self.machine.state {
            let _ = qframe::storage::atomic_write(&state.join("volume"), format!("{}\n", self.volume).as_bytes());
        }
    }

    /// Shuffles what is still to come, or puts it back in the order of the list.
    fn shuffle(&mut self) {
        let on = !self.queue.is_shuffled();
        self.queue.set_shuffle(on, seed());
        self.follow();
    }

    /// Repeat goes round: off, the whole queue, the track heard.
    fn repeat(&mut self) {
        let next = match self.queue.repeat() {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        };
        self.queue.set_repeat(next);
        self.follow();
    }

    /// Sets the volume `level`, out of 100, and hears it, unless the sound is silenced.
    fn set_volume(&mut self, level: u8) {
        self.volume = level.min(100);
        self.muted = false;
        self.player.set_volume(self.volume);
    }

    /// Starts reading the player on a steady beat, unless it is read already.
    fn tick(&mut self) -> Command<Msg> {
        self.settling = false;
        if self.ticker.is_some() {
            return Command::none();
        }
        let beat = Arc::clone(&self.beat);
        let task = Task::new("player", move |cx| {
            while cx.sleep(Duration::from_millis(beat.load(Ordering::Relaxed))) {
                cx.send(Msg::Tick);
            }
            Ok(Msg::Tick)
        });
        self.ticker = Some(task.id());
        Command::task(task)
    }

    /// Reads the player: a track heard to its end, or one that could not be played, gives way to
    /// the next row; a sound device that went away stops everything and says so; a player that
    /// stands still stops being read.
    fn read_player(&mut self) -> Command<Msg> {
        #[cfg(test)]
        {
            self.reads += 1;
        }
        self.status = self.player.status();
        let current = self.current().map(|track| track.path.clone());
        let art = match self.status.track.clone() {
            Some(heard) if current.is_some_and(|current| current != heard) => self.went_on(&heard),
            _ => Command::none(),
        };
        let then = self.read_player_state();
        Command::batch([art, then])
    }

    /// What the state the player is in asks of the screen.
    fn read_player_state(&mut self) -> Command<Msg> {
        if self.status.state == State::Playing && self.choices.style != settings::Style::Off {
            let rate = self.player.heard(&mut self.heard);
            self.spectrum.hear(&self.heard, rate, self.beat());
        } else {
            self.spectrum.quiet(self.beat());
        }
        self.calm_age += self.beat();
        if self.calm_age >= CALM {
            self.calm_age = Duration::ZERO;
            self.calm.copy_from_slice(self.spectrum.measured());
        }
        match (self.status.state, self.status.problem.clone()) {
            (State::Playing, _) => Command::none(),
            (State::Ended | State::Stopped, Some(Problem::Track(reason))) => {
                let name = self.current().map_or_else(String::new, |track| track.title.clone());
                let told = Command::toast(
                    Toast::danger(t!("music.play.failed", name = name.as_str())).body(reason).key("play"),
                );
                Command::batch([told, self.go_on()])
            }
            (State::Ended, _) => self.go_on(),
            (State::Stopped, Some(Problem::Output(reason))) => {
                let told = Command::toast(Toast::danger(t!("music.output.lost")).body(reason).key("output"));
                Command::batch([told, self.stop_ticking()])
            }
            (State::Paused | State::Stopped, _) => self.stop_ticking(),
        }
    }

    /// The queue's next track once the one loaded is over, or the end of reading when there is
    /// none.
    fn go_on(&mut self) -> Command<Msg> {
        match self.following() {
            Some(index) => self.play_row(index),
            None => self.stop_ticking(),
        }
    }

    /// Stops reading the player once the visualizer has come down to rest; until then the beat
    /// goes on and only moves the visualizer.
    fn stop_ticking(&mut self) -> Command<Msg> {
        if !self.spectrum.resting() {
            self.settling = true;
            return Command::none();
        }
        self.settling = false;
        self.calm.fill(0.0);
        self.ticker.take().map_or_else(Command::none, Command::cancel_task)
    }

    /// One beat while the visualizer comes down to rest after the sound stopped.
    fn settle(&mut self) -> Command<Msg> {
        self.spectrum.quiet(self.beat());
        self.calm.fill(0.0);
        if self.spectrum.resting() { self.stop_ticking() } else { Command::none() }
    }

    /// What `msg` changes on the screen, and what it asks to be done next.
    fn handle(&mut self, msg: Msg) -> Command<Msg> {
        match msg {
            Msg::Scanned(tracks) => {
                self.reading = false;
                return self.list_tracks(tracks);
            }
            // The index is only a quick first look: once the folders themselves are read, it is late.
            Msg::Indexed(tracks) if self.tracks.is_none() && !tracks.is_empty() => return self.list_tracks(tracks),
            Msg::Indexed(_) => {}
            Msg::Select(at) => self.cursor = Some(at),
            Msg::Play(at) => {
                self.cursor = Some(at);
                let Some(row) = self.list.get(at).copied() else { return Command::none() };
                return self.play(row);
            }
            Msg::Pages(msg) => return self.page_msg(msg),
            Msg::Lists(msg) => return self.list_msg(msg),
            Msg::Setting(msg) => return self.setting_msg(msg),
            Msg::Menu(msg) => return self.menu_msg(msg),
            Msg::PlayPause => return self.play_pause(),
            Msg::Next => return self.next(),
            Msg::Previous => return self.previous(),
            Msg::Seek(forward) => self.seek(forward),
            Msg::Shuffle => self.shuffle(),
            Msg::Repeat => self.repeat(),
            Msg::Volume(louder) => {
                let level = if louder {
                    self.volume.saturating_add(VOLUME_STEP)
                } else {
                    self.volume.saturating_sub(VOLUME_STEP)
                };
                self.set_volume(level);
            }
            Msg::Mute => {
                if self.muted {
                    self.set_volume(self.volume);
                } else {
                    self.muted = true;
                    self.player.set_volume(0);
                }
            }
            Msg::Tick => {
                if self.ticker.is_some() {
                    return if self.settling { self.settle() } else { self.read_player() };
                }
            }
            Msg::Settings(open) => {
                self.settings_open = open;
                if !open {
                    return Command::focus(view::TRACKS);
                }
            }
            Msg::Help(open) => self.help_open = open,
            Msg::Art(art) => {
                // A cover read for an album no longer heard is kept, but not shown.
                if self.art.as_ref().is_some_and(|shown| shown.key == art.key) {
                    self.art = Some(art.clone());
                }
                self.keep_cover(art);
            }
            Msg::Quit => {
                self.keep_queue();
                return Command::quit();
            }
            Msg::Page(page) => return self.show_page(page),
            Msg::Cancel => {
                if self.settings_open {
                    return self.update(Msg::Settings(false));
                }
                return self.back();
            }
            Msg::Appearance(change) => return self.appearance.update(change, &mut self.settings),
            Msg::Preferences(preferences) => self.appearance.refresh(preferences),
            Msg::NewVersion(update) => return Command::toast(update.toast()),
            Msg::Served(server) => self.mpris = Some(server),
            Msg::Asked(request) => return self.asked(request),
        }
        Command::none()
    }
}

impl App for Music {
    type Msg = Msg;

    fn init(&mut self) -> Command<Msg> {
        Command::batch([self.read_index(), self.scan(), self.ask_for_update(), self.serve_bus()])
    }

    fn preferences(&self, preferences: &Preferences) -> Option<Msg> {
        Some(Msg::Preferences(preferences.clone()))
    }

    fn update(&mut self, msg: Msg) -> Command<Msg> {
        let command = self.handle(msg);
        self.announce();
        command
    }

    fn view(&self, ui: &mut View<'_, Msg>) {
        self.screen(ui);
    }

    fn before_quit(&self) -> Option<Msg> {
        Some(Msg::Quit)
    }

    fn terminating(&self, _cause: qframe::runtime::Termination) -> Option<Msg> {
        Some(Msg::Quit)
    }

    fn action(&self, name: &str) -> Option<Msg> {
        let msg = match name {
            "play-pause" => Msg::PlayPause,
            "next" => Msg::Next,
            "previous" => Msg::Previous,
            "now-playing" => Msg::Page(Page::NowPlaying),
            "queue" => Msg::Page(Page::Queue),
            "tracks" => Msg::Page(Page::Tracks),
            "albums" => Msg::Page(Page::Albums),
            "artists" => Msg::Page(Page::Artists),
            "playlists" => Msg::Page(Page::Playlists),
            "save-queue" => Msg::Lists(playlists::ListMsg::SaveAsk),
            "remove" if self.page == Page::Playlists && self.shelf.open.is_none() => {
                Msg::Lists(playlists::ListMsg::RemoveAsk)
            }
            "search" => Msg::Pages(pages::PageMsg::SearchFocus),
            "sidebar" => Msg::Pages(pages::PageMsg::Sidebar(!self.sidebar_open)),
            "remove" if self.page == Page::Queue => Msg::Pages(pages::PageMsg::QueueRemove),
            "shuffle" => Msg::Shuffle,
            "repeat" => Msg::Repeat,
            "seek-forward" => Msg::Seek(true),
            "seek-back" => Msg::Seek(false),
            "volume-up" => Msg::Volume(true),
            "volume-down" => Msg::Volume(false),
            "settings" => Msg::Settings(!self.settings_open),
            "help" => Msg::Help(true),
            "cancel" => Msg::Cancel,
            _ => return None,
        };
        Some(msg)
    }
}

impl Music {
    /// How long passes between two readings of the player now.
    fn beat(&self) -> Duration {
        Duration::from_millis(self.beat.load(Ordering::Relaxed))
    }

    /// Reads the player as often as what is drawn needs: thirty times a second while the
    /// visualizer moves with the sound, four times while it is calm or not drawn, which is
    /// enough for the time and the end of a track.
    pub(super) fn set_beat(&self, moving: bool) {
        self.beat.store(millis(if moving { TICK } else { CALM }), Ordering::Relaxed);
    }
}

/// `duration` in whole milliseconds.
fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// A number to shuffle by, different each time it is asked for.
fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| u64::try_from(since.as_nanos() % u128::from(u64::MAX)).unwrap_or(0))
}

/// `duration` as a player shows it: `3:07`, or `1:02:45` from an hour on.
#[must_use]
pub fn clock(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 { format!("{hours}:{minutes:02}:{seconds:02}") } else { format!("{minutes}:{seconds:02}") }
}

#[cfg(test)]
mod tests;
