//! A music server of the person's own that speaks the Subsonic API: Navidrome, Gonic,
//! Airsonic-Advanced and the like.
//!
//! The login goes along with every request. With an API key (the OpenSubsonic extension) only the
//! key is sent. With a password, the password itself never leaves the machine: each request carries
//! a fresh salt and the MD5 of the password followed by it, which is what the API asks for. Either
//! way the address of a request holds the login, so such an address is never shown, written into an
//! error, or handed to the desktop.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use md5::{Digest, Md5};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::Value;

use super::{Reach, RemotePlaylist, RemoteTrack, SourceError, reach_of};

/// The version of the API qmus speaks: the one the OpenSubsonic servers all answer to.
const VERSION: &str = "1.16.1";

/// The name qmus gives itself to the server, which some show beside what is being played.
const CLIENT: &str = "qmus";

/// How many tracks one request of the catalogue asks for: few enough for a quick answer, many
/// enough that a library of fifty thousand tracks is read in a hundred requests.
const PAGE: usize = 500;

/// The most a single answer of the API may weigh; a list of every playlist stays far below it.
const ANSWER_LIMIT: u64 = 64 * 1024 * 1024;

/// The characters left as they are in a field of a query: letters, digits and `-._~`.
const FIELD: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');

/// How the server is logged in to.
#[derive(Clone, PartialEq, Eq)]
pub enum Auth {
    /// An API key the server gave (OpenSubsonic `apiKey`).
    ApiKey(String),
    /// The person's password.
    Password(String),
}

impl std::fmt::Debug for Auth {
    // A login printed in a test failure or a log would be a login given away.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApiKey(_) => out.write_str("ApiKey(..)"),
            Self::Password(_) => out.write_str("Password(..)"),
        }
    }
}

/// A server, and the person logged in to it.
#[derive(Clone)]
pub struct Subsonic {
    /// The server's address, without a slash at its end.
    base: String,
    /// The person's name on the server.
    user: String,
    /// How the person logs in.
    auth: Auth,
    /// The connections to the server.
    agent: ureq::Agent,
}

impl std::fmt::Debug for Subsonic {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Subsonic")
            .field("base", &self.base)
            .field("user", &self.user)
            .field("auth", &self.auth)
            .finish()
    }
}

impl Subsonic {
    /// The server at `address`, logged in to as `user` with `auth`.
    ///
    /// # Errors
    ///
    /// [`SourceError::Address`] when `address` is not an `http://` or `https://` address with a
    /// host in it.
    pub fn new(address: &str, user: &str, auth: Auth) -> Result<Self, SourceError> {
        let base = super::base_of(address)?;
        Ok(Self { base, user: user.trim().to_owned(), auth, agent: super::agent() })
    }

    /// Whether the server is there and takes the login.
    ///
    /// # Errors
    ///
    /// Why not: unreachable, the login turned away, or an answer that is not the API's.
    pub fn ping(&self) -> Result<(), SourceError> {
        self.call("ping", &[]).map(|_| ())
    }

    /// One page of every track the server has: `count` of them from `offset` on.
    ///
    /// # Errors
    ///
    /// As [`Subsonic::ping`].
    pub fn songs(&self, offset: usize, count: usize) -> Result<Vec<RemoteTrack>, SourceError> {
        let (offset, count) = (offset.to_string(), count.to_string());
        // An empty query is how the API is asked for everything, page by page.
        let answer = self.call(
            "search3",
            &[("query", ""), ("artistCount", "0"), ("albumCount", "0"), ("songCount", &count), ("songOffset", &offset)],
        )?;
        Ok(tracks_of(answer.get("searchResult3").and_then(|result| result.get("song"))))
    }

    /// Every track the server has, read a page at a time.
    ///
    /// # Errors
    ///
    /// As [`Subsonic::ping`]; a page that fails fails the whole, so that a library is never shown
    /// with a hole in it.
    pub fn catalogue(&self) -> Result<Vec<RemoteTrack>, SourceError> {
        let mut all = Vec::new();
        loop {
            let page = self.songs(all.len(), PAGE)?;
            let full = page.len() == PAGE;
            all.extend(page);
            if !full {
                return Ok(all);
            }
        }
    }

    /// The tracks the server finds for `query`.
    ///
    /// # Errors
    ///
    /// As [`Subsonic::ping`].
    pub fn search(&self, query: &str) -> Result<Vec<RemoteTrack>, SourceError> {
        let answer =
            self.call("search3", &[("query", query), ("artistCount", "0"), ("albumCount", "0"), ("songCount", "100")])?;
        Ok(tracks_of(answer.get("searchResult3").and_then(|result| result.get("song"))))
    }

