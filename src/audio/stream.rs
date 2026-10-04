//! Playing a track that comes over the network: it is fetched into qmus's own cache while the
//! decoder reads the same file, waiting where it has not arrived yet.
//!
//! A track fetched to its end is kept under its name and played from the cache the next time
//! without asking the server again. The cache is qmus's, never the person's music, and is held to a
//! size by letting the oldest tracks go.
//!
//! The address a track is fetched from carries the login, so no error here repeats it.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

use md5::{Digest, Md5};
use symphonia::core::io::MediaSource;

/// How long a read may wait for bytes that have not arrived before the track is given up: long
/// enough for a home server on a slow line, short enough that a server gone silent is said.
const PATIENCE: Duration = Duration::from_secs(30);

/// The longest a single track may take to arrive.
const WHOLE: Duration = Duration::from_secs(30 * 60);

/// How much is fetched between two looks at whether the reader still wants it.
const PIECE: usize = 64 * 1024;

/// The ending of a track still being fetched.
const PARTIAL: &str = "part";

/// The tracks fetched over the network, kept in a folder of qmus's own.
#[derive(Debug, Clone)]
pub struct StreamCache {
    /// The folder.
    folder: PathBuf,
    /// The most the folder holds, in bytes, before the oldest tracks go.
    cap: u64,
    /// How long a read waits for bytes.
    patience: Duration,
}

impl StreamCache {
    /// The cache in `folder`, holding at most `cap` bytes.
    #[must_use]
    pub fn new(folder: PathBuf, cap: u64) -> Self {
        Self { folder, cap, patience: PATIENCE }
    }

    /// The same cache waiting at most `patience` for bytes that have not arrived.
    #[must_use]
    pub fn patience(mut self, patience: Duration) -> Self {
        self.patience = patience;
        self
    }

    /// The track known as `key`, to be read from its start: from the cache when it was fetched
    /// whole before, else fetched from `address` as it is read.
    ///
    /// # Errors
    ///
    /// Says why when the cache folder cannot be written or the server does not send the track.
    pub fn open(&self, key: &str, address: &str) -> Result<Box<dyn MediaSource>, String> {
        let whole = self.folder.join(name_of(key));
        if let Ok(file) = File::open(&whole) {
            // Played again: the cache's own order of age follows what is heard, not what was fetched.
            let _ = file.set_modified(SystemTime::now());
            return Ok(Box::new(file));
        }
        fs::create_dir_all(&self.folder).map_err(|error| error.to_string())?;
        self.trim();
        let partial = whole.with_extension(PARTIAL);
        let writer = File::create(&partial).map_err(|error| error.to_string())?;
        let reader = File::open(&partial).map_err(|error| error.to_string())?;
        let shared = Arc::new(Shared::default());
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(8)))
            .timeout_recv_response(Some(self.patience))
            // A whole budget, not one per read: long enough for any track on a slow line, there so
            // that a connection gone silent still ends its thread.
            .timeout_recv_body(Some(WHOLE))
            .http_status_as_error(false)
            .build()
            .into();
        let mut answer = agent.get(address).call().map_err(|_| "the server could not be reached".to_owned())?;
        let status = answer.status().as_u16();
        if !(200..300).contains(&status) {
            let _ = fs::remove_file(&partial);
            return Err(format!("the server answered HTTP {status}"));
        }
        let total = answer
            .headers()
            .get("content-length")
            .and_then(|length| length.to_str().ok())
            .and_then(|length| length.parse::<u64>().ok());
        shared.lock().total = total;
        let fetching = Arc::clone(&shared);
        std::thread::spawn(move || {
            let body = answer.body_mut().as_reader();
            fetch(body, writer, &fetching, &partial, &whole);
        });
        Ok(Box::new(Growing { file: reader, at: 0, shared, patience: self.patience }))
    }

    /// Lets the oldest tracks go until the folder holds at most the cap. A track still being
    /// fetched is never among them, and nor is any file the cache did not name.
    fn trim(&self) {
        let Ok(entries) = fs::read_dir(&self.folder) else { return };
        let mut kept: Vec<(SystemTime, u64, PathBuf)> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let meta = entry.metadata().ok()?;
                // Only a track this cache fetched whole is ever let go: one still arriving ends in
                // `.part`, and a file of any other name is not the cache's to remove.
                let ours = path.file_name().and_then(|name| name.to_str()).is_some_and(named_by_cache);
                (meta.is_file() && ours).then(|| (meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len(), path))
            })
            .collect();
        kept.sort();
        let mut size: u64 = kept.iter().map(|(_, size, _)| size).sum();
        for (_, bytes, path) in kept {
            if size <= self.cap {
                break;
            }
            if fs::remove_file(&path).is_ok() {
                size -= bytes;
            }
        }
    }
}

