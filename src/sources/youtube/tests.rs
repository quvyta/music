use super::*;
use crate::testing::Scratch;
use crate::testing::ytdlp::{LIKED, Way, fake};

#[test]
fn the_liked_music_comes_from_yt_dlp_with_the_browsers_login_and_every_field() {
    let scratch = Scratch::new("ytdlp-liked");
    let stand_in = fake(&scratch, LIKED, "https://audio.example/a.m4a", Way::Answers);
    let ytdlp = YtDlp::new(&stand_in.program, "firefox");
    let tracks = ytdlp.liked().expect("the liked music");
    assert_eq!(
        tracks[0],
        RemoteTrack {
            id: "yt-aaaaaaa".into(),
            title: "Gece Mavisi".into(),
            artist: "Kalben".into(),
            album: String::new(),
            number: None,
            duration: Some(Duration::from_secs(187)),
            cover: Some("https://i.ytimg.com/vi/yt-aaaaaaa/hqdefault.jpg".into()),
        }
    );
    assert_eq!(tracks[1].artist, "Kalben", "an uploader is an artist too");
    let calls = stand_in.calls();
    assert!(calls[0].contains("--cookies-from-browser firefox") && calls[0].contains("list=LM"), "{calls:?}");
}

#[test]
fn a_tracks_sound_is_asked_for_as_aac_and_the_version_needs_no_login() {
    let scratch = Scratch::new("ytdlp-audio");
    let stand_in = fake(&scratch, LIKED, "https://audio.example/a.m4a", Way::Answers);
    let ytdlp = YtDlp::new(&stand_in.program, "chromium");
    assert_eq!(ytdlp.version().expect("it runs"), "2025.09.26");
    assert_eq!(ytdlp.audio_address("yt-aaaaaaa"), Ok("https://audio.example/a.m4a".to_owned()));
    let calls = stand_in.calls();
    assert!(!calls[0].contains("cookies"), "{calls:?}");
    assert!(
        calls[1].contains("-f bestaudio[ext=m4a]")
            && calls[1].contains("-g -- https://music.youtube.com/watch?v=yt-aaaaaaa")
    );
}

#[test]
fn no_login_in_the_browser_is_said_as_one() {
    let scratch = Scratch::new("ytdlp-nologin");
    let stand_in = fake(&scratch, LIKED, "", Way::NoLogin);
    let ytdlp = YtDlp::new(&stand_in.program, "firefox");
    assert_eq!(ytdlp.liked(), Err(SourceError::Login));
    assert!(ytdlp.audio_address("x").is_err());
}

#[test]
fn a_yt_dlp_that_never_finishes_is_stopped_when_its_time_is_up() {
    let scratch = Scratch::new("ytdlp-hangs");
    let stand_in = fake(&scratch, LIKED, "", Way::Hangs);
    let ytdlp =
        YtDlp::new(&stand_in.program, "firefox").patience(Duration::from_millis(500), Duration::from_millis(500));
    let started = Instant::now();
    assert_eq!(ytdlp.liked(), Err(SourceError::Unreachable(Reach::Timeout)));
    assert!(started.elapsed() < Duration::from_secs(20), "the wait has an end: {:?}", started.elapsed());
}
