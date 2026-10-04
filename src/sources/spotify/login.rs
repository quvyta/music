//! Logging in to Spotify with the person's browser: the authorization code flow with PKCE.
//!
//! qmus makes a secret, shows Spotify's login page in the browser with only a hash of the secret
//! in its address, and waits on this computer's loopback address for the browser to come back with
//! a code. The code and the secret together buy the tokens. Neither the code, the secret nor the
//! tokens are ever written anywhere.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::sources::{Reach, SourceError, reach_of};

/// Where Spotify's own players are sent back to after logging in: the only address Spotify
/// returns their logins to.
pub const REDIRECT: &str = "http://127.0.0.1:8898/login";

/// What qmus asks the login for: to play, to read the person's name and country, the liked songs
/// and the playlists.
pub const SCOPES: &str =
    "streaming user-read-private user-library-read playlist-read-private playlist-read-collaborative";

/// How long qmus waits for the browser to come back.
pub const PATIENCE: Duration = Duration::from_secs(5 * 60);

/// The characters left as they are in a field of a query or a form: letters, digits and `-._~`.
const FIELD: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');

/// Where Spotify's accounts service is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    /// The login page.
    pub authorize: String,
    /// Where a code or a refresh token is exchanged for tokens.
    pub token: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            authorize: "https://accounts.spotify.com/authorize".to_owned(),
            token: "https://accounts.spotify.com/api/token".to_owned(),
        }
    }
}

/// The secret of one login and what goes along with it.
#[derive(Clone)]
pub struct Pkce {
    /// The secret itself, sent only with the code.
    pub verifier: String,
    /// The hash of the secret that the login page sees.
    pub challenge: String,
    /// A word that comes back with the code, so a code meant for another login is refused.
    pub state: String,
}

impl std::fmt::Debug for Pkce {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Pkce").finish_non_exhaustive()
    }
}

impl Pkce {
    /// A new secret, from the system's source of randomness.
    ///
    /// # Errors
    ///
    /// When the system has no randomness to give, which no login should go on without.
    pub fn new() -> Result<Self, SourceError> {
        let mut secret = [0u8; 64];
        let mut state = [0u8; 16];
        getrandom::fill(&mut secret)
            .and_then(|()| getrandom::fill(&mut state))
            .map_err(|_| SourceError::Server("no randomness".to_owned()))?;
        let verifier = base64url(&secret);
        let challenge = base64url(&Sha256::digest(verifier.as_bytes()));
        Ok(Self { verifier, challenge, state: base64url(&state) })
    }
}

/// The tokens a login gives.
#[derive(Clone)]
pub struct Tokens {
    /// What every request is sent with.
    pub access: String,
    /// What buys a new access token once this one runs out.
    pub refresh: String,
    /// When the access token runs out.
    pub expires: Instant,
}

impl std::fmt::Debug for Tokens {
    // A token printed in a test failure or a log would be a login given away.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Tokens").field("expires", &self.expires).finish_non_exhaustive()
    }
}

/// The address of Spotify's login page for `client` with `pkce`.
#[must_use]
pub fn authorize_address(endpoints: &Endpoints, client: &str, pkce: &Pkce) -> String {
    let fields = [
        ("client_id", client),
        ("response_type", "code"),
        ("redirect_uri", REDIRECT),
        ("scope", SCOPES),
        ("code_challenge_method", "S256"),
        ("code_challenge", &pkce.challenge),
        ("state", &pkce.state),
    ];
    format!("{}?{}", endpoints.authorize, form(&fields))
}

/// Waits on `listener` for the browser to come back from the login with `state`, at most
/// `patience` and until `stop` is set, answers it with a page saying the tab can be closed, and
/// gives the code.
///
/// # Errors
///
/// [`SourceError::Login`] when the person said no on the login page or the browser brought back
/// another login's state; [`SourceError::Unreachable`] with [`Reach::Timeout`] when nothing came,
/// and with [`Reach::Other`] once the wait was stopped.
pub fn wait_for_code(
    listener: &TcpListener,
    state: &str,
    patience: Duration,
    stop: &AtomicBool,
) -> Result<String, SourceError> {
    let gone = || SourceError::Unreachable(Reach::Other);
    listener.set_nonblocking(true).map_err(|_| gone())?;
    let start = Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).map_err(|_| gone())?;
                let mut reader = BufReader::new(stream.try_clone().map_err(|_| gone())?);
                let mut first = String::new();
                reader.read_line(&mut first).map_err(|_| gone())?;
                let target = first.split_whitespace().nth(1).unwrap_or_default();
                // A browser asks for its icon too; only the address with the answer counts.
                if !target.starts_with("/login") {
                    let _ = (&stream).write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                    continue;
                }
                let answer = code_of(target, state);
                let page: &[u8] = if answer.is_ok() { DONE } else { REFUSED };
                let _ = (&stream).write_all(page);
                return answer;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                // The login came back some other way, or the person gave up on it.
                if stop.load(Ordering::Relaxed) {
                    return Err(gone());
                }
                if start.elapsed() >= patience {
                    return Err(SourceError::Unreachable(Reach::Timeout));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return Err(gone()),
        }
    }
}

