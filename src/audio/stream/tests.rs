use std::time::Duration;

use super::*;
use crate::audio::Decoder;
use crate::testing::server::{FakeServer, Response};
use crate::testing::{Scratch, sine_wav};

/// The bytes of a short tone, as a server would send the file.
fn tone(scratch: &Scratch, seconds: f64) -> Vec<u8> {
    let path = scratch.path("made/tone.wav");
    sine_wav(&path, 8_000, 2, seconds, 440.0);
    fs::read(path).expect("the tone")
}

/// How many stereo frames `source` decodes to, or why it stopped.
fn frames_of(source: Box<dyn MediaSource>) -> Result<usize, String> {
    let mut decoder = Decoder::open_source(source, Some("wav"))?;
    let mut out = Vec::new();
    while decoder.next_into(&mut out)? {}
    Ok(out.len() / 2)
}

/// Waits until the track `key` is kept whole in `folder`: the decoder can finish a file before the
/// fetch has put it in its place.
fn kept_whole(folder: &Path, key: &str) -> PathBuf {
    let whole = folder.join(name_of(key));
    let started = Instant::now();
    while !whole.exists() {
        assert!(started.elapsed() < Duration::from_secs(20), "{key} is never kept");
        std::thread::sleep(Duration::from_millis(10));
    }
    whole
}

/// A server that sends `bytes` with `shape` applied to every answer.
fn serving(bytes: Vec<u8>, shape: impl Fn(Response) -> Response + Send + Sync + 'static) -> FakeServer {
    FakeServer::start(move |_| shape(Response::bytes(bytes.clone()).header("Content-Type", "audio/wav")))
}

#[test]
fn a_track_fetched_plays_whole_and_is_kept_for_the_next_time_without_asking_again() {
    let scratch = Scratch::new("stream-whole");
    let bytes = tone(&scratch, 0.5);
    let server = serving(bytes.clone(), |answer| answer);
    let cache = StreamCache::new(scratch.path("cache"), 1 << 30);
    let address = format!("{}/rest/stream?id=1", server.url());
    assert_eq!(frames_of(cache.open("srv/1", &address).expect("opened")), Ok(4_000));
    assert_eq!(fs::read(kept_whole(&scratch.path("cache"), "srv/1")).expect("kept whole"), bytes);
    assert_eq!(frames_of(cache.open("srv/1", &address).expect("opened")), Ok(4_000));
    assert_eq!(server.requests().len(), 1, "the second time is played from the cache");
}

#[test]
fn a_slow_server_is_waited_for_and_the_track_plays_to_its_end() {
    let scratch = Scratch::new("stream-slow");
    let bytes = tone(&scratch, 0.5);
    let server = serving(bytes, |answer| Response { piece: Some((1_024, Duration::from_millis(3))), ..answer });
    let cache = StreamCache::new(scratch.path("cache"), 1 << 30);
    let source = cache.open("srv/slow", &format!("{}/s", server.url())).expect("opened");
    assert_eq!(frames_of(source), Ok(4_000));
}

#[test]
fn a_server_that_stops_halfway_is_an_error_and_nothing_half_is_kept() {
    let scratch = Scratch::new("stream-cut");
    let bytes = tone(&scratch, 0.5);
    let half = bytes.len() / 2;
    let server = serving(bytes, move |answer| Response { cut_at: Some(half), ..answer });
    let cache = StreamCache::new(scratch.path("cache"), 1 << 30).patience(Duration::from_secs(20));
    let started = Instant::now();
    let played = frames_of(cache.open("srv/cut", &format!("{}/s", server.url())).expect("opened"));
    assert!(played.is_err(), "a track cut short is not played as if whole: {played:?}");
    assert!(started.elapsed() < Duration::from_secs(10), "said at once, not after waiting out the patience");
    assert!(fs::read_dir(scratch.path("cache")).expect("folder").next().is_none(), "no half track is kept");
}

#[test]
fn a_server_gone_silent_is_given_up_after_the_patience() {
    let scratch = Scratch::new("stream-silent");
    // Long enough to need more than the first piece.
    let bytes = tone(&scratch, 4.0);
    // Two pieces, then a pause far longer than the patience.
    let server = serving(bytes, |answer| Response { piece: Some((16_384, Duration::from_secs(30))), ..answer });
    let cache = StreamCache::new(scratch.path("cache"), 1 << 30).patience(Duration::from_millis(500));
    let started = Instant::now();
    let played = frames_of(cache.open("srv/silent", &format!("{}/s", server.url())).expect("opened"));
    assert!(played.is_err(), "{played:?}");
    assert!(started.elapsed() < Duration::from_secs(20), "the wait has an end");
}

#[test]
fn a_track_the_server_does_not_have_is_an_error_that_does_not_name_the_address() {
    let scratch = Scratch::new("stream-missing");
    let server = FakeServer::start(|_| Response::status(404));
    let cache = StreamCache::new(scratch.path("cache"), 1 << 30);
    let address = format!("{}/rest/stream?id=9&t=token-that-is-secret", server.url());
    let error = cache.open("srv/9", &address).err().expect("not there");
    assert!(
        error.contains("404") && !error.contains("token-that-is-secret") && !error.contains("127.0.0.1"),
        "{error}"
    );
    let off = std::net::TcpListener::bind("127.0.0.1:0").expect("a port").local_addr().expect("its address");
    let error = cache.open("srv/10", &format!("http://{off}/rest/stream?t=token-that-is-secret")).err().expect("off");
    assert!(!error.contains("token-that-is-secret"), "{error}");
}

