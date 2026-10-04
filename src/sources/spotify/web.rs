//! Spotify's Web API, for what the person keeps there: the liked songs, the playlists and the
//! covers. The sound itself comes through librespot.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::Value;

use super::login::{self, Endpoints, Tokens};
use crate::sources::{RemotePlaylist, RemoteTrack, SourceError, reach_of};

/// Where the Web API is.
pub const API: &str = "https://api.spotify.com/v1";

/// How many liked songs one request asks for: the most the API gives.
const SONGS_PAGE: usize = 50;

/// How many tracks of a playlist one request asks for: the most the API gives.
const LIST_PAGE: usize = 100;

/// How long before it runs out an access token is renewed, so no request goes with one that dies
/// on the way.
const EARLY: Duration = Duration::from_secs(60);

/// The most a single answer may weigh.
const ANSWER_LIMIT: u64 = 64 * 1024 * 1024;

/// The characters left as they are in a step of a path.
const FIELD: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');

/// The account logged in to, and its tokens, renewed as they run out.
#[derive(Clone)]
pub struct Spotify {
    /// Where the Web API is.
    api: String,
    /// Where the tokens are renewed.
    endpoints: Endpoints,
    /// The id the login was made for.
    client: String,
    /// The tokens, shared by every copy, so one renewal serves them all.
    tokens: Arc<Mutex<Tokens>>,
    /// The connections to the API.
    agent: ureq::Agent,
    /// What gives the sound of the tracks, once there is a session for it.
    audio: Option<super::audio::Audio>,
}

impl std::fmt::Debug for Spotify {
    // The tokens are left out: printed, they would be the login given away.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Spotify").field("api", &self.api).finish_non_exhaustive()
    }
}

impl Spotify {
    /// The account logged in to with `tokens`, made for `client`, asking the API at `api` and
    /// renewing at `endpoints`.
    #[must_use]
    pub fn new(api: &str, endpoints: Endpoints, client: &str, tokens: Tokens) -> Self {
        Self {
            api: api.trim_end_matches('/').to_owned(),
            endpoints,
            client: client.to_owned(),
            tokens: Arc::new(Mutex::new(tokens)),
            agent: crate::sources::agent(),
            audio: None,
        }
    }

    /// The same account, its tracks' sound given by `audio`.
    #[must_use]
    pub fn with_audio(mut self, audio: super::audio::Audio) -> Self {
        self.audio = Some(audio);
        self
    }

    /// How the sound of the track `id` is reached; `None` while there is no session for it.
    #[must_use]
    pub fn opener(&self, id: &str) -> Option<crate::audio::Opener> {
        let audio = self.audio.clone()?;
        Some(crate::audio::Opener::Spotify { audio, id: id.to_owned() })
    }

    /// The access token now, renewed first when it is about to run out.
    ///
    /// # Errors
    ///
    /// When it ran out and Spotify would not renew it: the person logs in again.
    pub fn access(&self) -> Result<String, SourceError> {
        let mut tokens = self.tokens.lock().unwrap_or_else(PoisonError::into_inner);
        if tokens.expires <= Instant::now() + EARLY {
            *tokens = login::refresh(&self.endpoints, &self.client, &tokens.refresh)?;
        }
        Ok(tokens.access.clone())
    }

    /// Whether the account may play here: [`SourceError::Premium`] for one that is not Premium,
    /// which Spotify does not let play in another player.
    ///
    /// # Errors
    ///
    /// As said, and why Spotify could not be reached.
    pub fn ping(&self) -> Result<(), SourceError> {
        let me = self.call(&format!("{}/me", self.api))?;
        match me.get("product").and_then(Value::as_str) {
            Some("premium") => Ok(()),
            _ => Err(SourceError::Premium),
        }
    }

    /// The person's liked songs, newest first.
    ///
    /// # Errors
    ///
    /// As [`Spotify::ping`]; a page that fails fails the whole.
    pub fn catalogue(&self) -> Result<Vec<RemoteTrack>, SourceError> {
        self.pages(&format!("{}/me/tracks", self.api), SONGS_PAGE, |item| item.get("track").and_then(track_of))
    }

    /// The playlists the person keeps or follows.
    ///
    /// # Errors
    ///
    /// As [`Spotify::ping`].
    pub fn playlists(&self) -> Result<Vec<RemotePlaylist>, SourceError> {
        self.pages(&format!("{}/me/playlists", self.api), SONGS_PAGE, |list| {
            Some(RemotePlaylist {
                id: list.get("id")?.as_str()?.to_owned(),
                name: list.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                tracks: list
                    .get("tracks")
                    .and_then(|tracks| tracks.get("total"))
                    .and_then(Value::as_u64)
                    .map_or(0, |count| usize::try_from(count).unwrap_or(usize::MAX)),
            })
        })
    }