/// The page the browser shows once the login came back.
const DONE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n<!doctype html><title>qmus</title><p>qmus is logged in to Spotify. This tab can be closed.</p>";

/// The page the browser shows when the login came back refused.
const REFUSED: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n<!doctype html><title>qmus</title><p>qmus is not logged in to Spotify. This tab can be closed.</p>";

/// The code an address the browser came back to (or one the person pasted) carries, when it comes
/// back for the login with `state`.
///
/// # Errors
///
/// [`SourceError::Login`] when it carries an error, another state or no code.
pub fn code_of(address: &str, state: &str) -> Result<String, SourceError> {
    let query = address.split_once('?').map_or("", |(_, query)| query);
    let field = |name: &str| {
        query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == name).then(|| percent_decode_str(value).decode_utf8_lossy().into_owned())
        })
    };
    if field("state").as_deref() != Some(state) || field("error").is_some() {
        return Err(SourceError::Login);
    }
    field("code").filter(|code| !code.is_empty()).ok_or(SourceError::Login)
}

/// Exchanges `code` and the secret `verifier` of its login for tokens.
///
/// # Errors
///
/// [`SourceError::Login`] when Spotify refuses the code, and why it could not be reached.
pub fn exchange(endpoints: &Endpoints, client: &str, code: &str, verifier: &str) -> Result<Tokens, SourceError> {
    let fields = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", REDIRECT),
        ("client_id", client),
        ("code_verifier", verifier),
    ];
    tokens(endpoints, &fields, None)
}

/// A new access token for `refresh`, once the one before has run out; Spotify may hand out a new
/// refresh token with it, and keeps the old one working when it does not.
///
/// # Errors
///
/// As [`exchange`].
pub fn refresh(endpoints: &Endpoints, client: &str, refresh: &str) -> Result<Tokens, SourceError> {
    let fields = [("grant_type", "refresh_token"), ("refresh_token", refresh), ("client_id", client)];
    tokens(endpoints, &fields, Some(refresh))
}

/// Asks the token endpoint with `fields`; `kept` is the refresh token to keep when none comes back.
fn tokens(endpoints: &Endpoints, fields: &[(&str, &str)], kept: Option<&str>) -> Result<Tokens, SourceError> {
    let mut answer = super::super::agent()
        .post(&endpoints.token)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(form(fields))
        .map_err(|error| SourceError::Unreachable(reach_of(&error)))?;
    let status = answer.status().as_u16();
    let text = answer.body_mut().read_to_string().map_err(|error| SourceError::Unreachable(reach_of(&error)))?;
    match status {
        200..=299 => {}
        400 | 401 | 403 => return Err(SourceError::Login),
        _ => return Err(SourceError::Server(format!("HTTP {status}"))),
    }
    let value: Value = serde_json::from_str(&text).map_err(|_| SourceError::Server("not Spotify".to_owned()))?;
    let access = value.get("access_token").and_then(Value::as_str).filter(|token| !token.is_empty());
    let refresh = value.get("refresh_token").and_then(Value::as_str).or(kept);
    let (Some(access), Some(refresh)) = (access, refresh) else {
        return Err(SourceError::Server("not Spotify".to_owned()));
    };
    let lasts = value.get("expires_in").and_then(Value::as_u64).unwrap_or(3600);
    Ok(Tokens {
        access: access.to_owned(),
        refresh: refresh.to_owned(),
        expires: Instant::now() + Duration::from_secs(lasts),
    })
}

/// `fields` as a query or a form body.
fn form(fields: &[(&str, &str)]) -> String {
    let parts: Vec<String> =
        fields.iter().map(|(name, value)| format!("{name}={}", utf8_percent_encode(value, FIELD))).collect();
    parts.join("&")
}

/// `bytes` in the URL-safe base64 alphabet without padding, as PKCE writes them.
fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let word = chunk.iter().enumerate().fold(0u32, |word, (at, byte)| word | u32::from(*byte) << (16 - 8 * at));
        for at in 0..=chunk.len() {
            out.push(char::from(ALPHABET[(word >> (18 - 6 * at) & 63) as usize]));
        }
    }
    out
}

#[cfg(test)]
mod tests;
