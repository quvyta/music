use super::*;
use crate::testing::jellyfin::{self, PASSWORD, Shelf, TOKEN, USER};
use crate::testing::server::FakeServer;

/// A server of three tracks and one playlist of two of them.
fn server() -> FakeServer {
    jellyfin::start(Shelf {
        songs: vec![
            ("j-1", "Gece Mavisi", "Kalben", "Ayrı", 1, 187),
            ("j-2", "Sabah Treni", "Kalben", "Ayrı", 2, 201),
            ("j-3", "Uzun Yol", "Adamlar", "Eski", 1, 240),
        ],
        playlists: vec![("p-1", "Yol", vec!["j-3", "j-1"])],
        tone: Vec::new(),
    })
}

/// Logged in to `server` as the account `jellyfin-77aa`.
fn logged_in(server: &FakeServer) -> Jellyfin {
    Jellyfin::login(&server.url(), "hakan", PASSWORD, "jellyfin-77aa").expect("logged in")
}

#[test]
fn the_password_goes_once_to_log_in_and_the_session_goes_with_every_request_after() {
    let server = server();
    let jellyfin = logged_in(&server);
    jellyfin.ping().expect("the session holds");
    let requests = server.requests();
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/Users/AuthenticateByName");
    assert!(requests[0].body.contains(PASSWORD) && requests[0].body.contains("hakan"));
    let header = &requests[0].headers["authorization"];
    assert!(header.contains("Client=\"qmus\"") && header.contains("DeviceId=\"jellyfin-77aa\""), "{header}");
    for later in &requests[1..] {
        assert!(!later.body.contains(PASSWORD) && !later.query.iter().any(|(_, value)| value == PASSWORD));
        assert!(later.headers["authorization"].contains(&format!("Token=\"{TOKEN}\"")), "{later:?}");
    }
    let shown = format!("{jellyfin:?}");
    assert!(!shown.contains(TOKEN) && !shown.contains(PASSWORD), "{shown}");
}

#[test]
fn a_wrong_password_is_a_login_turned_away_and_a_server_that_is_not_there_is_unreachable() {
    let server = server();
    let wrong = Jellyfin::login(&server.url(), "hakan", "yanlış", "jellyfin-77aa");
    assert_eq!(wrong.err(), Some(SourceError::Login));
    let gone = Jellyfin::login("http://127.0.0.1:1", "hakan", PASSWORD, "jellyfin-77aa");
    assert!(matches!(gone, Err(SourceError::Unreachable(_))), "{gone:?}");
    assert_eq!(Jellyfin::login("ftp://x", "hakan", PASSWORD, "k").err(), Some(SourceError::Address));
}

#[test]
fn the_catalogue_gives_every_track_with_its_fields_a_page_at_a_time() {
    let server = server();
    let tracks = logged_in(&server).catalogue().expect("the catalogue");
    assert_eq!(tracks.len(), 3);
    assert_eq!(
        tracks[0],
        RemoteTrack {
            id: "j-1".into(),
            title: "Gece Mavisi".into(),
            artist: "Kalben".into(),
            album: "Ayrı".into(),
            number: Some(1),
            duration: Some(Duration::from_secs(187)),
            cover: Some("al-Ayrı".into()),
        }
    );
    let asked =
        server.requests().into_iter().find(|request| request.path == format!("/Users/{USER}/Items")).expect("a page");
    assert_eq!(
        (asked.get("IncludeItemTypes"), asked.get("StartIndex"), asked.get("Limit")),
        (Some("Audio"), Some("0"), Some("500"))
    );
}

#[test]
fn a_track_is_fetched_from_an_address_with_the_session_in_it() {
    let server = server();
    let address = logged_in(&server).stream_address("j-2");
    assert!(address.starts_with(&format!("{}/Audio/j-2/stream?", server.url())), "{address}");
    assert!(address.contains("static=true") && address.contains(&format!("api_key={TOKEN}")), "{address}");
}

#[test]
fn playlists_list_and_open_in_their_own_order() {
    let server = server();
    let jellyfin = logged_in(&server);
    let lists = jellyfin.playlists().expect("the playlists");
    assert_eq!(lists, [RemotePlaylist { id: "p-1".into(), name: "Yol".into(), tracks: 2 }]);
    let titles: Vec<String> =
        jellyfin.playlist("p-1").expect("the tracks").into_iter().map(|track| track.title).collect();
    assert_eq!(titles, ["Uzun Yol", "Gece Mavisi"]);
    assert_eq!(jellyfin.playlist("p-9").err(), Some(SourceError::NotFound));
}

#[test]
fn a_cover_is_a_picture_and_plays_are_reported_where_the_api_wants_them() {
    let server = server();
    let jellyfin = logged_in(&server);
    assert_eq!(jellyfin.cover("al-Ayrı", 600).expect("a picture"), crate::testing::solid_png([0, 0, 255]));
    jellyfin.scrobble("j-1", false).expect("begun");
    jellyfin.scrobble("j-1", true).expect("heard");
    let posts: Vec<(String, String)> = server
        .requests()
        .into_iter()
        .filter(|request| request.method == "POST" && request.path != "/Users/AuthenticateByName")
        .map(|request| (request.path, request.body))
        .collect();
    assert_eq!(posts[0].0, "/Sessions/Playing");
    assert!(posts[0].1.contains("\"ItemId\":\"j-1\""), "{posts:?}");
    assert_eq!(posts[1].0, format!("/Users/{USER}/PlayedItems/j-1"));
}
