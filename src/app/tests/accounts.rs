use super::*;
use crate::testing::server::{FakeServer, Response};

/// The password the tests log in with; it must never reach a file.
pub(super) const PASSWORD: &str = "gizli-parola-77";

/// A server that takes any login.
fn welcoming() -> FakeServer {
    FakeServer::start(|_| Response::json(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#))
}

/// A server that turns every login away.
fn refusing() -> FakeServer {
    FakeServer::start(|_| {
        Response::json(
            r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":40,"message":"Wrong username or password"}}}"#,
        )
    })
}

/// Clicks the first place `text` shows on screen: the first kind's button among several.
pub(super) fn click_first(h: &mut Harness<Music>, text: &str) {
    let (x, y) = find(h, text).unwrap_or_else(|| panic!("no {text}:\n{}", h.screen()));
    h.click(x, y);
}

/// Clicks the last place `text` shows on screen.
pub(super) fn click_last(h: &mut Harness<Music>, text: &str) {
    let screen = h.screen();
    let (column, row) = screen
        .lines()
        .enumerate()
        .filter_map(|(row, line)| line.rfind(text).map(|at| (line[..at].chars().count(), row)))
        .last()
        .unwrap_or_else(|| panic!("no {text}:\n{screen}"));
    h.click(i32::try_from(column).expect("a column"), i32::try_from(row).expect("a row"));
}

/// The screen with its settings open, tall enough for the accounts.
pub(super) fn settings_of(scratch: &Scratch) -> Harness<Music> {
    let folder = albums(scratch);
    let mut h = open(scratch, &folder);
    h.resize(110, 60);
    click_icon(&mut h, "settings");
    settle(&mut h);
    h
}

/// Fills the add dialog, which has just opened, and presses Connect.
pub(super) fn fill_in(h: &mut Harness<Music>, name: &str, address: &str, password: &str) {
    h.type_text(name);
    h.press("tab");
    h.type_text(address);
    h.press("tab");
    // The way of logging in keeps its password.
    h.press("tab");
    h.type_text("hakan");
    h.press("tab");
    h.type_text(password);
    h.press("enter");
}

/// What the accounts file holds; empty when there is none.
pub(super) fn kept(scratch: &Scratch) -> String {
    std::fs::read_to_string(scratch.path("config").join(crate::accounts::FILE)).unwrap_or_default()
}

#[test]
fn a_server_added_from_the_settings_is_connected_and_its_password_is_written_nowhere() {
    let scratch = Scratch::new("accounts-add");
    let server = welcoming();
    let mut h = settings_of(&scratch);
    click_first(&mut h, "Add account");
    settle(&mut h);
    fill_in(&mut h, "Ev sunucusu", &server.url(), PASSWORD);
    wait_for(&mut h, |music| music.account_dialog.is_none());
    settle(&mut h);
    let screen = h.screen();
    assert!(screen.contains("Ev sunucusu") && screen.contains("connected"), "{screen}");
    let ping = &server.requests()[0];
    assert_eq!(
        (ping.path.as_str(), ping.get("u")),
        ("/rest/ping", Some("hakan")),
        "the server was asked with what was typed"
    );
    let file = kept(&scratch);
    assert!(file.contains("Ev sunucusu") && file.contains(&server.url()) && file.contains("hakan"), "{file}");
    assert!(!file.contains(PASSWORD), "the password is never written: {file}");
    for entry in walk(&scratch.path("")) {
        if let Ok(text) = std::fs::read_to_string(&entry) {
            assert!(!text.contains(PASSWORD), "{} holds the password", entry.display());
        }
    }
}

#[test]
fn a_login_turned_away_is_said_in_the_dialog_and_nothing_is_kept() {
    let scratch = Scratch::new("accounts-refused");
    let server = refusing();
    let mut h = settings_of(&scratch);
    click_first(&mut h, "Add account");
    settle(&mut h);
    fill_in(&mut h, "Ev", &server.url(), "yanlış");
    wait_for(&mut h, |music| music.account_dialog.as_ref().is_some_and(|dialog| dialog.problem.is_some()));
    assert!(h.screen().contains("did not accept this login"), "{}", h.screen());
    assert!(kept(&scratch).is_empty(), "an account that never connected is not kept");
    h.press("esc");
    settle(&mut h);
    assert!(h.app().accounts.is_empty() && h.app().account_dialog.is_none());
}

#[test]
fn an_account_of_the_last_run_asks_for_its_password_and_connects_once_given_it() {
    let scratch = Scratch::new("accounts-again");
    let server = welcoming();
    std::fs::create_dir_all(scratch.path("config")).expect("folder");
    let file = format!(
        "[[account]]\nkey = \"subsonic-1a2b\"\nkind = \"subsonic\"\nname = \"Ev sunucusu\"\naddress = \"{}\"\nuser = \"hakan\"\n",
        server.url()
    );
    std::fs::write(scratch.path("config").join(crate::accounts::FILE), &file).expect("written");
    let mut h = settings_of(&scratch);
    assert!(h.screen().contains("password needed"), "{}", h.screen());
    click_last(&mut h, "Log in");
    settle(&mut h);
    // Only the password is asked: the rest is known.
    h.type_text(PASSWORD);
    h.press("enter");
    wait_for(&mut h, |music| music.account_dialog.is_none());
    settle(&mut h);
    assert!(h.screen().contains("connected") && !h.screen().contains("password needed"), "{}", h.screen());
    let ping = &server.requests()[0];
    assert_eq!(ping.get("t").map(str::len), Some(32), "a token, not the password");
    assert_eq!(kept(&scratch), file, "logging in again changes nothing that is kept");
}

#[test]
fn an_account_is_removed_only_after_yes_and_the_file_forgets_it() {
    let scratch = Scratch::new("accounts-remove");
    std::fs::create_dir_all(scratch.path("config")).expect("folder");
    std::fs::write(
        scratch.path("config").join(crate::accounts::FILE),
        "[[account]]\nkey = \"subsonic-1a2b\"\nkind = \"subsonic\"\nname = \"Ev sunucusu\"\naddress = \"http://127.0.0.1:1\"\nuser = \"hakan\"\n",
    )
    .expect("written");
    let mut h = settings_of(&scratch);
    click_last(&mut h, "Remove");
    settle(&mut h);
    assert!(h.screen().contains("Remove Ev sunucusu?"), "{}", h.screen());
    click_last(&mut h, "Cancel");
    settle(&mut h);
    assert_eq!(h.app().accounts.len(), 1, "no is no");
    assert!(!kept(&scratch).is_empty());
    click_last(&mut h, "Remove");
    settle(&mut h);
    // The dialog's own Remove, below the row's.
    click_last(&mut h, "Remove");
    settle(&mut h);
    assert!(h.app().accounts.is_empty(), "{}", h.screen());
    assert!(kept(&scratch).is_empty(), "the file forgets it");
}

#[test]
fn a_plain_address_beyond_the_home_network_is_said_to_be_unencrypted_and_a_home_one_is_not() {
    let scratch = Scratch::new("accounts-unencrypted");
    let mut h = settings_of(&scratch);
    click_first(&mut h, "Add account");
    settle(&mut h);
    h.press("tab");
    h.type_text("http://192.168.1.5:4533");
    settle(&mut h);
    assert!(!h.screen().contains("not encrypted"), "{}", h.screen());
    h.press("ctrl+a");
    h.type_text("http://music.example.org");
    settle(&mut h);
    assert!(h.screen().contains("not encrypted"), "{}", h.screen());
}

#[test]
fn an_accounts_file_that_cannot_be_read_is_never_written_over() {
    let scratch = Scratch::new("accounts-unreadable");
    let server = welcoming();
    std::fs::create_dir_all(scratch.path("config")).expect("folder");
    let broken = "[[account]]\nkey = 3\n# the person's own words\n";
    std::fs::write(scratch.path("config").join(crate::accounts::FILE), broken).expect("written");
    let mut h = settings_of(&scratch);
    click_first(&mut h, "Add account");
    settle(&mut h);
    fill_in(&mut h, "Ev", &server.url(), PASSWORD);
    wait_for(&mut h, |music| music.account_dialog.is_none());
    assert_eq!(kept(&scratch), broken, "what the person had is left as it was");
}

/// Every file under `root`.
fn walk(root: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found
}