/// The name a track is kept under: its key turned into letters a file name can hold.
fn name_of(key: &str) -> String {
    let digest = Md5::digest(key.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Whether `name` is one [`name_of`] gives: thirty-two lower-case hex digits and nothing else.
fn named_by_cache(name: &str) -> bool {
    name.len() == 32 && name.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// What the fetching and the reading share.
#[derive(Default)]
struct Shared {
    /// How far the fetch has got, and how it ended.
    state: Mutex<Progress>,
    /// Rung whenever the fetch moves on or ends.
    moved: Condvar,
    /// Set when the reader is gone, so the fetch stops.
    unwanted: AtomicBool,
}

impl Progress {
    /// Whether every byte the server said it would send is here.
    fn whole(&self) -> bool {
        self.total.is_some_and(|total| self.have >= total)
    }
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Progress> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// How far a fetch has got.
#[derive(Debug, Default)]
struct Progress {
    /// Bytes in the file so far.
    have: u64,
    /// The whole length, when the server said.
    total: Option<u64>,
    /// The fetch ended: `Ok` when every byte arrived, else why not.
    ended: Option<Result<(), String>>,
}

/// Copies `body` into `file` piece by piece, telling the reader as it goes, until the end, an
/// error, or the reader no longer wants it. A track fetched whole moves from `partial` to `whole`.
fn fetch(mut body: impl Read, mut file: File, shared: &Shared, partial: &Path, whole: &Path) {
    let mut piece = vec![0; PIECE];
    let ended = loop {
        // A track whose every byte is here is finished and kept even when the reader is gone:
        // a decoder that knows the length stops before the end of the body is read.
        if shared.unwanted.load(Ordering::Relaxed) && !shared.lock().whole() {
            break Err("no longer wanted".to_owned());
        }
        match body.read(&mut piece) {
            // A body shorter than the length the server gave comes as an error, not as an end.
            Ok(0) => break Ok(()),
            Ok(read) => {
                if file.write_all(&piece[..read]).is_err() {
                    break Err("the cache could not be written".to_owned());
                }
                shared.lock().have += read as u64;
                shared.moved.notify_all();
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break Err("the server stopped sending".to_owned()),
        }
    };
    let _ = file.flush();
    if ended.is_ok() {
        // Kept for the next time; a reader already open goes on reading the same file.
        let _ = fs::rename(partial, whole);
    } else {
        let _ = fs::remove_file(partial);
    }
    shared.lock().ended = Some(ended);
    shared.moved.notify_all();
}

/// A file still being fetched, read as if it were whole: a read waits for bytes that have not
/// arrived.
struct Growing {
    /// The file, opened for reading.
    file: File,
    /// Where the next read starts.
    at: u64,
    /// What the fetch says.
    shared: Arc<Shared>,
    /// How long a read waits.
    patience: Duration,
}

impl Read for Growing {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let deadline = Instant::now() + self.patience;
        let available = {
            let mut progress = self.shared.lock();
            loop {
                if progress.have > self.at {
                    break progress.have - self.at;
                }
                match &progress.ended {
                    Some(Ok(())) => return Ok(0),
                    Some(Err(why)) => return Err(io::Error::other(why.clone())),
                    None => {}
                }
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "the server sends nothing"));
                }
                progress = self
                    .shared
                    .moved
                    .wait_timeout(progress, left)
                    .map_or_else(|poisoned| poisoned.into_inner().0, |(guard, _)| guard);
            }
        };
        let wanted = buffer.len().min(usize::try_from(available).unwrap_or(usize::MAX));
        self.file.seek(SeekFrom::Start(self.at))?;
        let read = self.file.read(&mut buffer[..wanted])?;
        self.at += read as u64;
        Ok(read)
    }
}

impl Seek for Growing {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let total = self.shared.lock().total;
        let at = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::Current(by) => self.at.checked_add_signed(by),
            SeekFrom::End(by) => total.and_then(|total| total.checked_add_signed(by)),
        };
        // A place past what has arrived is fine: the next read waits for it.
        self.at = at.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no such place in the track"))?;
        Ok(self.at)
    }
}

impl MediaSource for Growing {
    fn is_seekable(&self) -> bool {
        // Without a length the decoder cannot find the end, so it is not offered a place to go.
        self.shared.lock().total.is_some()
    }

    fn byte_len(&self) -> Option<u64> {
        self.shared.lock().total
    }
}

impl Drop for Growing {
    fn drop(&mut self) {
        self.shared.unwanted.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests;