    /// The address the track `id` is played from, with the login in it: for the player alone,
    /// never to be shown or handed on.
    #[must_use]
    pub fn stream_address(&self, id: &str) -> String {
        self.address("stream", &[("id", id)])
    }

    /// The picture of the cover `id`, at most `size` pixels on its longer side.
    ///
    /// # Errors
    ///
    /// As [`Subsonic::ping`]; [`SourceError::NotFound`] when the server has no such cover.
    pub fn cover(&self, id: &str, size: u32) -> Result<Vec<u8>, SourceError> {
        let size = size.to_string();
        let mut answer = self.get(&self.address("getCoverArt", &[("id", id), ("size", &size)]))?;
        let picture = answer
            .headers()
            .get("content-type")
            .and_then(|kind| kind.to_str().ok())
            .is_some_and(|kind| kind.starts_with("image/"));
        if !picture {
            // A failed request answers with the API's error rather than a picture.
            let text = read(answer.body_mut())?;
            return Err(failure_of(&text).unwrap_or(SourceError::NotFound));
        }
        let mut bytes = Vec::new();
        answer
            .body_mut()
            .as_reader()
            .take(ANSWER_LIMIT)
            .read_to_end(&mut bytes)
            .map_err(|_| SourceError::Unreachable(Reach::Other))?;
        Ok(bytes)
    }

    /// The playlists the server keeps for the person.
    ///
    /// # Errors
    ///
    /// As [`Subsonic::ping`].
    pub fn playlists(&self) -> Result<Vec<RemotePlaylist>, SourceError> {
        let answer = self.call("getPlaylists", &[])?;
        let list = answer.get("playlists").and_then(|lists| lists.get("playlist"));
        Ok(items(list)
            .filter_map(|list| {
                Some(RemotePlaylist {
                    id: text(list.get("id")?)?,
                    name: list.get("name").and_then(text).unwrap_or_default(),
                    tracks: list
                        .get("songCount")
                        .and_then(Value::as_u64)
                        .map_or(0, |count| usize::try_from(count).unwrap_or(usize::MAX)),
                })
            })
            .collect())
    }

    /// The tracks of the playlist `id`, in its order.
    ///
    /// # Errors
    ///
    /// As [`Subsonic::ping`]; [`SourceError::NotFound`] when there is no such playlist.
    pub fn playlist(&self, id: &str) -> Result<Vec<RemoteTrack>, SourceError> {
        let answer = self.call("getPlaylist", &[("id", id)])?;
        Ok(tracks_of(answer.get("playlist").and_then(|list| list.get("entry"))))
    }

    /// Tells the server the track `id` is being heard (`done` false) or has been heard (`done`
    /// true), for the server's own play counts.
    ///
    /// # Errors
    ///
    /// As [`Subsonic::ping`].
    pub fn scrobble(&self, id: &str, done: bool) -> Result<(), SourceError> {
        self.call("scrobble", &[("id", id), ("submission", if done { "true" } else { "false" })]).map(|_| ())
    }

    /// The answer of the API's `method` to `fields`, inside its `subsonic-response`.
    fn call(&self, method: &str, fields: &[(&str, &str)]) -> Result<Value, SourceError> {
        let mut answer = self.get(&self.address(method, fields))?;
        let status = answer.status().as_u16();
        let text = read(answer.body_mut())?;
        if let Some(failure) = failure_of(&text) {
            return Err(failure);
        }
        match status {
            401 | 403 => return Err(SourceError::Login),
            404 => return Err(SourceError::NotFound),
            200..=299 => {}
            _ => return Err(SourceError::Server(format!("HTTP {status}"))),
        }
        let value: Value = serde_json::from_str(&text).map_err(|_| not_the_api())?;
        let body = value.get("subsonic-response").ok_or_else(not_the_api)?;
        if body.get("status").and_then(Value::as_str) != Some("ok") {
            return Err(not_the_api());
        }
        Ok(body.clone())
    }

    /// Sends a request to `address`.
    fn get(&self, address: &str) -> Result<ureq::http::Response<ureq::Body>, SourceError> {
        self.agent.get(address).call().map_err(|error| SourceError::Unreachable(reach_of(&error)))
    }