#[test]
fn a_place_far_into_the_track_is_reached_once_it_arrives() {
    let scratch = Scratch::new("stream-seek");
    let bytes = tone(&scratch, 2.0);
    let server = serving(bytes, |answer| Response { piece: Some((4_096, Duration::from_millis(2))), ..answer });
    let cache = StreamCache::new(scratch.path("cache"), 1 << 30);
    let source = cache.open("srv/seek", &format!("{}/s", server.url())).expect("opened");
    let mut decoder = Decoder::open_source(source, Some("wav")).expect("a decoder");
    decoder.seek(Duration::from_millis(1_500)).expect("reached");
    let mut out = Vec::new();
    while decoder.next_into(&mut out).expect("read") {}
    // The same place in the file on disk, entered the same way.
    let mut from_disk = Decoder::open(&scratch.path("made/tone.wav")).expect("a decoder");
    from_disk.seek(Duration::from_millis(1_500)).expect("reached");
    let mut expected = Vec::new();
    while from_disk.next_into(&mut expected).expect("read") {}
    assert_eq!(out.len(), expected.len(), "the place reached is the file's own");
    assert!(out.len() / 2 < 6_000, "and it is far into the track: {}", out.len() / 2);
}

#[test]
fn the_oldest_tracks_go_once_the_cache_is_over_its_size() {
    let scratch = Scratch::new("stream-trim");
    let bytes = tone(&scratch, 0.5);
    let size = bytes.len() as u64;
    let server = serving(bytes, |answer| answer);
    // Room for two tracks.
    let cache = StreamCache::new(scratch.path("cache"), size * 2 + size / 2);
    // A file the cache did not fetch, old and large, which is never the cache's to remove.
    let foreign = scratch.path("cache/Bir Derdim Var.flac");
    fs::create_dir_all(scratch.path("cache")).expect("folder");
    fs::write(&foreign, vec![7; usize::try_from(size * 3).expect("size")]).expect("written");
    File::options().write(true).open(&foreign).expect("open").set_modified(SystemTime::UNIX_EPOCH).expect("aged");
    for (at, key) in ["a", "b", "c", "d"].iter().enumerate() {
        frames_of(cache.open(key, &format!("{}/s?id={at}", server.url())).expect("opened")).expect("played");
        kept_whole(&scratch.path("cache"), key);
        // A clock that tells files written in the same instant apart.
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(foreign.exists(), "a file the cache did not fetch stays");
    let kept: Vec<_> =
        ["a", "b", "c", "d"].iter().map(|key| scratch.path("cache").join(name_of(key)).exists()).collect();
    assert_eq!(kept, [false, true, true, true], "before each new track the oldest go down to the size");
}

#[test]
fn a_reader_put_down_stops_the_fetch() {
    let scratch = Scratch::new("stream-dropped");
    let bytes = tone(&scratch, 4.0);
    let server = serving(bytes, |answer| Response { piece: Some((PIECE, Duration::from_millis(50))), ..answer });
    let cache = StreamCache::new(scratch.path("cache"), 1 << 30);
    let source = cache.open("srv/drop", &format!("{}/s", server.url())).expect("opened");
    drop(source);
    let partial = scratch.path("cache").join(name_of("srv/drop")).with_extension(PARTIAL);
    let started = Instant::now();
    while partial.exists() {
        assert!(started.elapsed() < Duration::from_secs(20), "the fetch never stopped");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!scratch.path("cache").join(name_of("srv/drop")).exists(), "a track not fetched whole is not kept");
}

/// A body that hands over its bytes and, with the last of them, says the reader has gone: the
/// moment a decoder that knows the length stops, before the end of the body is read.
struct LeftAtTheEnd {
    bytes: std::io::Cursor<Vec<u8>>,
    shared: std::sync::Arc<Shared>,
}

impl Read for LeftAtTheEnd {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.bytes.read(buffer)?;
        if self.bytes.position() == self.bytes.get_ref().len() as u64 {
            self.shared.unwanted.store(true, Ordering::Relaxed);
        }
        Ok(read)
    }
}

#[test]
fn a_track_whose_last_byte_has_arrived_is_kept_even_when_the_reader_is_gone_before_the_end() {
    let scratch = Scratch::new("stream-finished");
    let bytes = tone(&scratch, 0.5);
    let shared = std::sync::Arc::new(Shared::default());
    shared.lock().total = Some(bytes.len() as u64);
    let folder = scratch.path("cache");
    fs::create_dir_all(&folder).expect("folder");
    let (partial, whole) = (folder.join("t.part"), folder.join("t"));
    let file = File::create(&partial).expect("file");
    let body = LeftAtTheEnd { bytes: std::io::Cursor::new(bytes.clone()), shared: std::sync::Arc::clone(&shared) };
    fetch(body, file, &shared, &partial, &whole);
    assert_eq!(fs::read(&whole).expect("kept"), bytes);
    assert!(matches!(shared.lock().ended, Some(Ok(()))));
}