    /// The tracks of the playlist `id`, in its order.
    ///
    /// # Errors
    ///
    /// As [`Spotify::ping`]; [`SourceError::NotFound`] when there is no such playlist.
    pub fn playlist(&self, id: &str) -> Result<Vec<RemoteTrack>, SourceError> {
        let address = format!("{}/playlists/{}/tracks", self.api, utf8_percent_encode(id, FIELD));
        self.pages(&address, LIST_PAGE, |item| item.get("track").and_then(track_of))
    }

    /// The picture at `address`, an album cover's: Spotify's pictures are public.
    ///
    /// # Errors
    ///
    /// Why it could not be fetched.
    pub fn cover(&self, address: &str) -> Result<Vec<u8>, SourceError> {
        let mut answer = self.agent.get(address).call().map_err(|error| SourceError::Unreachable(reach_of(&error)))?;
        if !answer.status().is_success() {
            return Err(SourceError::NotFound);
        }
        answer
            .body_mut()
            .with_config()
            .limit(ANSWER_LIMIT)
            .read_to_vec()
            .map_err(|error| SourceError::Unreachable(reach_of(&error)))
    }

    /// Every item of the list at `address`, `page` at a time, as `item` reads them; the items it
    /// cannot read, such as a track kept only on the person's own device, are passed over.
    fn pages<T>(&self, address: &str, page: usize, item: impl Fn(&Value) -> Option<T>) -> Result<Vec<T>, SourceError> {
        let mut all = Vec::new();
        let mut offset = 0;
        loop {
            let answer = self.call(&format!("{address}?limit={page}&offset={offset}"))?;
            let items = answer.get("items").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
            all.extend(items.iter().filter_map(&item));
            offset += items.len();
            let more = answer.get("next").is_some_and(|next| !next.is_null());
            if !more || items.is_empty() {
                return Ok(all);
            }
        }
    }

    /// The answer of a `GET` of `address`, as JSON; a token Spotify says has run out is renewed
    /// once and the request sent again.
    fn call(&self, address: &str) -> Result<Value, SourceError> {
        for again in [false, true] {
            if again {
                self.tokens.lock().unwrap_or_else(PoisonError::into_inner).expires = Instant::now();
            }
            let token = self.access()?;
            let mut answer = self
                .agent
                .get(address)
                .header("Authorization", &format!("Bearer {token}"))
                .call()
                .map_err(|error| SourceError::Unreachable(reach_of(&error)))?;
            let status = answer.status().as_u16();
            let text = answer
                .body_mut()
                .with_config()
                .limit(ANSWER_LIMIT)
                .read_to_string()
                .map_err(|error| SourceError::Unreachable(reach_of(&error)))?;
            match status {
                401 if !again => continue,
                200..=299 => return serde_json::from_str(&text).map_err(|_| SourceError::Server("not Spotify".into())),
                401 | 403 => return Err(SourceError::Login),
                404 => return Err(SourceError::NotFound),
                _ => return Err(SourceError::Server(format!("HTTP {status}"))),
            }
        }
        Err(SourceError::Login)
    }
}

/// The track a Web API track object describes; `None` for one without an id, such as a file kept
/// only on the person's own device.
fn track_of(track: &Value) -> Option<RemoteTrack> {
    if track.get("is_local").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let text = |value: Option<&Value>| value.and_then(Value::as_str).map(str::trim).unwrap_or_default().to_owned();
    let artists: Vec<String> = track
        .get("artists")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|artist| text(artist.get("name")))
        .filter(|name| !name.is_empty())
        .collect();
    let album = track.get("album");
    // The pictures come largest first; the first is the cover.
    let cover = album
        .and_then(|album| album.get("images"))
        .and_then(Value::as_array)
        .and_then(|images| images.first())
        .and_then(|image| image.get("url"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    Some(RemoteTrack {
        id: track.get("id")?.as_str()?.to_owned(),
        title: text(track.get("name")),
        artist: artists.join(", "),
        album: text(album.and_then(|album| album.get("name"))),
        number: track.get("track_number").and_then(Value::as_u64).and_then(|number| u32::try_from(number).ok()),
        duration: track.get("duration_ms").and_then(Value::as_u64).filter(|ms| *ms > 0).map(Duration::from_millis),
        cover,
    })
}

#[cfg(test)]
mod tests;
