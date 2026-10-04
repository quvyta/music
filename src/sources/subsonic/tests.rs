use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::testing::server::{FakeServer, Request, Response};

/// The password every test logs in with; no request may carry it.
const PASSWORD: &str = "gizli parola 1";

/// An answer of the API that went well, holding `inner` beside the status.
fn ok(inner: &str) -> Response {
    let comma = if inner.is_empty() { "" } else { "," };
    Response::json(&format!(r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1"{comma}{inner}}}}}"#))
}

/// An answer of the API that failed with `code`.
fn failed(code: u32) -> Response {
    Response::json(&format!(
        r#"{{"subsonic-response":{{"status":"failed","version":"1.16.1","error":{{"code":{code},"message":"no"}}}}}}"#
    ))
}

/// A song item of the API.
fn song(id: usize, title: &str) -> String {
    format!(
        r#"{{"id":"{id}","title":"{title}","artist":"Kalben","album":"Sonsuz","track":{n},"duration":187,"coverArt":"al-{id}"}}"#,
        n = id % 20 + 1
    )
}

/// The server logged in to with the password.
fn with_password(server: &FakeServer) -> Subsonic {
    Subsonic::new(&server.url(), "hakan", Auth::Password(PASSWORD.to_owned())).expect("a server")
}

/// That no request carried the password, in any field or header.
fn password_never_sent(requests: &[Request]) {
    for request in requests {
        assert!(request.query.iter().all(|(_, value)| !value.contains(PASSWORD)), "{request:?}");
        assert!(request.headers.values().all(|value| !value.contains(PASSWORD)), "{request:?}");
        assert!(request.get("p").is_none(), "the password is never sent, not even encoded: {request:?}");
    }
}

#[test]
fn the_token_is_the_md5_of_the_password_and_the_salt_as_the_api_says() {
    // The example the Subsonic API's own documentation gives.
    assert_eq!(token("sesame", "c19b2d"), "26719a1196d2a940705a59634eb18eab");
}

#[test]
fn a_password_login_sends_a_new_salt_and_its_token_each_time_and_never_the_password() {
    let server = FakeServer::start(|_| ok(""));
    let subsonic = with_password(&server);
    subsonic.ping().expect("the server is there");
    subsonic.ping().expect("and still is");
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(request.path, "/rest/ping");
        assert_eq!(request.get("u"), Some("hakan"));
        assert_eq!(request.get("c"), Some("qmus"));
        assert_eq!(request.get("f"), Some("json"));
        let salt = request.get("s").expect("a salt");
        assert_eq!(request.get("t"), Some(token(PASSWORD, salt).as_str()), "the token goes with its own salt");
    }
    assert_ne!(requests[0].get("s"), requests[1].get("s"), "a request overheard cannot be sent again");
    password_never_sent(&requests);
}

#[test]
fn an_api_key_login_sends_the_key_alone() {
    let server = FakeServer::start(|_| ok(""));
    let subsonic = Subsonic::new(&server.url(), "hakan", Auth::ApiKey("anahtar-42".to_owned())).expect("a server");
    subsonic.ping().expect("the server is there");
    let request = &server.requests()[0];
    assert_eq!(request.get("apiKey"), Some("anahtar-42"));
    assert!(request.get("u").is_none() && request.get("t").is_none() && request.get("s").is_none(), "{request:?}");
}

#[test]
fn the_whole_catalogue_is_read_a_page_at_a_time_each_track_once() {
    const TRACKS: usize = 1203;
    let server = FakeServer::start(|request| {
        let offset: usize = request.get("songOffset").and_then(|offset| offset.parse().ok()).unwrap_or(0);
        let count: usize = request.get("songCount").and_then(|count| count.parse().ok()).unwrap_or(20);
        let songs: Vec<String> =
            (offset..(offset + count).min(TRACKS)).map(|id| song(id, &format!("Şarkı {id}"))).collect();
        ok(&format!(r#""searchResult3":{{"song":[{}]}}"#, songs.join(",")))
    });
    let tracks = with_password(&server).catalogue().expect("the catalogue");
    assert_eq!(tracks.len(), TRACKS);
    let ids: std::collections::HashSet<_> = tracks.iter().map(|track| track.id.clone()).collect();
    assert_eq!(ids.len(), TRACKS, "no track twice");
    let offsets: Vec<_> =
        server.requests().iter().map(|request| request.get("songOffset").unwrap_or("").to_owned()).collect();
    assert_eq!(offsets, ["0", "500", "1000"]);
    assert!(
        server.requests().iter().all(|request| request.get("query") == Some("")),
        "an empty query asks for everything"
    );
}

#[test]
fn a_song_of_the_api_becomes_a_track_with_every_field_it_gives() {
    let server = FakeServer::start(|_| {
        ok(
            r#""searchResult3":{"song":{"id":17,"title":" Gece Mavisi ","artist":"Kalben","album":"Sonsuz","track":3,"duration":0,"coverArt":"al-1"}}"#,
        )
    });
    let tracks = with_password(&server).search("gece").expect("found");
    assert_eq!(
        tracks,
        [RemoteTrack {
            id: "17".to_owned(),
            title: "Gece Mavisi".to_owned(),
            artist: "Kalben".to_owned(),
            album: "Sonsuz".to_owned(),
            number: Some(3),
            duration: None,
            cover: Some("al-1".to_owned()),
        }],
        "one song comes as an object rather than a list, a number as an id, and no length as zero"
    );
    assert_eq!(server.requests()[0].get("query"), Some("gece"));
}

#[test]
fn a_folder_listed_among_songs_is_left_out() {
    let server = FakeServer::start(|_| {
        ok(r#""searchResult3":{"song":[{"id":"d1","title":"Klasör","isDir":true},{"id":"s1","title":"Şarkı"}]}"#)
    });
    let tracks = with_password(&server).search("").expect("found");
    assert_eq!(tracks.iter().map(|track| track.id.as_str()).collect::<Vec<_>>(), ["s1"]);
}

#[test]
fn a_wrong_login_is_said_as_such_whichever_way_the_server_says_it() {
    let server = FakeServer::start(|_| failed(40));
    assert_eq!(with_password(&server).ping(), Err(SourceError::Login));
    let server = FakeServer::start(|_| failed(44));
    assert_eq!(with_password(&server).ping(), Err(SourceError::Login), "a wrong API key");
    let server = FakeServer::start(|_| Response::status(401));
    assert_eq!(with_password(&server).ping(), Err(SourceError::Login), "an HTTP refusal");
    password_never_sent(&server.requests());
}

#[test]
fn something_not_there_is_not_found_and_another_failure_keeps_the_servers_words() {
    let server = FakeServer::start(|_| failed(70));
    assert_eq!(with_password(&server).playlist("yok"), Err(SourceError::NotFound));
    let server = FakeServer::start(|_| failed(0));
    assert_eq!(with_password(&server).ping(), Err(SourceError::Server("no".to_owned())));
}

#[test]
fn a_page_that_is_not_the_api_says_it_is_not_a_subsonic_server() {
    let server = FakeServer::start(|_| Response::bytes(b"<html>router login</html>".to_vec()));
    assert_eq!(with_password(&server).ping(), Err(SourceError::Server("not a Subsonic server".to_owned())));
}

#[test]
fn a_server_that_is_off_is_unreachable_and_says_why() {
    // A port that was free a moment ago and has nothing listening on it now.
    let address = std::net::TcpListener::bind("127.0.0.1:0").expect("a port").local_addr().expect("its address");
    let subsonic =
        Subsonic::new(&format!("http://{address}"), "hakan", Auth::Password(PASSWORD.to_owned())).expect("a server");
    assert_eq!(subsonic.ping(), Err(SourceError::Unreachable(Reach::Refused)));
}

#[test]
fn an_address_without_a_web_scheme_or_a_host_is_refused_before_anything_is_sent() {
    for address in ["", "music.local", "ftp://music.local", "http://", "https:///rest"] {
        assert!(
            matches!(Subsonic::new(address, "hakan", Auth::Password(PASSWORD.to_owned())), Err(SourceError::Address)),
            "{address}"
        );
    }
    let subsonic =
        Subsonic::new(" https://music.example.org/ ", "hakan", Auth::Password(PASSWORD.to_owned())).expect("a server");
    assert!(
        subsonic.stream_address("1").starts_with("https://music.example.org/rest/stream?"),
        "the slash at the end is not doubled"
    );
}

#[test]
fn the_login_is_never_in_what_is_printed_of_a_server() {
    let subsonic = Subsonic::new("http://127.0.0.1:1", "hakan", Auth::Password(PASSWORD.to_owned())).expect("a server");
    let printed = format!("{subsonic:?}");
    assert!(!printed.contains(PASSWORD), "{printed}");
    let subsonic =
        Subsonic::new("http://127.0.0.1:1", "hakan", Auth::ApiKey("anahtar-42".to_owned())).expect("a server");
    assert!(!format!("{subsonic:?}").contains("anahtar-42"));
}

#[test]
fn the_stream_address_names_the_track_and_carries_a_token_not_the_password() {
    let server = FakeServer::start(|_| ok(""));
    let address = with_password(&server).stream_address("tr 17/ş");
    assert!(address.starts_with(&format!("{}/rest/stream?", server.url())), "{address}");
    assert!(address.contains("id=tr%2017%2F%C5%9F"), "an id with any letters in it stays one field: {address}");
    assert!(address.contains("&t=") && address.contains("&s="), "{address}");
    assert!(!address.contains("gizli"), "{address}");
}

#[test]
fn a_cover_comes_back_as_its_picture_and_a_missing_one_is_not_found() {
    let picture = vec![0x89, b'P', b'N', b'G', 1, 2, 3];
    let sent = picture.clone();
    let server = FakeServer::start(move |request| match request.get("id") {
        Some("al-1") => Response::bytes(sent.clone()).header("Content-Type", "image/png"),
        _ => failed(70),
    });
    let subsonic = with_password(&server);
    assert_eq!(subsonic.cover("al-1", 600), Ok(picture));
    assert_eq!(server.requests()[0].get("size"), Some("600"));
    assert_eq!(subsonic.cover("al-2", 600), Err(SourceError::NotFound));
}

#[test]
fn the_servers_playlists_and_their_tracks_are_listed_in_their_order() {
    let server = FakeServer::start(|request| match request.path.as_str() {
        "/rest/getPlaylists" => ok(
            r#""playlists":{"playlist":[{"id":"p1","name":"Yol","songCount":2},{"id":"p2","name":"Gece","songCount":0}]}"#,
        ),
        "/rest/getPlaylist" => {
            ok(&format!(r#""playlist":{{"id":"p1","name":"Yol","entry":[{},{}]}}"#, song(9, "Son"), song(2, "İlk")))
        }
        _ => failed(0),
    });
    let subsonic = with_password(&server);
    let lists = subsonic.playlists().expect("the playlists");
    assert_eq!(
        lists,
        [
            RemotePlaylist { id: "p1".to_owned(), name: "Yol".to_owned(), tracks: 2 },
            RemotePlaylist { id: "p2".to_owned(), name: "Gece".to_owned(), tracks: 0 }
        ]
    );
    let tracks = subsonic.playlist("p1").expect("its tracks");
    assert_eq!(
        tracks.iter().map(|track| track.title.as_str()).collect::<Vec<_>>(),
        ["Son", "İlk"],
        "the playlist's order"
    );
    assert_eq!(server.requests()[1].get("id"), Some("p1"));
}

#[test]
fn a_track_heard_is_told_to_the_server_once_while_heard_and_once_when_done() {
    let asked = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&asked);
    let server = FakeServer::start(move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
        ok("")
    });
    let subsonic = with_password(&server);
    subsonic.scrobble("17", false).expect("told");
    subsonic.scrobble("17", true).expect("told");
    let submissions: Vec<_> =
        server.requests().iter().map(|request| request.get("submission").unwrap_or("").to_owned()).collect();
    assert_eq!(submissions, ["false", "true"]);
    assert_eq!(asked.load(Ordering::SeqCst), 2);
}
