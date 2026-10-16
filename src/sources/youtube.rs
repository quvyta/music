//! YouTube Music, through the person's own yt-dlp.
//!
//! qmus neither bundles nor downloads yt-dlp: it uses the one the person installed, and offers no
//! YouTube Music account without it. Everything qmus asks of YouTube goes through yt-dlp, run as a
//! program of its own with a bound on how long it may take. yt-dlp reads the person's YouTube
//! login from the browser they name; qmus never sees, keeps or writes that login.

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{Reach, RemoteTrack, SourceError};

/// The browsers yt-dlp reads a YouTube login from, by the names it knows them by.
pub const BROWSERS: [&str; 7] = ["firefox", "chrome", "chromium", "brave", "edge", "vivaldi", "opera"];

/// The person's liked music.
const LIKED: &str = "https://music.youtube.com/playlist?list=LM";

/// How long reading the liked music may take: a long list is read in many pages.
const LIST_PATIENCE: Duration = Duration::from_secs(120);

/// How long finding a track's sound may take.
const AUDIO_PATIENCE: Duration = Duration::from_secs(30);

/// The sound qmus asks for: AAC, which qmus decodes itself.
const FORMAT: &str = "bestaudio[ext=m4a]/bestaudio[acodec^=mp4a]";

/// The most of a failure's words kept to show the person.
const SAID: usize = 200;

/// The yt-dlp in the folders of `search`, a value like `PATH`, when there is one.
#[must_use]
pub fn find_in(search: &OsStr) -> Option<PathBuf> {
    program_in(search, "yt-dlp")
}

/// The program `name` in the folders of `search`, when one of them has it.
fn program_in(search: &OsStr, name: &str) -> Option<PathBuf> {
    std::env::split_paths(search).map(|folder| folder.join(name)).find(|program| is_program(program))
}

/// How this machine's package manager would install yt-dlp, found in the folders of `search`;
/// `None` where there is no package manager qmus knows. Only shown to the person: qmus never runs
/// it.
#[must_use]
pub fn install_command(search: &OsStr) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let root = std::fs::metadata("/proc/self").is_ok_and(|meta| meta.uid() == 0);
    qframe::install::Install::package_with("yt-dlp", |program| program_in(search, program), root)
        .map(|install| install.command_line())
}

/// Whether `path` is a file this user may run.
fn is_program(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata().is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// The person's yt-dlp, and the browser whose YouTube login it reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YtDlp {
    /// The program.
    program: PathBuf,
    /// The browser, one of [`BROWSERS`].
    browser: String,
    /// How long reading a list may take, and finding a track's sound.
    patience: (Duration, Duration),
}

impl YtDlp {
    /// The yt-dlp at `program`, reading the YouTube login of `browser`.
    #[must_use]
    pub fn new(program: &Path, browser: &str) -> Self {
        Self { program: program.to_path_buf(), browser: browser.to_owned(), patience: (LIST_PATIENCE, AUDIO_PATIENCE) }
    }

    /// The same, waiting at most `list` for a list and `audio` for a track's sound.
    #[must_use]
    pub fn patience(mut self, list: Duration, audio: Duration) -> Self {
        self.patience = (list, audio);
        self
    }

    /// The version of yt-dlp, which says it runs.
    ///
    /// # Errors
    ///
    /// Why yt-dlp could not be run.
    pub fn version(&self) -> Result<String, SourceError> {
        Ok(self.run(&["--version"], self.patience.1, false)?.trim().to_owned())
    }

    /// The person's liked music, as YouTube Music lists it.
    ///
    /// # Errors
    ///
    /// [`SourceError::Login`] when yt-dlp finds no YouTube login in the browser, and why yt-dlp
    /// failed otherwise.
    pub fn liked(&self) -> Result<Vec<RemoteTrack>, SourceError> {
        let text = self.run(&["--flat-playlist", "-J", LIKED], self.patience.0, true)?;
        let list: Value =
            serde_json::from_str(&text).map_err(|_| SourceError::Server("yt-dlp gave no list".to_owned()))?;
        Ok(list.get("entries").and_then(Value::as_array).into_iter().flatten().filter_map(track_of).collect())
    }

