//! The tests of the MPRIS server. They never touch the person's session bus: a socket pair with
//! zbus at both ends stands in for it, one end serving and the other asking exactly as any MPRIS
//! client does.
//!
//! Nothing here plays sound either. A test asks the server what a desktop would ask it, and looks
//! at what comes back.

mod bus;
mod calls;
mod properties;
mod updates;

use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use zbus::MatchRule;
use zbus::blocking::connection::Builder;
use zbus::blocking::proxy::Builder as ProxyBuilder;
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::message::{Message, Type};
use zbus::proxy::CacheProperties;
use zbus::zvariant::{OwnedValue, Value};

use super::{Now, Playback, Request, Server};

/// The bus name a client on the bus would look qmus up by.
const DESTINATION: &str = "org.mpris.MediaPlayer2.qmus";

/// The interface that says what qmus is, as a client names it.
const IDENTITY: &str = "org.mpris.MediaPlayer2";

/// The interface that says what qmus is doing, as a client names it.
const PLAYBACK: &str = "org.mpris.MediaPlayer2.Player";

/// Long enough for a signal or a reply on a machine that is busy building something else.
const GENEROUS: Duration = Duration::from_secs(20);

/// A server on one end of a socket pair, a client on the other, and a note of every request the
/// server handed on to qmus.
pub(super) struct Pair {
    /// The server under test.
    pub(super) server: Server,
    /// The client that asks it things.
    pub(super) client: Client,
    /// What qmus was asked for.
    pub(super) asked: Asked,
}

impl Pair {
    /// A connection pair with the MPRIS server on one end and a client on the other.
    pub(super) fn new() -> Self {
        let (served, client) = ends();
        let asked = Asked::default();
        let server = Server::on(served, asked.collect()).expect("the MPRIS object is served");
        Self { server, client: client.join().expect("the client end connects"), asked }
    }

    /// A pair whose server is already saying that a track is heard.
    pub(super) fn playing(track: u64) -> Self {
        let pair = Self::new();
        pair.server.update(&Now { id: track, status: Playback::Playing, ..Now::default() });
        pair
    }
}

/// The two ends of a socket pair: the one a server is to be served on, and the client at the
/// other, which greets the server from a thread of its own and is ready once the server has
/// greeted back.
pub(crate) fn ends() -> (UnixStream, std::thread::JoinHandle<Client>) {
    let (served, asking) = UnixStream::pair().expect("a socket pair");
    let client = std::thread::spawn(move || {
        Client::new(&Builder::async_io_unix_stream(asking).p2p().build().expect("the client end connects"))
    });
    (served, client)
}

/// A client on the other end of the pair: what any MPRIS client does, done by hand.
pub(crate) struct Client {
    /// The interface that says what qmus is.
    identity: Proxy<'static>,
    /// The interface that says what qmus is doing.
    player: Proxy<'static>,
    /// Every signal on the object, as they arrive.
    signals: Receiver<Message>,
}

impl Client {
    /// A client for the server at the other end of `connection`.
    fn new(connection: &Connection) -> Self {
        let signals = listen(connection);
        Self { identity: proxy(connection, IDENTITY), player: proxy(connection, PLAYBACK), signals }
    }

    /// Calls the method `member` of the player interface, which takes nothing, and waits for the
    /// answer.
    pub(crate) fn call(&self, member: &str) -> zbus::Result<()> {
        self.player.call_method(member, &()).map(|_| ())
    }

    /// Calls the method `member` of the player interface with the arguments `body`, which go on
    /// the bus as the values they are, and waits for the answer.
    pub(crate) fn call_with<Body>(&self, member: &str, body: &Body) -> zbus::Result<()>
    where
        Body: serde::Serialize + zbus::zvariant::DynamicType,
    {
        self.player.call_method(member, body).map(|_| ())
    }

    /// Reads `property` of the player interface.
    pub(crate) fn get<Wanted>(&self, property: &str) -> Wanted
    where
        Wanted: TryFrom<OwnedValue>,
        <Wanted as TryFrom<OwnedValue>>::Error: Into<zbus::Error>,
    {
        self.player.get_property(property).expect("the property is answered")
    }

    /// Reads `property` of the interface that says what qmus is.
    pub(crate) fn identity<Wanted>(&self, property: &str) -> Wanted
    where
        Wanted: TryFrom<OwnedValue>,
        <Wanted as TryFrom<OwnedValue>>::Error: Into<zbus::Error>,
    {
        self.identity.get_property(property).expect("the property is answered")
    }

