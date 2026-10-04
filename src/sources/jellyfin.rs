//! A Jellyfin server of the person's own, through its own API.
//!
//! The password is sent once, to log in; the server answers with a session token, and every request
//! after that carries the token instead. The token and the server's id of the person are held only
//! while qmus is open. The address a track is fetched from carries the token, so such an address is
//! never shown, written into an error, or handed to the desktop.

use std::io::Read;
use std::time::Duration;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Value, json};

use super::{RemotePlaylist, RemoteTrack, SourceError, reach_of};

/// The name qmus gives itself to the server, which shows it beside the sessions it lists.
const CLIENT: &str = "qmus";

/// How many items one request of the catalogue asks for.
const PAGE: usize = 500;

/// The most a single answer may weigh.
const ANSWER_LIMIT: u64 = 64 * 1024 * 1024;

/// A run time is counted in ticks of a hundred nanoseconds.
const TICKS_PER_SECOND: u64 = 10_000_000;

/// The fields of the catalogue's items qmus reads beyond those every item has.
const FIELDS: &str = "Artists,AlbumArtist,Album,IndexNumber,RunTimeTicks,AlbumId,AlbumPrimaryImageTag";

/// The characters left as they are in a field of a query: letters, digits and `-._~`.
const FIELD: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');

/// A server, and the person logged in to it.
#[derive(Clone)]
pub struct Jellyfin {
    /// The server's address, without a slash at its end.
    base: String,
    /// The name this copy of qmus goes by on the server: the account's key, so the server sees
    /// one device for one account, run after run.
    device: String,
    /// The session token the server gave.
    token: String,
    /// The server's id of the person.
    user: String,
    /// The connections to the server.
    agent: ureq::Agent,
}

impl std::fmt::Debug for Jellyfin {
    // A token printed in a test failure or a log would be a login given away.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Jellyfin").field("base", &self.base).field("user", &self.user).finish_non_exhaustive()
    }
}

impl Jellyfin {
    /// Logs in to the server at `address` as `user` with `password`, as the device `device`.
    ///
    /// # Errors
    ///
    /// [`SourceError::Address`] for an address that is not one, [`SourceError::Login`] when the
    /// server turns the login away, and why the server could not be reached or answered.
    pub fn login(address: &str, user: &str, password: &str, device: &str) -> Result<Self, SourceError> {
        let base = super::base_of(address)?;
        let mut server =
            Self { base, device: device.to_owned(), token: String::new(), user: String::new(), agent: super::agent() };
        let answer = server.send("/Users/AuthenticateByName", &json!({ "Username": user.trim(), "Pw": password }))?;
        let token = answer.get("AccessToken").and_then(Value::as_str).filter(|token| !token.is_empty());
        let id = answer.get("User").and_then(|user| user.get("Id")).and_then(Value::as_str);
        let (Some(token), Some(id)) = (token, id) else { return Err(not_jellyfin()) };
        server.token = token.to_owned();
        server.user = id.to_owned();
        Ok(server)
    }

    /// Whether the server is there and still takes the session.
    ///
    /// # Errors
    ///
    /// As [`Jellyfin::login`].
    pub fn ping(&self) -> Result<(), SourceError> {
        self.call(&format!("/Users/{}", self.user), &[]).map(|_| ())
    }

    /// Every track the server has, read a page at a time.
    ///
    /// # Errors
    ///
    /// As [`Jellyfin::login`]; a page that fails fails the whole.
    pub fn catalogue(&self) -> Result<Vec<RemoteTrack>, SourceError> {
        let mut all = Vec::new();
        loop {
            let start = all.len().to_string();
            let limit = PAGE.to_string();
            let answer = self.call(
                &format!("/Users/{}/Items", self.user),
                &[
                    ("IncludeItemTypes", "Audio"),
                    ("Recursive", "true"),
                    ("Fields", FIELDS),
                    ("StartIndex", &start),
                    ("Limit", &limit),
                ],
            )?;
            let page: Vec<RemoteTrack> = items(&answer).filter_map(track_of).collect();
            let full = items(&answer).count() == PAGE;
            all.extend(page);
            if !full {
                return Ok(all);
            }
        }
    }

