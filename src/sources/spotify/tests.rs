use std::io::{Read, Write};
use std::net::TcpStream;

use super::*;
use crate::testing::spotify::{self, CODE};
use crate::testing::{Scratch, sine_wav};

/// Comes back to `port` from the login page as a browser does, with `code` and `state`.
fn come_back(port: u16, code: &str, state: &str) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("qmus listens");
    write!(stream, "GET /login?code={code}&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").expect("asked");
    let _ = stream.read_to_string(&mut String::new());
}

#[test]
fn a_login_brought_back_by_the_browser_becomes_an_account_with_its_sound() {
    let scratch = Scratch::new("spotify-setup");
    sine_wav(&scratch.path("tone.wav"), 8_000, 1, 0.2, 440.0);
    let server = spotify::start("premium");
    let setup = spotify::setup(&server, &scratch.path("tone.wav"));
    let listener = setup.listen().expect("a free port");
    let port = listener.local_addr().expect("the address").port();
    let pkce = login::Pkce::new().expect("a secret");
    let state = pkce.state.clone();
    let browser = std::thread::spawn(move || come_back(port, CODE, &state));
    let client = setup.log_in(&listener, &pkce, &std::sync::atomic::AtomicBool::new(false)).expect("logged in");
    browser.join().expect("the browser");
    let Client::Spotify(account) = &client else { panic!("a Spotify account: {client:?}") };
    assert!(account.opener("sp1").is_some(), "its tracks have a sound");
    assert_eq!(client.catalogue().expect("the liked songs").len(), 2);
    let exchanged = server.requests().into_iter().find(|request| request.path == "/api/token").expect("the exchange");
    assert!(exchanged.body.contains(&format!("code_verifier={}", pkce.verifier)));
}

#[test]
fn an_account_that_is_not_premium_is_turned_away_before_any_session_is_made() {
    let scratch = Scratch::new("spotify-free");
    let server = spotify::start("free");
    let mut setup = spotify::setup(&server, &scratch.path("none.wav"));
    setup.audio = std::sync::Arc::new(|_| panic!("no session is made for an account that cannot play"));
    let pkce = login::Pkce::new().expect("a secret");
    assert_eq!(setup.finish(CODE, &pkce.verifier).err(), Some(SourceError::Premium));
}
