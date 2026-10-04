//! The person's accounts: which services qmus plays from beside this computer, and how to reach
//! them.
//!
//! What is kept on disk is only what says where an account is and who logs in: its kind, the name
//! the person gave it, the address and the user name. A password or a key is never written to a
//! file. Until the system's keyring can hold them, they live in memory while qmus is open and are
//! asked for again on the next start.

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::net::IpAddr;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::sources::Client;

/// The name of the accounts file in the shared Quvyta folder, beside `music.conf`.
pub const FILE: &str = "music.accounts";

/// The kinds of account qmus can play from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A music server of the person's own that speaks the Subsonic API.
    #[default]
    Subsonic,
    /// A Jellyfin server of the person's own.
    Jellyfin,
    /// A Spotify Premium account.
    Spotify,
}

/// How the person logs in to an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Login {
    /// A user name and a password.
    #[default]
    Password,
    /// A key the service gave.
    ApiKey,
}

/// One account, as the accounts file keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// The key it is known by: made once when it is added and never changed, so tracks and
    /// playlists still name it after the person renames it.
    pub key: String,
    /// What kind of service it is.
    pub kind: Kind,
    /// The name the person gave it.
    pub name: String,
    /// Where the service is.
    pub address: String,
    /// The person's name there; empty when logging in with a key.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user: String,
    /// How the person logs in.
    #[serde(default)]
    pub login: Login,
    /// Whether the tracks heard are reported to the service for its own play counts; written to
    /// the file only when turned off.
    #[serde(default = "on", skip_serializing_if = "is_on")]
    pub scrobble: bool,
}

/// The value a switch has until the person turns it off.
fn on() -> bool {
    true
}

/// Whether a switch is as it is until turned off, so the file need not say it.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_on(value: &bool) -> bool {
    *value
}

/// The accounts file: its accounts in the order they were added.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Written {
    /// The accounts.
    #[serde(default, rename = "account")]
    accounts: Vec<Account>,
}

/// The accounts kept in `file`: none when there is no such file.
///
/// # Errors
///
/// When the file is there but cannot be read or is not an accounts file. The caller then leaves it
/// alone rather than writing over what the person had.
pub fn load(file: &Path) -> io::Result<Vec<Account>> {
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let written: Written =
        toml::from_str(&text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    Ok(written.accounts)
}

/// Keeps `accounts` in `file`, whole or not at all; no file once there are none.
///
/// # Errors
///
/// When the folder or the file cannot be written.
pub fn save(file: &Path, accounts: &[Account]) -> io::Result<()> {
    if accounts.is_empty() {
        return match fs::remove_file(file) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    }
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder)?;
    }
    let text = toml::to_string(&Written { accounts: accounts.to_vec() })
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    qframe::storage::atomic_write(file, text.as_bytes())
}

/// A key for a new account of `kind` that none of `taken` has.
#[must_use]
pub fn new_key(kind: Kind, taken: &[Account]) -> String {
    let prefix = match kind {
        Kind::Subsonic => "subsonic",
        Kind::Jellyfin => "jellyfin",
        Kind::Spotify => "spotify",
    };
    let mut hasher = RandomState::new().build_hasher();
    let free = |key: &String| taken.iter().all(|account| &account.key != key);
    for _ in 0..64 {
        hasher.write_usize(taken.len());
        let key = format!("{prefix}-{:08x}", hasher.finish() & 0xffff_ffff);
        if free(&key) {
            return key;
        }
    }
    // Numbered keys after that: among one more number than there are accounts, one is free.
    (0..=taken.len())
        .map(|number| format!("{prefix}-{number}"))
        .find(|key| free(key))
        .unwrap_or_else(|| prefix.to_owned())
}

/// The accounts logged in to in this run, each with its login or session, held in memory while
/// qmus is open and never written.
#[derive(Default)]
pub struct Logins(HashMap<String, Client>);

impl std::fmt::Debug for Logins {
    // Which accounts have a login is fine to print; the logins themselves are not.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_set().entries(self.0.keys()).finish()
    }
}

impl Logins {
    /// The account `key` logged in to, when the person logged in to it in this run.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Client> {
        self.0.get(key)
    }

    /// Remembers the account `key` logged in to as `client` until qmus closes.
    pub fn keep(&mut self, key: &str, client: Client) {
        self.0.insert(key.to_owned(), client);
    }

    /// Forgets the login of the account `key`.
    pub fn forget(&mut self, key: &str) {
        self.0.remove(key);
    }
}

