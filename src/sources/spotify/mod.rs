//! Spotify Premium, through librespot: logging in with the person's browser, the liked songs and
//! the playlists through Spotify's Web API, and the sound through librespot.
//!
//! Spotify lets only Premium accounts play in another player, and it does not let new applications
//! stream at all, so qmus logs in the way Spotify's own players do, through librespot, an
//! unofficial client. Nothing of Spotify's is written to disk: the session lives while qmus is open.

pub mod audio;
pub mod login;
pub mod web;

use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub use web::Spotify;

use super::{Client, SourceError};

/// Makes the source of a session's sound from its access token.
type AudioMaker = dyn Fn(&str) -> Result<audio::Audio, SourceError> + Send + Sync;

/// Where qmus logs in to Spotify, which Web API it asks, and how the sound's session is made: the
/// real ones on the person's machine, stand-ins in a test.
#[derive(Clone)]
pub struct Setup {
    /// Spotify's accounts service.
    pub endpoints: login::Endpoints,
    /// The Web API.
    pub api: String,
    /// The id the login is made for.
    pub client: String,
    /// The port of the loopback address the browser comes back to.
    pub port: u16,
    /// Makes the source of the sound for a logged-in account.
    pub audio: Arc<AudioMaker>,
}

impl std::fmt::Debug for Setup {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Setup").field("api", &self.api).field("port", &self.port).finish_non_exhaustive()
    }
}

impl Setup {
    /// Spotify itself, through librespot.
    #[cfg(feature = "spotify")]
    #[must_use]
    pub fn here() -> Option<Self> {
        Some(Self {
            endpoints: login::Endpoints::default(),
            api: web::API.to_owned(),
            client: librespot_core::SessionConfig::default().client_id,
            port: 8898,
            audio: Arc::new(|access| {
                audio::Librespot::connect(access).map(|session| Arc::new(session) as audio::Audio)
            }),
        })
    }

    /// Nothing: this qmus was built without Spotify.
    #[cfg(not(feature = "spotify"))]
    #[must_use]
    pub fn here() -> Option<Self> {
        None
    }

    /// Listens where the browser comes back to.
    ///
    /// # Errors
    ///
    /// When the port is taken, as by another player logging in at the same moment.
    pub fn listen(&self) -> std::io::Result<TcpListener> {
        TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], self.port)))
    }

    /// Waits on `listener` for the browser to come back from the login of `pkce`, then logs in;
    /// `stop` ends the wait.
    ///
    /// # Errors
    ///
    /// As [`login::wait_for_code`] and [`Setup::finish`].
    pub fn log_in(&self, listener: &TcpListener, pkce: &login::Pkce, stop: &AtomicBool) -> Result<Client, SourceError> {
        let code = login::wait_for_code(listener, &pkce.state, login::PATIENCE, stop)?;
        self.finish(&code, &pkce.verifier)
    }

    /// Logs in with the `code` the login gave and its secret `verifier`: the tokens, the Premium
    /// check and the session of the sound.
    ///
    /// # Errors
    ///
    /// [`SourceError::Login`] for a code Spotify refuses, [`SourceError::Premium`] for an account
    /// that is not Premium, and why Spotify could not be reached.
    pub fn finish(&self, code: &str, verifier: &str) -> Result<Client, SourceError> {
        let tokens = login::exchange(&self.endpoints, &self.client, code, verifier)?;
        let access = tokens.access.clone();
        let spotify = Spotify::new(&self.api, self.endpoints.clone(), &self.client, tokens);
        spotify.ping()?;
        let audio = (self.audio)(&access)?;
        Ok(Client::Spotify(spotify.with_audio(audio)))
    }
}

#[cfg(test)]
mod tests;