    /// The address of the API's `method` with `fields`, the login and the API's own fields.
    fn address(&self, method: &str, fields: &[(&str, &str)]) -> String {
        let mut query: Vec<(&str, String)> = Vec::with_capacity(fields.len() + 6);
        match &self.auth {
            // The API refuses a name beside a key: the key already says who it is.
            Auth::ApiKey(key) => query.push(("apiKey", key.clone())),
            Auth::Password(password) => {
                let salt = salt();
                query.push(("u", self.user.clone()));
                query.push(("t", token(password, &salt)));
                query.push(("s", salt));
            }
        }
        query.push(("v", VERSION.to_owned()));
        query.push(("c", CLIENT.to_owned()));
        query.push(("f", "json".to_owned()));
        query.extend(fields.iter().map(|(name, value)| (*name, (*value).to_owned())));
        let query: Vec<String> =
            query.iter().map(|(name, value)| format!("{name}={}", utf8_percent_encode(value, FIELD))).collect();
        format!("{}/rest/{method}?{}", self.base, query.join("&"))
    }
}

/// The token the API asks for in place of the password: the MD5 of the password followed by
/// `salt`, in lower-case hex.
fn token(password: &str, salt: &str) -> String {
    let mut digest = Md5::new();
    digest.update(password.as_bytes());
    digest.update(salt.as_bytes());
    digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A salt never used before in this run: the API wants a new one with every request, so that a
/// request overheard cannot be sent again as it is.
fn salt() -> String {
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(COUNT.fetch_add(1, Ordering::Relaxed));
    hasher.write_u128(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_nanos()),
    );
    format!("{:016x}", hasher.finish())
}

/// The whole text of an answer, within the limit.
fn read(body: &mut ureq::Body) -> Result<String, SourceError> {
    body.with_config().limit(ANSWER_LIMIT).read_to_string().map_err(|error| match error {
        ureq::Error::BodyExceedsLimit(_) => SourceError::Server("answer too large".to_owned()),
        other => SourceError::Unreachable(reach_of(&other)),
    })
}

/// The failure the API's answer `text` reports, when it reports one. Servers answer a failed call
/// with status 200 or with an HTTP error, and the API's error code says more than either.
fn failure_of(text: &str) -> Option<SourceError> {
    let value: Value = serde_json::from_str(text).ok()?;
    let body = value.get("subsonic-response")?;
    if body.get("status").and_then(Value::as_str) != Some("failed") {
        return None;
    }
    let error = body.get("error");
    let code = error.and_then(|error| error.get("code")).and_then(Value::as_u64).unwrap_or(0);
    let message = error.and_then(|error| error.get("message")).and_then(Value::as_str).unwrap_or_default();
    Some(match code {
        // A wrong name or password, a login the server cannot check by token, a wrong key, and
        // a person who may not do this: all of them ask the person for a different login.
        40 | 41 | 42 | 43 | 44 | 50 => SourceError::Login,
        70 => SourceError::NotFound,
        _ => SourceError::Server(message.to_owned()),
    })
}

/// What an answer that is not the API's is said to be.
fn not_the_api() -> SourceError {
    SourceError::Server("not a Subsonic server".to_owned())
}

/// The tracks a list of the API holds. The API gives a single item as an object and many as an
/// array, so both are read.
fn tracks_of(list: Option<&Value>) -> Vec<RemoteTrack> {
    items(list).filter_map(track_of).collect()
}

/// The items of `list`: an array's elements, or the one object it is.
fn items(list: Option<&Value>) -> impl Iterator<Item = &Value> {
    let many: &[Value] = match list {
        Some(Value::Array(items)) => items,
        Some(one @ Value::Object(_)) => std::slice::from_ref(one),
        _ => &[],
    };
    many.iter()
}

/// The track an item of the API describes, or `None` when it has no id to play it by.
fn track_of(item: &Value) -> Option<RemoteTrack> {
    if item.get("isDir").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    Some(RemoteTrack {
        id: text(item.get("id")?)?,
        title: item.get("title").and_then(text).unwrap_or_default(),
        artist: item.get("artist").and_then(text).unwrap_or_default(),
        album: item.get("album").and_then(text).unwrap_or_default(),
        number: item.get("track").and_then(Value::as_u64).and_then(|number| u32::try_from(number).ok()),
        duration: item.get("duration").and_then(Value::as_u64).filter(|seconds| *seconds > 0).map(Duration::from_secs),
        cover: item.get("coverArt").and_then(text),
    })
}

/// A field as text: servers write ids as strings, and a few as numbers.
fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.trim().to_owned()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
