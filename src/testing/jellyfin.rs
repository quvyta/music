//! A Jellyfin server for tests: it logs in one person with one password, lists the tracks and
//! playlists it is given, sends a tone for every track and a picture for every cover, and turns
//! away any request without the session it handed out.

use serde_json::{Value, json};

use super::server::{FakeServer, Request, Response};

/// The password the server takes.
pub const PASSWORD: &str = "jelly-parola-41";

/// The session token the server hands out.
pub const TOKEN: &str = "tok-5e55";

/// The server's id of the person.
pub const USER: &str = "u-17";

/// A track the server has: its id, title, artist, album, number and length in seconds.
pub type Song = (&'static str, &'static str, &'static str, &'static str, u32, u64);

/// What the server holds.
#[derive(Clone, Default)]
pub struct Shelf {
    /// Its tracks, in its own order.
    pub songs: Vec<Song>,
    /// Its playlists: id, name and the ids of their tracks in order.
    pub playlists: Vec<(&'static str, &'static str, Vec<&'static str>)>,
    /// What it sends for a track.
    pub tone: Vec<u8>,
}

/// Starts the server holding `shelf`.
pub fn start(shelf: Shelf) -> FakeServer {
    FakeServer::start(move |request| answer(&shelf, request))
}

/// Whether `request` carries the session, in the header or in the query.
fn signed(request: &Request) -> bool {
    let header =
        request.headers.get("authorization").is_some_and(|value| value.contains(&format!("Token=\"{TOKEN}\"")));
    header || request.get("api_key") == Some(TOKEN)
}

/// An item of a list answer for `song`.
fn item(song: &Song) -> Value {
    let (id, name, artist, album, number, seconds) = *song;
    json!({
        "Id": id, "Name": name, "Artists": [artist], "Album": album, "IndexNumber": number,
        "RunTimeTicks": seconds * 10_000_000, "AlbumId": format!("al-{album}"), "AlbumPrimaryImageTag": "t",
    })
}

/// What the server says to `request`.
fn answer(shelf: &Shelf, request: &Request) -> Response {
    let path = request.path.as_str();
    if path == "/Users/AuthenticateByName" {
        let body: Value = serde_json::from_str(&request.body).unwrap_or_default();
        if body.get("Pw").and_then(Value::as_str) == Some(PASSWORD) {
            return Response::json(&json!({ "AccessToken": TOKEN, "User": { "Id": USER } }).to_string());
        }
        return Response::status(401);
    }
    if !signed(request) {
        return Response::status(401);
    }
    let items =
        |found: Vec<Value>| Response::json(&json!({ "Items": found, "TotalRecordCount": found.len() }).to_string());
    match path {
        _ if path == format!("/Users/{USER}") => Response::json(&json!({ "Id": USER }).to_string()),
        _ if path == format!("/Users/{USER}/Items") && request.get("IncludeItemTypes") == Some("Playlist") => items(
            shelf
                .playlists
                .iter()
                .map(|(id, name, songs)| json!({ "Id": id, "Name": name, "ChildCount": songs.len() }))
                .collect(),
        ),
        _ if path == format!("/Users/{USER}/Items") => {
            let start: usize = request.get("StartIndex").and_then(|at| at.parse().ok()).unwrap_or(0);
            let limit: usize = request.get("Limit").and_then(|at| at.parse().ok()).unwrap_or(usize::MAX);
            items(shelf.songs.iter().skip(start).take(limit).map(item).collect())
        }
        _ if path.starts_with("/Playlists/") => {
            let id = path.trim_start_matches("/Playlists/").trim_end_matches("/Items");
            let Some((_, _, ids)) = shelf.playlists.iter().find(|(list, _, _)| *list == id) else {
                return Response::status(404);
            };
            items(ids.iter().filter_map(|id| shelf.songs.iter().find(|song| song.0 == *id)).map(item).collect())
        }
        _ if path.starts_with("/Audio/") => Response::bytes(shelf.tone.clone()).header("Content-Type", "audio/wav"),
        _ if path.starts_with("/Items/") => {
            Response::bytes(super::solid_png([0, 0, 255])).header("Content-Type", "image/png")
        }
        "/Sessions/Playing" => Response::status(204),
        _ if path.starts_with(&format!("/Users/{USER}/PlayedItems/")) => Response::json("{}"),
        _ => Response::status(404),
    }
}