/// Whether a login sent to `address` would cross a network as readable text: a plain `http://`
/// address outside this computer and the home network. A home server is usually reached that
/// way, so it is said rather than refused.
#[must_use]
pub fn unencrypted(address: &str) -> bool {
    let Some(rest) = address.trim().strip_prefix("http://") else { return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = match authority.strip_prefix('[') {
        Some(bracketed) => bracketed.split(']').next().unwrap_or_default(),
        None => authority.rsplit_once(':').map_or(authority, |(host, _)| host),
    };
    let host = host.to_ascii_lowercase();
    if host.is_empty()
        || host == "localhost"
        || host.ends_with(".local")
        || host.ends_with(".lan")
        || host.ends_with(".home.arpa")
    {
        return false;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => !(ip.is_loopback() || ip.is_private() || ip.is_link_local()),
        Ok(IpAddr::V6(ip)) => !(ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local()),
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Scratch;

    fn account(key: &str, name: &str) -> Account {
        Account {
            key: key.to_owned(),
            kind: Kind::Subsonic,
            name: name.to_owned(),
            address: "http://192.168.1.5:4533".to_owned(),
            user: "hakan".to_owned(),
            login: Login::Password,
            scrobble: true,
        }
    }

    #[test]
    fn accounts_kept_come_back_as_they_were_and_none_leaves_no_file() {
        let scratch = Scratch::new("accounts-roundtrip");
        let file = scratch.path("config").join(FILE);
        assert_eq!(load(&file).expect("no file is no accounts"), []);
        let kept = [
            account("subsonic-0001", "Ev sunucusu"),
            Account { login: Login::ApiKey, user: String::new(), ..account("subsonic-0002", "Ofis") },
        ];
        save(&file, &kept).expect("saved");
        assert_eq!(load(&file).expect("read"), kept);
        save(&file, &[]).expect("saved");
        assert!(!file.exists(), "no accounts, no file");
    }

    #[test]
    fn a_file_that_is_not_an_accounts_file_is_an_error_and_not_an_empty_list() {
        let scratch = Scratch::new("accounts-broken");
        let file = scratch.path("config").join(FILE);
        fs::create_dir_all(file.parent().expect("folder")).expect("folder");
        fs::write(&file, "[[account]]\nkey = 3\n").expect("written");
        assert!(load(&file).is_err(), "an empty list would let the next save write over it");
    }

    #[test]
    fn a_new_key_is_one_no_account_has() {
        let taken: Vec<Account> = (0..200).map(|at| account(&new_key(Kind::Subsonic, &[]), &at.to_string())).collect();
        for _ in 0..50 {
            let key = new_key(Kind::Subsonic, &taken);
            assert!(key.starts_with("subsonic-"));
            assert!(taken.iter().all(|account| account.key != key));
        }
    }

    #[test]
    fn the_logins_are_never_printed() {
        let mut logins = Logins::default();
        let server = crate::sources::subsonic::Subsonic::new(
            "http://192.168.1.5:4533",
            "hakan",
            crate::sources::subsonic::Auth::Password("gizli".to_owned()),
        )
        .expect("a server");
        logins.keep("subsonic-0001", Client::Subsonic(server));
        let printed = format!("{logins:?}");
        assert!(printed.contains("subsonic-0001") && !printed.contains("gizli"), "{printed}");
    }

    #[test]
    fn only_plain_addresses_beyond_the_home_network_are_unencrypted() {
        for address in [
            "http://music.example.org",
            "http://music.example.org:4533/navidrome",
            "http://8.8.8.8",
            "http://[2001:db8::1]:4533",
        ] {
            assert!(unencrypted(address), "{address}");
        }
        for address in [
            "https://music.example.org",
            "http://127.0.0.1:4533",
            "http://localhost:4533",
            "http://192.168.1.5:4533",
            "http://10.0.0.2",
            "http://172.20.1.1",
            "http://nas.local:4533",
            "http://[::1]:4533",
            "http://[fd00::5]",
        ] {
            assert!(!unencrypted(address), "{address}");
        }
    }
}
