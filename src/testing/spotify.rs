//! Spotify for tests: a token endpoint and a Web API on the loopback address, and a stand-in for
//! librespot that gives one short tone for every track. Nothing reaches Spotify.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use symphonia::core::io::MediaSource;

use super::server::{FakeServer, Response};
use crate::sources::spotify::Setup;
use crate::sources::spotify::audio::{Audio, SpotifyAudio};
use crate::sources::spotify::login::Endpoints;

/// The code the fake login gives.
pub const CODE: &str = "kod-42";

/// A stand-in for librespot: the tone at its path for every track.
pub struct Tone(pub PathBuf);

impl SpotifyAudio for Tone {
    fn open(&self, _id: &str) -> Result<Box<dyn MediaSource>, String> {
        Ok(Box::new(std::fs::File::open(&self.0).map_err(|error| error.to_string())?))
    }
}

/// Spotify for a person whose account is `product` ("premium" or "free"), with two liked songs of
/// Kalben's "Ayrı" and one playlist of them.
pub fn start(product: &'static str) -> FakeServer {
    FakeServer::start(move |request| match request.path.as_str() {
        "/api/token"
            if request.body.contains(&format!("code={CODE}")) || request.body.contains("grant_type=refresh") =>
        {
            Response::json(r#"{"access_token":"erisim","refresh_token":"yenile","expires_in":3600}"#)
        }
        "/api/token" => Response { status: 400, ..Response::json(r#"{"error":"invalid_grant"}"#) },
        _ if request.headers.get("authorization").map(String::as_str) != Some("Bearer erisim") => Response::status(401),
        "/v1/me" => Response::json(&format!(r#"{{"id":"hakan","product":"{product}"}}"#)),
        "/v1/me/tracks" => Response::json(concat!(
            r#"{"items":["#,
            r#"{"track":{"id":"sp1","name":"Gece Mavisi","artists":[{"name":"Kalben"}],"album":{"name":"Ayrı","images":[]},"track_number":1,"duration_ms":1000}},"#,
            r#"{"track":{"id":"sp2","name":"Sabah Treni","artists":[{"name":"Kalben"}],"album":{"name":"Ayrı","images":[]},"track_number":2,"duration_ms":1000}}"#,
            r#"],"next":null}"#
        )),
        "/v1/me/playlists" => {
            Response::json(r#"{"items":[{"id":"pl1","name":"Yol","tracks":{"total":2}}],"next":null}"#)
        }
        _ => Response::status(404),
    })
}

/// How qmus logs in to `server` and plays the tone at `tone`, listening on a free port.
pub fn setup(server: &FakeServer, tone: &Path) -> Setup {
    let tone = tone.to_path_buf();
    Setup {
        endpoints: Endpoints {
            authorize: format!("{}/authorize", server.url()),
            token: format!("{}/api/token", server.url()),
        },
        api: format!("{}/v1", server.url()),
        client: "client-1".to_owned(),
        port: 0,
        audio: Arc::new(move |_| Ok(Arc::new(Tone(tone.clone())) as Audio)),
    }
}
