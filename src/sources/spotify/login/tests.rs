use std::io::Read;
use std::net::TcpStream;

use super::*;
use crate::testing::server::{FakeServer, Response};

/// Sends the browser's request for `target` to `port`, and gives back what qmus answered.
fn browse(port: u16, target: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("qmus listens");
    stream.write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes()).expect("asked");
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
    answer
}

/// A listener on a free port of the loopback address, and its port.
fn listener() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
    let port = listener.local_addr().expect("the address").port();
    (listener, port)
}

#[test]
fn the_secret_and_its_hash_are_the_ones_the_standard_gives() {
    assert_eq!(
        [base64url(b"f"), base64url(b"fo"), base64url(b"foo"), base64url(b"foob")],
        ["Zg", "Zm8", "Zm9v", "Zm9vYg"]
    );
    // RFC 7636, appendix B.
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    assert_eq!(base64url(&Sha256::digest(verifier.as_bytes())), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    let (one, two) = (Pkce::new().expect("a secret"), Pkce::new().expect("a secret"));
    assert_ne!(one.verifier, two.verifier, "every login has its own secret");
    assert!((43..=128).contains(&one.verifier.len()), "{}", one.verifier.len());
    assert_eq!(one.challenge, base64url(&Sha256::digest(one.verifier.as_bytes())));
    assert!(!format!("{one:?}").contains(&one.verifier));
}

#[test]
fn the_login_page_sees_the_hash_and_never_the_secret() {
    let pkce = Pkce::new().expect("a secret");
    let address = authorize_address(&Endpoints::default(), "client-1", &pkce);
    assert!(address.starts_with("https://accounts.spotify.com/authorize?client_id=client-1&response_type=code"));
    assert!(
        address.contains(&format!("code_challenge={}", pkce.challenge))
            && address.contains("code_challenge_method=S256")
    );
    assert!(address.contains(&format!("state={}", pkce.state)));
    assert!(address.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8898%2Flogin"), "{address}");
    assert!(!address.contains(&pkce.verifier));
}

#[test]
fn the_code_the_browser_brings_back_is_taken_and_the_tab_is_told_it_can_close() {
    let (listener, port) = listener();
    let browser = std::thread::spawn(move || {
        let icon = browse(port, "/favicon.ico");
        let page = browse(port, "/login?code=kod-42&state=durum");
        (icon, page)
    });
    let code = wait_for_code(&listener, "durum", Duration::from_secs(20), &AtomicBool::new(false));
    let (icon, page) = browser.join().expect("the browser");
    assert_eq!(code, Ok("kod-42".to_owned()));
    assert!(icon.starts_with("HTTP/1.1 404"), "{icon}");
    assert!(page.contains("This tab can be closed"), "{page}");
}

#[test]
fn another_logins_state_a_refusal_and_silence_bring_no_code() {
    assert_eq!(code_of("/login?code=k&state=other", "durum"), Err(SourceError::Login));
    assert_eq!(code_of("/login?error=access_denied&state=durum", "durum"), Err(SourceError::Login));
    assert_eq!(code_of("http://127.0.0.1:8898/login?code=k%2D1&state=durum", "durum"), Ok("k-1".to_owned()));
    let (listener, _) = listener();
    let started = Instant::now();
    assert_eq!(
        wait_for_code(&listener, "durum", Duration::from_millis(300), &AtomicBool::new(false)),
        Err(SourceError::Unreachable(Reach::Timeout))
    );
    assert!(started.elapsed() < Duration::from_secs(10), "the wait has an end");
}

/// A token endpoint that gives tokens for the code `kod-42` and the refresh token `yenile`.
fn token_server() -> FakeServer {
    FakeServer::start(|request| {
        if request.body.contains("code=kod-42") || request.body.contains("refresh_token=yenile") {
            let refresh =
                if request.body.contains("grant_type=refresh_token") { "" } else { r#","refresh_token":"yenile""# };
            Response::json(&format!(r#"{{"access_token":"erisim","token_type":"Bearer","expires_in":3600{refresh}}}"#))
        } else {
            Response { status: 400, ..Response::json(r#"{"error":"invalid_grant"}"#) }
        }
    })
}

#[test]
fn a_code_and_its_secret_buy_tokens_and_the_refresh_token_buys_the_next() {
    let server = token_server();
    let endpoints = Endpoints { authorize: String::new(), token: format!("{}/api/token", server.url()) };
    let tokens = exchange(&endpoints, "client-1", "kod-42", "gizli-dogrulayici").expect("tokens");
    assert_eq!((tokens.access.as_str(), tokens.refresh.as_str()), ("erisim", "yenile"));
    assert!(tokens.expires > Instant::now() + Duration::from_secs(3000));
    let sent = &server.requests()[0];
    assert_eq!(sent.method, "POST");
    assert!(sent.body.contains("code_verifier=gizli-dogrulayici") && sent.body.contains("client_id=client-1"));
    let again = refresh(&endpoints, "client-1", "yenile").expect("a new token");
    assert_eq!(again.refresh, "yenile", "a refresh that names none keeps the one it had");
    assert_eq!(exchange(&endpoints, "client-1", "eski", "x").err(), Some(SourceError::Login));
    let shown = format!("{tokens:?}");
    assert!(!shown.contains("erisim") && !shown.contains("yenile"), "{shown}");
}

#[test]
fn a_wait_that_is_stopped_ends_at_once() {
    let (listener, _) = listener();
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let stopper = std::sync::Arc::clone(&stop);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        stopper.store(true, Ordering::Relaxed);
    });
    let started = Instant::now();
    assert_eq!(
        wait_for_code(&listener, "durum", Duration::from_secs(60), &stop),
        Err(SourceError::Unreachable(Reach::Other))
    );
    assert!(started.elapsed() < Duration::from_secs(10), "stopped long before its patience ran out");
}
