//! The `file://` URLs MPRIS carries. A path of the person's music has spaces in it and letters
//! outside ASCII, and neither survives a URL as it stands, so both directions are here: a path
//! that goes out as a URL, and a URL that comes back as a path.

use std::path::{Path, PathBuf};

/// The scheme of the URLs qmus speaks, and the only one it opens.
const SCHEME: &str = "file://";

/// The characters a URL may carry as they stand. Everything else is written as the number of its
/// byte, so that a path with a space, a `ç` or a `#` in it survives the trip.
fn plain(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/')
}

/// The path at `path` as a `file://` URL.
pub(super) fn url_of(path: &Path) -> String {
    let mut url = String::from(SCHEME);
    for byte in path.as_os_str().as_encoded_bytes() {
        if plain(*byte) {
            url.push(char::from(*byte));
        } else {
            url.push('%');
            url.push_str(&format!("{byte:02X}"));
        }
    }
    url
}

/// The path in the `file://` URL `url`, when it is one qmus can open: a local file, named in
/// whole bytes. A URL for anything else, or one naming a path that is not text, is refused.
pub(super) fn path_of(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix(SCHEME)?;
    // The form every client sends is `file:///path`; a URL that names a machine instead of a file
    // says nothing about which file of ours to play.
    if !rest.starts_with('/') {
        return None;
    }
    let bytes = unescape(rest.as_bytes())?;
    Some(PathBuf::from(String::from_utf8(bytes).ok()?))
}

/// The bytes of a URL with every `%XX` replaced by the byte it names.
fn unescape(url: &[u8]) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(url.len());
    let mut rest = url;
    while let Some(at) = rest.iter().position(|byte| *byte == b'%') {
        bytes.extend_from_slice(&rest[..at]);
        let number = std::str::from_utf8(rest.get(at + 1..at + 3)?).ok()?;
        bytes.push(u8::from_str_radix(number, 16).ok()?);
        rest = rest.get(at + 3..)?;
    }
    bytes.extend_from_slice(rest);
    Some(bytes)
}
