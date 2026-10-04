use std::time::{Duration, Instant};

use super::*;
use crate::testing::server::{FakeServer, Response};

/// Tokens that run out in an hour, or have run out with `spent`.
fn tokens(access: &str, spent: bool) -> Tokens {
    let expires = if spent { Instant::now() } else { Instant::now() + Duration::from_secs(3600) };
    Tokens { access: access.to_owned(), refresh: "yenile".to_owned(), expires }
}

/// A liked song as the API gives it.
fn song(id: &str, name: &str) -> String {
    format!(
        r#"{{"track":{{"id":"{id}","name":"{name}","artists":[{{"name":"Kalben"}},{{"name":"Can Ozan"}}],"album":{{"name":"Sonsuz","images":[{{"url":"https://i.scdn.co/image/big","width":640}},{{"url":"https://i.scdn.co/image/small","width":64}}]}},"track_number":3,"duration_ms":187500}}}}"#
    )
}

/// A Web API with 120 liked songs, one local file among them, a playlist and a Premium person,
/// and a token endpoint that renews `yenile`; only `taze` is let through once `eski` has run out.
fn api(product: &'static str) -> FakeServer {
    FakeServer::start(move |request| {
        if request.path == "/api/token" {
            return Response::json(r#"{"access_token":"taze","expires_in":3600}"#);
        }
        let bearer = request.headers.get("authorization").cloned().unwrap_or_default();
        if bearer != "Bearer taze" && bearer != "Bearer eski" {
            return Response::status(401);
        }
        let offset: usize = request.get("offset").and_then(|at| at.parse().ok()).unwrap_or(0);
        match request.path.as_str() {
            "/v1/me" => Response::json(&format!(r#"{{"id":"hakan","product":"{product}"}}"#)),
            "/v1/me/tracks" => {
                let mut items: Vec<String> = (offset..(offset + 50).min(120))
                    .map(|at| song(&format!("s{at}"), &format!("Şarkı {at}")))
                    .collect();
                if offset == 0 {
                    items[1] = r#"{"track":{"id":null,"is_local":true,"name":"Ev kaydı"}}"#.to_owned();
                }
                let next = if offset + 50 < 120 { r#""more""# } else { "null" };
                Response::json(&format!(r#"{{"items":[{}],"next":{next}}}"#, items.join(",")))
            }
            "/v1/me/playlists" => {
                Response::json(r#"{"items":[{"id":"pl1","name":"Yol","tracks":{"total":2}}],"next":null}"#)
            }
            "/v1/playlists/pl1/tracks" => {
                Response::json(&format!(r#"{{"items":[{},{}],"next":null}}"#, song("s7", "Yedi"), song("s3", "Üç")))
            }
            _ => Response::status(404),
        }
    })
}

/// The account at `server` with `tokens`.
fn account(server: &FakeServer, tokens: Tokens) -> Spotify {
    let endpoints = Endpoints { authorize: String::new(), token: format!("{}/api/token", server.url()) };
    Spotify::new(&format!("{}/v1", server.url()), endpoints, "client-1", tokens)
}

#[test]
fn the_liked_songs_come_a_page_at_a_time_with_every_field_and_without_local_files() {
    let server = api("premium");
    let tracks = account(&server, tokens("taze", false)).catalogue().expect("the liked songs");
    assert_eq!(tracks.len(), 119, "the local file is passed over");
    assert_eq!(
        tracks[0],
        RemoteTrack {
            id: "s0".into(),
            title: "Şarkı 0".into(),
            artist: "Kalben, Can Ozan".into(),
            album: "Sonsuz".into(),
            number: Some(3),
            duration: Some(Duration::from_millis(187_500)),
            cover: Some("https://i.scdn.co/image/big".into()),
        }
    );
    let offsets: Vec<String> =
        server.requests().iter().filter_map(|request| request.get("offset").map(str::to_owned)).collect();
    assert_eq!(offsets, ["0", "50", "100"]);
}

#[test]
fn an_account_that_is_not_premium_is_said_to_be_one() {
    assert_eq!(account(&api("free"), tokens("taze", false)).ping(), Err(SourceError::Premium));
    assert_eq!(account(&api("premium"), tokens("taze", false)).ping(), Ok(()));
}

#[test]
fn a_token_that_ran_out_is_renewed_before_the_request_and_the_renewed_one_is_shared() {
    let server = api("premium");
    let spotify = account(&server, tokens("bayat", true));
    let copy = spotify.clone();
    spotify.ping().expect("renewed first");
    copy.ping().expect("the copy uses the renewed token");
    let renewals = server.requests().iter().filter(|request| request.path == "/api/token").count();
    assert_eq!(renewals, 1, "one renewal serves every copy");
    assert!(!format!("{spotify:?}").contains("taze"));
}

#[test]
fn a_token_spotify_turns_away_is_renewed_once_and_the_request_sent_again() {
    let server = api("premium");
    // Not run out by the clock, but no longer taken.
    let spotify = account(&server, tokens("reddedilen", false));
    spotify.ping().expect("renewed and sent again");
    let paths: Vec<String> = server.requests().into_iter().map(|request| request.path).collect();
    assert_eq!(paths, ["/v1/me", "/api/token", "/v1/me"]);
}

#[test]
fn playlists_list_and_open_in_their_own_order() {
    let server = api("premium");
    let spotify = account(&server, tokens("taze", false));
    assert_eq!(
        spotify.playlists().expect("lists"),
        [RemotePlaylist { id: "pl1".into(), name: "Yol".into(), tracks: 2 }]
    );
    let titles: Vec<String> = spotify.playlist("pl1").expect("tracks").into_iter().map(|track| track.title).collect();
    assert_eq!(titles, ["Yedi", "Üç"]);
    assert_eq!(spotify.playlist("pl9").err(), Some(SourceError::NotFound));
}