    /// The address the sound of the track `id` is fetched from now; it soon runs out.
    ///
    /// # Errors
    ///
    /// Why yt-dlp could not give it, in words for the person.
    pub fn audio_address(&self, id: &str) -> Result<String, String> {
        let watch = format!("https://music.youtube.com/watch?v={id}");
        let text =
            self.run(&["-f", FORMAT, "-g", "--", &watch], self.patience.1, true).map_err(|error| match error {
                SourceError::Server(said) => said,
                SourceError::Login => "yt-dlp found no YouTube login in the browser".to_owned(),
                _ => "yt-dlp did not answer in time".to_owned(),
            })?;
        let address = text.lines().map(str::trim).find(|line| line.starts_with("http")).unwrap_or_default();
        if address.is_empty() { Err("yt-dlp gave no sound for this track".to_owned()) } else { Ok(address.to_owned()) }
    }

    /// A finder of the address of the track `id`'s sound, for the player to ask when it opens it.
    #[must_use]
    pub fn resolver(&self, id: &str) -> crate::audio::Resolver {
        let (ytdlp, id) = (self.clone(), id.to_owned());
        std::sync::Arc::new(move || ytdlp.audio_address(&id))
    }

    /// The picture at `address`, a track's cover on YouTube's public picture server.
    ///
    /// # Errors
    ///
    /// Why it could not be fetched.
    pub fn cover(&self, address: &str) -> Result<Vec<u8>, SourceError> {
        let mut answer =
            super::agent().get(address).call().map_err(|error| SourceError::Unreachable(super::reach_of(&error)))?;
        if !answer.status().is_success() {
            return Err(SourceError::NotFound);
        }
        answer
            .body_mut()
            .with_config()
            .limit(16 * 1024 * 1024)
            .read_to_vec()
            .map_err(|error| SourceError::Unreachable(super::reach_of(&error)))
    }

    /// Runs yt-dlp with `args`, with the browser's login when `signed`, for at most `patience`,
    /// and gives what it wrote.
    fn run(&self, args: &[&str], patience: Duration, signed: bool) -> Result<String, SourceError> {
        let mut command = Command::new(&self.program);
        if signed {
            command.args(["--cookies-from-browser", &self.browser]);
        }
        let mut child = command
            .args(["--no-warnings", "--no-progress"])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| SourceError::Server("yt-dlp could not be run".to_owned()))?;
        // What it writes is read as it comes, so a long list never fills the pipe and stops it.
        let reader = |stream: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut text = String::new();
                if let Some(mut stream) = stream {
                    let _ = stream.read_to_string(&mut text);
                }
                text
            })
        };
        let out = reader(child.stdout.take().map(|out| Box::new(out) as Box<dyn Read + Send>));
        let err = reader(child.stderr.take().map(|err| Box::new(err) as Box<dyn Read + Send>));
        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if start.elapsed() >= patience => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(SourceError::Unreachable(Reach::Timeout));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => return Err(SourceError::Server("yt-dlp could not be run".to_owned())),
            }
        };
        let (out, err) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
        if status.success() {
            return Ok(out);
        }
        Err(failure_of(&err))
    }
}

/// What yt-dlp's words on a failure say: a missing login, or the last thing it said.
fn failure_of(said: &str) -> SourceError {
    let lower = said.to_lowercase();
    if lower.contains("sign in") || lower.contains("cookies") || lower.contains("login") {
        return SourceError::Login;
    }
    let last = said.lines().rev().map(str::trim).find(|line| !line.is_empty()).unwrap_or("yt-dlp failed");
    SourceError::Server(last.chars().take(SAID).collect())
}

/// The track an entry of a flat list describes.
fn track_of(entry: &Value) -> Option<RemoteTrack> {
    let id = entry.get("id")?.as_str()?.to_owned();
    let text = |name: &str| entry.get(name).and_then(Value::as_str).map(str::trim).unwrap_or_default().to_owned();
    let channel = [text("artist"), text("channel"), text("uploader")].into_iter().find(|name| !name.is_empty());
    // A topic channel is the artist's own, named "Artist - Topic".
    let artist = channel.unwrap_or_default().trim_end_matches(" - Topic").to_owned();
    let seconds = entry.get("duration").and_then(Value::as_f64).filter(|seconds| *seconds > 0.0);
    Some(RemoteTrack {
        cover: Some(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg")),
        id,
        title: text("title"),
        artist,
        album: text("album"),
        number: None,
        duration: seconds.map(Duration::from_secs_f64),
    })
}

#[cfg(test)]
mod tests;