    /// Calls the method `member` of the interface that says what qmus is, which takes nothing, and
    /// waits for the answer.
    pub(crate) fn identity_call(&self, member: &str) -> zbus::Result<()> {
        self.identity.call_method(member, &()).map(|_| ())
    }

    /// Writes `property` of the player interface, as a client does when the sound or the queue is
    /// changed from the outside.
    pub(crate) fn set(&self, property: &str, value: impl Into<Value<'static>> + 'static) -> zbus::fdo::Result<()> {
        self.player.set_property(property, value)
    }

    /// The next signal to arrive at the object.
    pub(crate) fn next_signal(&self) -> Message {
        self.signals.recv_timeout(GENEROUS).expect("the signal comes")
    }

    /// Whether the object keeps quiet. A signal that was never going to be sent is not going to be
    /// sent within a second either, and a second is short beside the wait above.
    pub(crate) fn quiet(&self) -> bool {
        self.signals.recv_timeout(Duration::from_secs(1)).is_err()
    }
}

/// A proxy for `interface` of the MPRIS object that reads every property from the server instead of
/// from a cache, so that a test sees what the wire carries and not what it carried a moment ago.
fn proxy(connection: &Connection, interface: &'static str) -> Proxy<'static> {
    ProxyBuilder::new(connection)
        .destination(DESTINATION)
        .expect("the bus name is one a client may look up")
        .path(super::PATH)
        .expect("the object path is the one MPRIS names")
        .interface(interface)
        .expect("the interface name is well known")
        .cache_properties(CacheProperties::No)
        .build()
        .expect("the object answers")
}

/// Every signal on the MPRIS object, handed over one at a time. An iterator of zbus' own waits for
/// the next signal forever, which no test may do, so the waiting happens on a thread of its own.
fn listen(connection: &Connection) -> Receiver<Message> {
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .path(super::PATH)
        .expect("the object path is the one MPRIS names")
        .build();
    let signals = MessageIterator::for_match_rule(rule, connection, None).expect("signals are listened for");
    let (outbox, arriving) = mpsc::channel();
    std::thread::spawn(move || {
        for signal in signals.flatten() {
            if outbox.send(signal).is_err() {
                return;
            }
        }
    });
    arriving
}

/// Where the requests the server hands on to qmus are kept, so that a test can ask what came out
/// of the other end.
#[derive(Clone, Default)]
pub(super) struct Asked(Arc<Mutex<Vec<Request>>>);

impl Asked {
    /// The way for the server to hand its calls here.
    fn collect(&self) -> impl Fn(Request) + Send + Sync + 'static {
        let asked = self.clone();
        move |request| asked.lock().push(request)
    }

    /// Everything qmus has been asked for since the last time this was read.
    pub(super) fn take(&self) -> Vec<Request> {
        std::mem::take(&mut self.lock())
    }

    /// The requests, under their lock.
    fn lock(&self) -> MutexGuard<'_, Vec<Request>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The text a client reads out of a value on the bus.
pub(crate) fn text(value: &OwnedValue) -> &str {
    match &**value {
        Value::Str(text) => text.as_str(),
        Value::ObjectPath(path) => path.as_str(),
        _ => panic!("text on the bus, not {value:?}"),
    }
}

/// The number a client reads out of a value on the bus.
pub(crate) fn number(value: &OwnedValue) -> f64 {
    f64::try_from(value).unwrap_or_else(|_| panic!("a number on the bus, not {value:?}"))
}

/// Whether a client reads true out of a value on the bus.
pub(crate) fn yes(value: &OwnedValue) -> bool {
    bool::try_from(value).unwrap_or_else(|_| panic!("a yes or a no on the bus, not {value:?}"))
}

/// The values a piece of metadata carries, for a test that reads what a client would read.
pub(crate) fn entries(metadata: &OwnedValue) -> HashMap<String, OwnedValue> {
    HashMap::try_from(Value::from(metadata.clone())).expect("the metadata is a dictionary of values")
}

/// The names of the properties a signal names, in an order a test can write down.
pub(crate) fn names(changed: &HashMap<String, OwnedValue>) -> Vec<&str> {
    let mut names: Vec<&str> = changed.keys().map(String::as_str).collect();
    names.sort_unstable();
    names
}

/// The three things a `PropertiesChanged` signal carries: the interface, what changed on it, and
/// what is no longer known.
pub(crate) fn changes(signal: &Message) -> (String, HashMap<String, OwnedValue>, Vec<String>) {
    signal.body().deserialize().expect("the signal carries the properties that changed")
}