    /// The address the track `id` is played from, with the session in it: for the player alone,
    /// never to be shown or handed on.
    #[must_use]
    pub fn stream_address(&self, id: &str) -> String {
        self.address(&format!("/Audio/{}/stream", encoded(id)), &[("static", "true"), ("api_key", &self.token)])
    }

    /// The picture of the cover `id`, at most `size` pixels wide.
    ///
    /// # Errors
    ///
    /// As [`Jellyfin::login`]; [`SourceError::NotFound`] when the server has no such picture.
    pub fn cover(&self, id: &str, size: u32) -> Result<Vec<u8>, SourceError> {
        let size = size.to_string();
        let address = self.address(&format!("/Items/{}/Images/Primary", encoded(id)), &[("maxWidth", &size)]);
        let mut answer = self.get(&address)?;
        check(answer.status().as_u16())?;
        let mut bytes = Vec::new();
        answer
            .body_mut()
            .as_reader()
            .take(ANSWER_LIMIT)
            .read_to_end(&mut bytes)
            .map_err(|_| SourceError::Unreachable(super::Reach::Other))?;
        Ok(bytes)
    }

    /// The playlists the server keeps for the person.
    ///
    /// # Errors
    ///
    /// As [`Jellyfin::login`].
    pub fn playlists(&self) -> Result<Vec<RemotePlaylist>, SourceError> {
        let answer = self.call(
            &format!("/Users/{}/Items", self.user),
            &[("IncludeItemTypes", "Playlist"), ("Recursive", "true"), ("Fields", "ChildCount")],
        )?;
        Ok(items(&answer)
            .filter_map(|list| {
                Some(RemotePlaylist {
                    id: list.get("Id")?.as_str()?.to_owned(),
                    name: list.get("Name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    tracks: list
                        .get("ChildCount")
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
    /// As [`Jellyfin::login`]; [`SourceError::NotFound`] when there is no such playlist.
    pub fn playlist(&self, id: &str) -> Result<Vec<RemoteTrack>, SourceError> {
        let answer =
            self.call(&format!("/Playlists/{}/Items", encoded(id)), &[("userId", &self.user), ("Fields", FIELDS)])?;
        Ok(items(&answer).filter_map(track_of).collect())
    }

    /// Tells the server the track `id` is being heard (`done` false) or has been heard (`done`
    /// true), for its own play counts.
    ///
    /// # Errors
    ///
    /// As [`Jellyfin::login`].
    pub fn scrobble(&self, id: &str, done: bool) -> Result<(), SourceError> {
        if done {
            self.send(&format!("/Users/{}/PlayedItems/{}", self.user, encoded(id)), &json!({})).map(|_| ())
        } else {
            self.send("/Sessions/Playing", &json!({ "ItemId": id })).map(|_| ())
        }
    }

    /// The answer of a `GET` of `path` with `fields`, as JSON.
    fn call(&self, path: &str, fields: &[(&str, &str)]) -> Result<Value, SourceError> {
        let mut answer = self.get(&self.address(path, fields))?;
        let status = answer.status().as_u16();
        let text = read(answer.body_mut())?;
        check(status)?;
        serde_json::from_str(&text).map_err(|_| not_jellyfin())
    }

    /// The answer of a `POST` of `body` to `path`, as JSON; an empty answer is an empty object.
    fn send(&self, path: &str, body: &Value) -> Result<Value, SourceError> {
        let mut answer = self
            .agent
            .post(&self.address(path, &[]))
            .header("Authorization", &self.authorization())
            .header("Content-Type", "application/json")
            .send(body.to_string())
            .map_err(|error| SourceError::Unreachable(reach_of(&error)))?;
        let status = answer.status().as_u16();
        let text = read(answer.body_mut())?;
        check(status)?;
        if text.trim().is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_str(&text).map_err(|_| not_jellyfin())
    }

    /// Sends a `GET` to `address` with the session.
    fn get(&self, address: &str) -> Result<ureq::http::Response<ureq::Body>, SourceError> {
        self.agent
            .get(address)
            .header("Authorization", &self.authorization())
            .call()
            .map_err(|error| SourceError::Unreachable(reach_of(&error)))
    }

    /// The header Jellyfin knows a client by, with the session once there is one.
    fn authorization(&self) -> String {
        let mut header = format!(
            r#"MediaBrowser Client="{CLIENT}", Device="{CLIENT}", DeviceId="{}", Version="{}""#,
            self.device,
            env!("CARGO_PKG_VERSION")
        );
        if !self.token.is_empty() {
            header.push_str(&format!(r#", Token="{}""#, self.token));
        }
        header
    }

    /// The address of `path` with `fields`.
    fn address(&self, path: &str, fields: &[(&str, &str)]) -> String {
        if fields.is_empty() {
            return format!("{}{path}", self.base);
        }
        let query: Vec<String> =
            fields.iter().map(|(name, value)| format!("{name}={}", utf8_percent_encode(value, FIELD))).collect();
        format!("{}{path}?{}", self.base, query.join("&"))
    }
}

/// What an answer's status says, when it is not a success.
fn check(status: u16) -> Result<(), SourceError> {
    match status {
        200..=299 => Ok(()),
        401 | 403 => Err(SourceError::Login),
        404 => Err(SourceError::NotFound),
        _ => Err(SourceError::Server(format!("HTTP {status}"))),
    }
}

/// The whole text of an answer, within the limit.
fn read(body: &mut ureq::Body) -> Result<String, SourceError> {
    body.with_config().limit(ANSWER_LIMIT).read_to_string().map_err(|error| match error {
        ureq::Error::BodyExceedsLimit(_) => SourceError::Server("answer too large".to_owned()),
        other => SourceError::Unreachable(reach_of(&other)),
    })
}

/// What an answer that is not Jellyfin's is said to be.
fn not_jellyfin() -> SourceError {
    SourceError::Server("not a Jellyfin server".to_owned())
}

/// `id` as one step of a path.
fn encoded(id: &str) -> String {
    utf8_percent_encode(id, FIELD).to_string()
}

/// The items of a list answer.
fn items(answer: &Value) -> impl Iterator<Item = &Value> {
    answer.get("Items").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default().iter()
}

/// The track an item describes, or `None` when it has no id to play it by.
fn track_of(item: &Value) -> Option<RemoteTrack> {
    let text = |name: &str| item.get(name).and_then(Value::as_str).map(str::trim).unwrap_or_default().to_owned();
    let artists: Vec<&str> =
        item.get("Artists").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    let artist = if artists.is_empty() { text("AlbumArtist") } else { artists.join(", ") };
    let ticks = item.get("RunTimeTicks").and_then(Value::as_u64).filter(|ticks| *ticks > 0);
    // The album's picture is the cover; a track with a picture of its own and no album's keeps its own.
    let cover = match (item.get("AlbumId").and_then(Value::as_str), item.get("AlbumPrimaryImageTag")) {
        (Some(album), Some(_)) => Some(album.to_owned()),
        _ => {
            item.get("ImageTags").and_then(|tags| tags.get("Primary")).and(item.get("Id")?.as_str().map(str::to_owned))
        }
    };
    Some(RemoteTrack {
        id: item.get("Id")?.as_str()?.to_owned(),
        title: text("Name"),
        artist,
        album: text("Album"),
        number: item.get("IndexNumber").and_then(Value::as_u64).and_then(|number| u32::try_from(number).ok()),
        duration: ticks.map(|ticks| Duration::from_secs(ticks / TICKS_PER_SECOND)).filter(|length| !length.is_zero()),
        cover,
    })
}

#[cfg(test)]
mod tests;
