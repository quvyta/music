//! The person's accounts that qmus plays music from, beside the files of this computer.
//!
//! A track of an account is always played from that account: nothing here looks for a track of one
//! service on another by its name. Talking to a service never puts a password, a token or an
//! address holding one into an error or onto the screen.

use std::time::Duration;

/// How long a request may take before the service is taken to be unreachable. Generous, because a
/// home server on a small computer can be slow to list a large library.
const PATIENCE: Duration = Duration::from_secs(30);

/// How long the connection itself may take: a server that is off does not keep the screen waiting.
const CONNECT: Duration = Duration::from_secs(8);

pub mod cache;
pub mod jellyfin;
pub mod spotify;
pub mod subsonic;

/// An account logged in to, of whichever kind: what the rest of qmus asks things of.
#[derive(Debug, Clone)]
pub enum Client {
    /// A server that speaks the Subsonic API.
    Subsonic(subsonic::Subsonic),
    /// A Jellyfin server.
    Jellyfin(jellyfin::Jellyfin),
    /// A Spotify Premium account.
    Spotify(spotify::Spotify),
}

impl Client {
    /// Whether the service is there and takes the login.
    ///
    /// # Errors
    ///
    /// Why not: unreachable, the login turned away, or an answer that is not the service's.
    pub fn ping(&self) -> Result<(), SourceError> {
        match self {
            Self::Subsonic(server) => server.ping(),
            Self::Jellyfin(server) => server.ping(),
            Self::Spotify(account) => account.ping(),
        }
    }

    /// Every track the service has.
    ///
    /// # Errors
    ///
    /// As [`Client::ping`]; a page that fails fails the whole.
    pub fn catalogue(&self) -> Result<Vec<RemoteTrack>, SourceError> {
        match self {
            Self::Subsonic(server) => server.catalogue(),
            Self::Jellyfin(server) => server.catalogue(),
            Self::Spotify(account) => account.catalogue(),
        }
    }

    /// The address the track `id` is fetched from, with the login in it: for the player alone.
    /// `None` for Spotify, whose sound comes through librespot rather than from an address.
    #[must_use]
    pub fn stream_address(&self, id: &str) -> Option<String> {
        match self {
            Self::Subsonic(server) => Some(server.stream_address(id)),
            Self::Jellyfin(server) => Some(server.stream_address(id)),
            Self::Spotify(_) => None,
        }
    }

    /// How the sound of the track `id` is reached when it is not fetched from an address: Spotify's,
    /// through librespot.
    #[must_use]
    pub fn opener(&self, id: &str) -> Option<crate::audio::Opener> {
        match self {
            Self::Spotify(account) => account.opener(id),
            Self::Subsonic(_) | Self::Jellyfin(_) => None,
        }
    }

    /// The picture of the cover `id`, at most `size` pixels on its longer side.
    ///
    /// # Errors
    ///
    /// As [`Client::ping`].
    pub fn cover(&self, id: &str, size: u32) -> Result<Vec<u8>, SourceError> {
        match self {
            Self::Subsonic(server) => server.cover(id, size),
            Self::Jellyfin(server) => server.cover(id, size),
            // Spotify names a cover by the address of its picture, already of a size.
            Self::Spotify(account) => account.cover(id),
        }
    }

    /// The playlists the service keeps for the person.
    ///
    /// # Errors
    ///
    /// As [`Client::ping`].
    pub fn playlists(&self) -> Result<Vec<RemotePlaylist>, SourceError> {
        match self {
            Self::Subsonic(server) => server.playlists(),
            Self::Jellyfin(server) => server.playlists(),
            Self::Spotify(account) => account.playlists(),
        }
    }

    /// The tracks of the playlist `id`, in its order.
    ///
    /// # Errors
    ///
    /// As [`Client::ping`].
    pub fn playlist(&self, id: &str) -> Result<Vec<RemoteTrack>, SourceError> {
        match self {
            Self::Subsonic(server) => server.playlist(id),
            Self::Jellyfin(server) => server.playlist(id),
            Self::Spotify(account) => account.playlist(id),
        }
    }

    /// Tells the service the track `id` is being heard, or has been heard.
    ///
    /// # Errors
    ///
    /// As [`Client::ping`].
    pub fn scrobble(&self, id: &str, done: bool) -> Result<(), SourceError> {
        match self {
            Self::Subsonic(server) => server.scrobble(id, done),
            Self::Jellyfin(server) => server.scrobble(id, done),
            // Spotify's history takes no word from another player.
            Self::Spotify(_) => Ok(()),
        }
    }
}

/// What went wrong while talking to an account, in terms the screen can put into words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// The service could not be reached: no answer, refused, or a name that does not resolve.
    Unreachable(Reach),
    /// The service turned the login away.
    Login,
    /// The service does not have what was asked for.
    NotFound,
    /// The service answered with something qmus could not use, or said it failed: what it said.
    Server(String),
    /// The address given is not one a service can be reached at.
    Address,
    /// The account is not one the service lets play in another player, as a Spotify account that
    /// is not Premium.
    Premium,
}

/// Why a service could not be reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Nothing listens at the address, or the connection was refused.
    Refused,
    /// The name of the address does not resolve.
    NoSuchHost,
    /// The service did not answer in time.
    Timeout,
    /// The secure connection could not be set up, as with a certificate that does not match.
    Secure,
    /// The connection broke for another reason.
    Other,
}

/// A track as an account lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTrack {
    /// The id the service gives it, which is what it is played by.
    pub id: String,
    /// Its title.
    pub title: String,
    /// Its artist; empty when the service names none.
    pub artist: String,
    /// Its album; empty when the service names none.
    pub album: String,
    /// Its number on the album.
    pub number: Option<u32>,
    /// How long it plays, when the service says.
    pub duration: Option<Duration>,
    /// The id of its cover, when it has one.
    pub cover: Option<String>,
}

/// A playlist an account keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePlaylist {
    /// The id the service gives it.
    pub id: String,
    /// Its name.
    pub name: String,
    /// How many tracks it holds.
    pub tracks: usize,
}

/// Why a request did not get through, from ureq's error.
pub(crate) fn reach_of(error: &ureq::Error) -> Reach {
    match error {
        ureq::Error::HostNotFound => Reach::NoSuchHost,
        ureq::Error::ConnectionFailed => Reach::Refused,
        ureq::Error::Timeout(_) => Reach::Timeout,
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) | ureq::Error::TlsRequired => Reach::Secure,
        ureq::Error::Io(error) => match error.kind() {
            std::io::ErrorKind::ConnectionRefused => Reach::Refused,
            std::io::ErrorKind::TimedOut => Reach::Timeout,
            _ => Reach::Other,
        },
        _ => Reach::Other,
    }
}

/// `address` without a slash at its end, when it is an `http://` or `https://` address with a host.
pub(crate) fn base_of(address: &str) -> Result<String, SourceError> {
    let base = address.trim().trim_end_matches('/').to_owned();
    let host = base.strip_prefix("https://").or_else(|| base.strip_prefix("http://"));
    if host.is_none_or(|host| host.is_empty() || host.starts_with('/')) {
        return Err(SourceError::Address);
    }
    Ok(base)
}

/// The connections to a service: bounded in time, and an answer's status read rather than taken
/// as an error.
pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(PATIENCE))
        .timeout_connect(Some(CONNECT))
        .http_status_as_error(false)
        .build()
        .into()
}
