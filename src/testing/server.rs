//! A small web server on this machine's loopback address, for tests of the sources that talk to
//! one: it answers each request with what the test says and keeps every request it was sent, so a
//! test can see what qmus asked as well as what it did with the answer. Nothing leaves the machine.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A request the server was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The method: `GET`, `POST` and the like.
    pub method: String,
    /// The path, without the query.
    pub path: String,
    /// The query's fields, decoded, in the order they came.
    pub query: Vec<(String, String)>,
    /// The headers, their names in lower case.
    pub headers: HashMap<String, String>,
    /// The body, as text.
    pub body: String,
}

impl Request {
    /// The first value the query gives `name`.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.query.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }
}

/// What the server answers.
#[derive(Debug, Clone)]
pub struct Response {
    /// The status code.
    pub status: u16,
    /// Headers beyond the length.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Vec<u8>,
    /// The body is sent in pieces of this many bytes with `pause` between them, for a slow server.
    pub piece: Option<(usize, Duration)>,
    /// The connection is closed after this many bytes of the body, for a server that goes away.
    pub cut_at: Option<usize>,
}

impl Response {
    /// A 200 answer of `body` as JSON.
    pub fn json(body: &str) -> Self {
        Self::bytes(body.as_bytes().to_vec()).header("Content-Type", "application/json")
    }

    /// A 200 answer of `body`.
    pub fn bytes(body: Vec<u8>) -> Self {
        Self { status: 200, headers: Vec::new(), body, piece: None, cut_at: None }
    }

    /// An empty answer with `status`.
    pub fn status(status: u16) -> Self {
        Self { status, ..Self::bytes(Vec::new()) }
    }

    /// The same answer with one more header.
    #[must_use]
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }
}

/// The answers, by what was asked.
type Answer = dyn Fn(&Request) -> Response + Send + Sync;

/// The server: it runs until the test ends.
pub struct FakeServer {
    /// Where it listens.
    address: SocketAddr,
    /// Every request it was sent, in order.
    requests: Arc<Mutex<Vec<Request>>>,
}

impl FakeServer {
    /// Starts a server on a free port of the loopback address that answers with `answer`.
    pub fn start(answer: impl Fn(&Request) -> Response + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free port on the loopback address");
        let address = listener.local_addr().expect("the address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&requests);
        let answer: Arc<Answer> = Arc::new(answer);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let kept = Arc::clone(&kept);
                let answer = Arc::clone(&answer);
                std::thread::spawn(move || serve(stream, &kept, answer.as_ref()));
            }
        });
        Self { address, requests }
    }

    /// The address to give a client, without a slash at the end.
    pub fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// Every request sent so far.
    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().expect("the requests").clone()
    }
}

/// Reads one request off `stream`, keeps it and writes the answer.
fn serve(stream: TcpStream, kept: &Mutex<Vec<Request>>, answer: &Answer) {
    let mut reader = BufReader::new(stream.try_clone().expect("the stream"));
    let mut first = String::new();
    if reader.read_line(&mut first).is_err() {
        return;
    }
    let method = first.split_whitespace().next().unwrap_or("GET").to_owned();
    let target = first.split_whitespace().nth(1).unwrap_or("/").to_owned();
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_lowercase(), value.trim().to_owned());
        }
    }
    let length = headers.get("content-length").and_then(|length| length.parse::<usize>().ok()).unwrap_or(0);
    let mut body = vec![0; length];
    if std::io::Read::read_exact(&mut reader, &mut body).is_err() {
        return;
    }
    let body = String::from_utf8_lossy(&body).into_owned();
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let query = query
        .split('&')
        .filter(|field| !field.is_empty())
        .map(|field| {
            let (key, value) = field.split_once('=').unwrap_or((field, ""));
            (decode(key), decode(value))
        })
        .collect();
    let request = Request { method, path: path.to_owned(), query, headers, body };
    kept.lock().expect("the requests").push(request.clone());
    let response = answer(&request);
    let _ = write(stream, &response);
}

/// Writes `response` and closes the connection.
fn write(mut stream: TcpStream, response: &Response) -> std::io::Result<()> {
    let mut head =
        format!("HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n", response.status, response.body.len());
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    let body = &response.body[..response.cut_at.unwrap_or(response.body.len()).min(response.body.len())];
    match response.piece {
        Some((size, pause)) => {
            for piece in body.chunks(size.max(1)) {
                stream.write_all(piece)?;
                stream.flush()?;
                std::thread::sleep(pause);
            }
        }
        None => stream.write_all(body)?,
    }
    stream.flush()
}

/// `text` with its `%xx` escapes and `+` turned back into what they stand for.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'%' if at + 2 < bytes.len() => {
                let hex =
                    std::str::from_utf8(&bytes[at + 1..at + 3]).ok().and_then(|hex| u8::from_str_radix(hex, 16).ok());
                match hex {
                    Some(byte) => {
                        out.push(byte);
                        at += 3;
                    }
                    None => {
                        out.push(b'%');
                        at += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                at += 1;
            }
            byte => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
