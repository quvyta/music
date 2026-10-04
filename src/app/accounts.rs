//! The settings page's accounts: the services the person plays from beside this computer, adding
//! one, logging in to one again, and removing one.
//!
//! Removing an account forgets it in qmus and nothing else: what the service holds is never
//! touched.

use qframe::prelude::*;
use qframe::widgets::{
    Button, Field, Modal, Segmented, SettingRow, SettingsRows, Switch, Text as Words, TextInput, Toast,
};

use super::{Msg, Music};
use crate::accounts::{self, Account, Kind, Login};
use crate::sources::Client;
use crate::sources::jellyfin::Jellyfin;
use crate::sources::subsonic::{Auth, Subsonic};
use crate::sources::{Reach, SourceError};

/// The first field of the account dialog, which has the focus when it opens.
const FIRST: &str = "account-first";

/// The account dialog's width: room for a long address.
const DIALOG: u16 = 64;

/// Where an account stands in this run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Standing {
    /// No password or key has been given since qmus opened.
    NeedsLogin,
    /// The service took the login.
    Connected,
    /// The last try failed: why.
    Failed(SourceError),
}

/// The dialog of an account, while it is open.
#[derive(Debug, Clone, Default)]
pub(super) struct Dialog {
    /// The account being logged in to again; `None` adds a new one.
    pub(super) existing: Option<String>,
    /// The kind of account.
    pub(super) kind: Kind,
    /// The key the account is known by, made when it is first tried.
    pub(super) key: Option<String>,
    /// The name typed.
    pub(super) name: String,
    /// The address typed.
    pub(super) address: String,
    /// The user name typed.
    pub(super) user: String,
    /// How the person logs in.
    pub(super) login: Login,
    /// The password or key typed; never written anywhere.
    pub(super) secret: String,
    /// Why the last try failed.
    pub(super) problem: Option<String>,
    /// Whether a try is on its way, and which: an answer to an older one is late.
    pub(super) trying: Option<u64>,
    /// The browser login under way, for Spotify.
    pub(super) browser: Option<BrowserLogin>,
}

/// A login in the person's browser, under way.
#[derive(Clone, Default)]
pub(super) struct BrowserLogin {
    /// The login page's address, shown so it can be opened elsewhere.
    pub(super) address: String,
    /// The secret and the state of the login.
    pub(super) pkce: Option<crate::sources::spotify::login::Pkce>,
    /// The address the browser ended on, pasted by the person when it could not come back here.
    pub(super) pasted: String,
    /// Set to end the wait for the browser: the login came back another way, or was given up.
    pub(super) stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl std::fmt::Debug for BrowserLogin {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("BrowserLogin").finish_non_exhaustive()
    }
}

/// Something on the accounts part of the settings page.
#[derive(Debug, Clone)]
pub enum AccountMsg {
    /// The dialog opens to add an account of this kind.
    Add(Kind),
    /// The dialog opens to log in to the account at this place again.
    LogIn(usize),
    /// The name typed.
    Name(String),
    /// The address typed.
    Address(String),
    /// The user name typed.
    User(String),
    /// The way of logging in chosen.
    Method(usize),
    /// The password or key typed.
    Secret(String),
    /// The address the browser ended on, pasted.
    Pasted(String),
    /// The pasted address is used to log in.
    PasteDone,
    /// The dialog's values are tried against the service.
    Connect,
    /// The service answered the try with this number: logged in, or why not.
    Answered(u64, Result<Client, SourceError>),
    /// The dialog closes with nothing done.
    Close,
    /// The person asked to remove the account at this place.
    RemoveAsk(usize),
    /// The person said yes.
    Remove,
    /// Reporting plays to the account at this place is turned on or off.
    Scrobble(usize, bool),
}

impl Music {
    /// Applies a change of the accounts part.
    pub(super) fn account_msg(&mut self, msg: AccountMsg) -> Command<Msg> {
        match msg {
            AccountMsg::Add(kind) => {
                // Jellyfin is logged in to with a password only.
                self.account_dialog = Some(Dialog { kind, ..Dialog::default() });
                return Command::focus(FIRST);
            }
            AccountMsg::LogIn(at) => {
                let Some(account) = self.accounts.get(at) else { return Command::none() };
                self.account_dialog = Some(Dialog {
                    existing: Some(account.key.clone()),
                    kind: account.kind,
                    name: account.name.clone(),
                    address: account.address.clone(),
                    user: account.user.clone(),
                    login: account.login,
                    ..Dialog::default()
                });
                return Command::focus(FIRST);
            }
            AccountMsg::Name(typed) => self.typed(|dialog| dialog.name = typed),
            AccountMsg::Address(typed) => self.typed(|dialog| dialog.address = typed),
            AccountMsg::User(typed) => self.typed(|dialog| dialog.user = typed),
            AccountMsg::Method(at) => {
                let login = if at == 1 { Login::ApiKey } else { Login::Password };
                self.typed(|dialog| dialog.login = login);
            }
            AccountMsg::Secret(typed) => self.typed(|dialog| dialog.secret = typed),
            AccountMsg::Pasted(typed) => self.typed(|dialog| {
                if let Some(browser) = &mut dialog.browser {
                    browser.pasted = typed;
                }
            }),
            AccountMsg::PasteDone => return self.pasted_login(),
            AccountMsg::Connect => return self.try_account(),
            AccountMsg::Answered(number, answer) => return self.answered(number, answer),
            AccountMsg::Close => {
                // A login given up frees the port the browser would have come back to.
                if let Some(browser) = self.account_dialog.as_ref().and_then(|dialog| dialog.browser.as_ref()) {
                    browser.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                self.account_dialog = None;
                self.removing = None;
            }
            AccountMsg::RemoveAsk(at) => {
                self.removing = self.accounts.get(at).map(|account| account.key.clone());
            }
            AccountMsg::Remove => return self.remove_account(),
            AccountMsg::Scrobble(at, on) => {
                let Some(account) = self.accounts.get_mut(at) else { return Command::none() };
                account.scrobble = on;
                return self.save_accounts();
            }
        }
        Command::none()
    }

    /// Changes the dialog with what was typed, which also clears the last try's problem.
    fn typed(&mut self, change: impl FnOnce(&mut Dialog)) {
        if let Some(dialog) = &mut self.account_dialog {
            change(dialog);
            dialog.problem = None;
        }
    }

    /// Tries the dialog's values against the service, off the drawing thread.
    fn try_account(&mut self) -> Command<Msg> {
        let Some(dialog) = &mut self.account_dialog else { return Command::none() };
        if dialog.trying.is_some() {
            return Command::none();
        }
        if dialog.kind == Kind::Spotify {
            return self.browser_login();
        }
        let Some(dialog) = &mut self.account_dialog else { return Command::none() };
        let user = dialog.user.trim();
        let missing = dialog.secret.is_empty() || (dialog.login == Login::Password && user.is_empty());
        if missing {
            dialog.problem = Some(t!("music.accounts.missing"));
            return Command::none();
        }
        // A new account's key is made now: Jellyfin knows this copy of qmus by it from the first login.
        let key = dialog.key.clone().or_else(|| dialog.existing.clone());
        let key = key.unwrap_or_else(|| accounts::new_key(dialog.kind, &self.accounts));
        dialog.key = Some(key.clone());
        let connect: Box<dyn FnOnce() -> Result<Client, SourceError> + Send> = match dialog.kind {
            Kind::Subsonic => match Subsonic::new(&dialog.address, user, auth_of(dialog)) {
                Ok(server) => Box::new(move || server.ping().map(|()| Client::Subsonic(server))),
                Err(error) => {
                    dialog.problem = Some(words(&error));
                    return Command::none();
                }
            },
            Kind::Jellyfin => {
                let (address, user, password) = (dialog.address.clone(), user.to_owned(), dialog.secret.clone());
                Box::new(move || Jellyfin::login(&address, &user, &password, &key).map(Client::Jellyfin))
            }
            Kind::Spotify => return Command::none(),
        };
        self.tries += 1;
        let number = self.tries;
        dialog.trying = Some(number);
        Command::perform(move || Msg::Accounts(AccountMsg::Answered(number, connect())))
    }

    /// Starts a login in the person's browser: Spotify's login page opens there, and qmus waits
    /// on this computer for the browser to come back.
    fn browser_login(&mut self) -> Command<Msg> {
        let Some(setup) = self.machine.spotify.clone() else { return Command::none() };
        let Some(dialog) = &mut self.account_dialog else { return Command::none() };
        let pkce = match crate::sources::spotify::login::Pkce::new() {
            Ok(pkce) => pkce,
            Err(error) => {
                dialog.problem = Some(words(&error));
                return Command::none();
            }
        };
        let Ok(listener) = setup.listen() else {
            dialog.problem = Some(t!("music.spotify.busy"));
            return Command::none();
        };
        if dialog.key.is_none() {
            dialog.key =
                Some(dialog.existing.clone().unwrap_or_else(|| accounts::new_key(Kind::Spotify, &self.accounts)));
        }
        let address = crate::sources::spotify::login::authorize_address(&setup.endpoints, &setup.client, &pkce);
        // A login started again ends the wait of the one before, which holds the port.
        if let Some(before) = &dialog.browser {
            before.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        dialog.browser = Some(BrowserLogin {
            address: address.clone(),
            pkce: Some(pkce.clone()),
            pasted: String::new(),
            stop: std::sync::Arc::clone(&stop),
        });
        self.tries += 1;
        let number = self.tries;
        dialog.trying = Some(number);
        // The wait for the browser runs beside the screen rather than as one piece of work: it may
        // last minutes, and the screen goes on meanwhile.
        let (done, answer) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = done.send(setup.log_in(&listener, &pkce, &stop));
        });
        let wait = Command::task(qframe::runtime::Task::new("spotify-login", move |cx| match cx.recv(&answer) {
            Some(answer) => Ok(Msg::Accounts(AccountMsg::Answered(number, answer))),
            None => Err("the login was left".to_owned()),
        }));
        Command::batch([Command::open(address), wait])
    }

    /// Logs in with the address the browser ended on, pasted by the person: for a browser on another
    /// machine, which cannot come back to this one.
    fn pasted_login(&mut self) -> Command<Msg> {
        let Some(setup) = self.machine.spotify.clone() else { return Command::none() };
        let Some(dialog) = &mut self.account_dialog else { return Command::none() };
        let Some(browser) = &dialog.browser else { return Command::none() };
        let Some(pkce) = browser.pkce.clone() else { return Command::none() };
        let code = match crate::sources::spotify::login::code_of(browser.pasted.trim(), &pkce.state) {
            Ok(code) => code,
            Err(_) => {
                dialog.problem = Some(t!("music.spotify.pasted-wrong"));
                return Command::none();
            }
        };
        // The login came back this way: the wait for the browser ends.
        browser.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        self.tries += 1;
        let number = self.tries;
        dialog.trying = Some(number);
        Command::perform(move || Msg::Accounts(AccountMsg::Answered(number, setup.finish(&code, &pkce.verifier))))
    }

    /// What the service said to the try `number`: the account is kept and logged in, or the
    /// dialog says why not.
    fn answered(&mut self, number: u64, answer: Result<Client, SourceError>) -> Command<Msg> {
        let Some(dialog) = &mut self.account_dialog else { return Command::none() };
        if dialog.trying != Some(number) {
            return Command::none();
        }
        dialog.trying = None;
        let client = match answer {
            Ok(client) => client,
            Err(error) => {
                dialog.problem = Some(words(&error));
                if let Some(key) = dialog.existing.clone() {
                    self.standing.insert(key, Standing::Failed(error));
                    // Its tracks go faint.
                    return self.relist();
                }
                return Command::none();
            }
        };
        let Some(dialog) = self.account_dialog.take() else { return Command::none() };
        let name = dialog.name.trim();
        let name = match (name.is_empty(), dialog.kind) {
            (false, _) => name.to_owned(),
            (true, Kind::Spotify) => "Spotify".to_owned(),
            (true, _) => host_of(&dialog.address),
        };
        let Some(key) = dialog.key.clone() else { return Command::none() };
        let account = Account {
            key: key.clone(),
            kind: dialog.kind,
            name: name.clone(),
            address: dialog.address.trim().trim_end_matches('/').to_owned(),
            user: if dialog.login == Login::Password { dialog.user.trim().to_owned() } else { String::new() },
            login: dialog.login,
            // Logging in again keeps what the person chose about reporting plays.
            scrobble: self.accounts.iter().find(|kept| kept.key == key).is_none_or(|kept| kept.scrobble),
        };
        // Logging in again changes nothing that is kept, so the file is only written for a change.
        let changed = match self.accounts.iter_mut().find(|kept| kept.key == key) {
            Some(kept) if *kept == account => false,
            Some(kept) => {
                *kept = account;
                true
            }
            None => {
                self.accounts.push(account);
                true
            }
        };
        self.logins.keep(&key, client);
        let catalogue = self.fetch_catalogue(&key);
        self.standing.insert(key, Standing::Connected);
        let saved = if changed { self.save_accounts() } else { Command::none() };
        Command::batch([
            saved,
            catalogue,
            Command::toast(Toast::success(t!("music.accounts.connected-toast", name = name.as_str())).key("account")),
        ])
    }

    /// Forgets the account the person said yes to, with its login; the service keeps everything.
    fn remove_account(&mut self) -> Command<Msg> {
        let Some(key) = self.removing.take() else { return Command::none() };
        self.accounts.retain(|account| account.key != key);
        self.logins.forget(&key);
        self.standing.remove(&key);
        if self.shown == super::pages::Shown::Account(key.clone()) {
            self.shown = super::pages::Shown::All;
        }
        Command::batch([self.save_accounts(), self.forget_listing(&key)])
    }

    /// Keeps the accounts file, unless it was there but could not be read: what the person had in
    /// it is not written over.
    fn save_accounts(&self) -> Command<Msg> {
        let Some(file) = self.accounts_file() else { return Command::none() };
        if self.accounts_unreadable {
            return Command::toast(Toast::warning(t!("music.accounts.unreadable")).key("accounts"));
        }
        match accounts::save(&file, &self.accounts) {
            Ok(()) => Command::none(),
            Err(error) => {
                Command::toast(Toast::warning(t!("music.accounts.not-saved")).body(error.to_string()).key("accounts"))
            }
        }
    }

    /// The accounts' rows: each account with where it stands, then a row for each kind that can be
    /// added.
    pub(super) fn account_rows(&self, list: &mut SettingsRows<'_, Msg>) {
        list.heading(t!("music.accounts.title"));
        for (at, account) in self.accounts.iter().enumerate() {
            let standing = self.standing.get(&account.key).cloned().unwrap_or(Standing::NeedsLogin);
            let said = match &standing {
                Standing::NeedsLogin => t!("music.accounts.needs-login"),
                Standing::Connected => t!("music.accounts.connected"),
                Standing::Failed(error) => words(error),
            };
            let place =
                if account.kind == Kind::Spotify { "Spotify Premium".to_owned() } else { account.address.clone() };
            let mut about = format!("{place} · {said}");
            if accounts::unencrypted(&account.address) {
                about = format!("{about} · {}", t!("music.accounts.unencrypted-short"));
            }
            list.row(SettingRow::new(account.name.clone()).description(about), |ui| {
                ui.row(|ui| {
                    // Spotify's history takes no word from another player, so there is nothing to turn off.
                    if account.kind != Kind::Spotify {
                        ui.add(
                            Switch::new(account.scrobble)
                                .label(t!("music.accounts.scrobble"))
                                .on_toggle(move |on| Msg::Accounts(AccountMsg::Scrobble(at, on))),
                        )
                        .id(format!("scrobble-{at}"));
                    }
                    if standing != Standing::Connected {
                        ui.add(Button::new(t!("music.accounts.log-in")).on_press(Msg::Accounts(AccountMsg::LogIn(at))))
                            .id(format!("log-in-{at}"));
                    }
                    ui.add(Button::new(t!("music.accounts.remove")).on_press(Msg::Accounts(AccountMsg::RemoveAsk(at))))
                        .id(format!("remove-account-{at}"));
                });
            });
        }
        let spotify = self.machine.spotify.is_some().then_some((
            Kind::Spotify,
            "music.spotify.title",
            "music.spotify.text",
            "add-spotify",
        ));
        for (kind, name, about, id) in [
            Some((Kind::Subsonic, "music.accounts.subsonic", "music.accounts.subsonic-text", "add-account")),
            Some((Kind::Jellyfin, "music.accounts.jellyfin", "music.accounts.jellyfin-text", "add-jellyfin")),
            spotify,
        ]
        .into_iter()
        .flatten()
        {
            list.row(SettingRow::new(t!(name)).description(t!(about)), |ui| {
                ui.add(
                    Button::new(t!("music.accounts.add")).icon("add").on_press(Msg::Accounts(AccountMsg::Add(kind))),
                )
                .id(id);
            });
        }
    }

    /// The account dialog or the question before removing one, when either is open.
    pub(super) fn account_dialogs(&self, ui: &mut View<'_, Msg>) {
        if let Some(key) = &self.removing {
            let name = self.accounts.iter().find(|account| &account.key == key).map(|account| account.name.as_str());
            let modal = Modal::new()
                .title(t!("music.accounts.remove-title", name = name.unwrap_or_default()))
                .variant("danger")
                .width(DIALOG)
                .on_close(Msg::Accounts(AccountMsg::Close))
                .action(Button::new(t!("music.accounts.cancel")).on_press(Msg::Accounts(AccountMsg::Close)))
                .action(
                    Button::new(t!("music.accounts.remove-button"))
                        .variant("danger")
                        .on_press(Msg::Accounts(AccountMsg::Remove)),
                );
            ui.add_with(modal, |ui| {
                ui.add(Words::new(t!("music.accounts.remove-text"))).fill_width();
            });
            return;
        }
        let Some(dialog) = &self.account_dialog else { return };
        let title = if dialog.existing.is_some() {
            t!("music.accounts.log-in-title", name = dialog.name.as_str())
        } else if dialog.kind == Kind::Spotify {
            t!("music.spotify.add-title")
        } else {
            t!("music.accounts.add-title")
        };
        let busy = dialog.trying.is_some();
        let modal = Modal::new()
            .title(title)
            .width(DIALOG)
            .on_close(Msg::Accounts(AccountMsg::Close))
            .action(Button::new(t!("music.accounts.cancel")).on_press(Msg::Accounts(AccountMsg::Close)))
            .action(
                Button::new(if dialog.kind == Kind::Spotify {
                    t!("music.spotify.log-in")
                } else {
                    t!("music.accounts.connect")
                })
                .variant("primary")
                .loading(busy)
                .disabled(busy)
                .on_press(Msg::Accounts(AccountMsg::Connect)),
            );
        let adding = dialog.existing.is_none();
        ui.add_with(modal, |ui| {
            ui.column(|ui| {
                if adding {
                    ui.add_with(Field::new(t!("music.accounts.name")), |ui| {
                        ui.add(
                            TextInput::new(dialog.name.clone())
                                .placeholder(t!("music.accounts.name-hint"))
                                .on_change(|typed| Msg::Accounts(AccountMsg::Name(typed))),
                        )
                        .fill_width()
                        .id(FIRST);
                    })
                    .fill_width();
                    if dialog.kind != Kind::Spotify {
                        let mut address = Field::new(t!("music.accounts.address")).required(true);
                        if accounts::unencrypted(&dialog.address) {
                            address = address.hint(t!("music.accounts.unencrypted"));
                        }
                        ui.add_with(address, |ui| {
                            ui.add(
                                TextInput::new(dialog.address.clone())
                                    .placeholder("https://music.example.org")
                                    .on_change(|typed| Msg::Accounts(AccountMsg::Address(typed))),
                            )
                            .fill_width();
                        })
                        .fill_width();
                        // Jellyfin is logged in to with a password; its API keys are a server's own.
                        if dialog.kind == Kind::Subsonic {
                            ui.add_with(Field::new(t!("music.accounts.method")), |ui| {
                                ui.add(
                                    Segmented::new([t!("music.accounts.password"), t!("music.accounts.api-key")])
                                        .selected(usize::from(dialog.login == Login::ApiKey))
                                        .on_select(|at| Msg::Accounts(AccountMsg::Method(at))),
                                );
                            })
                            .fill_width();
                        }
                    }
                    if dialog.kind != Kind::Spotify && dialog.login == Login::Password {
                        ui.add_with(Field::new(t!("music.accounts.user")).required(true), |ui| {
                            ui.add(
                                TextInput::new(dialog.user.clone())
                                    .on_change(|typed| Msg::Accounts(AccountMsg::User(typed))),
                            )
                            .fill_width();
                        })
                        .fill_width();
                    }
                }
                if dialog.kind == Kind::Spotify {
                    self.spotify_part(ui, dialog);
                } else {
                    let secret = if dialog.login == Login::ApiKey {
                        t!("music.accounts.api-key")
                    } else {
                        t!("music.accounts.password")
                    };
                    ui.add_with(
                        Field::new(secret)
                            .required(true)
                            .hint(t!("music.accounts.kept-in-memory"))
                            .error(dialog.problem.clone()),
                        |ui| {
                            let input = ui
                                .add(
                                    TextInput::new(dialog.secret.clone())
                                        .password(true)
                                        .on_change(|typed| Msg::Accounts(AccountMsg::Secret(typed)))
                                        .on_submit(|_| Msg::Accounts(AccountMsg::Connect)),
                                )
                                .fill_width();
                            if !adding {
                                input.id(FIRST);
                            }
                        },
                    )
                    .fill_width();
                }
            });
        });
    }
}

impl Music {
    /// The Spotify part of the account dialog: what the browser login asks of the person, and,
    /// once it is under way, the login page's address and a box for the address the browser
    /// ended on, for a browser on another machine.
    fn spotify_part(&self, ui: &mut View<'_, Msg>, dialog: &Dialog) {
        ui.add(Words::new(t!("music.spotify.how")).role("secondary")).fill_width();
        if let Some(problem) = &dialog.problem {
            ui.add(Words::new(problem.clone()).color("danger")).fill_width();
        }
        let Some(browser) = &dialog.browser else { return };
        ui.add(Words::new(t!("music.spotify.waiting"))).fill_width();
        ui.add(Words::new(browser.address.clone()).role("secondary")).fill_width().id("spotify-address");
        ui.add_with(Field::new(t!("music.spotify.paste")).hint(t!("music.spotify.paste-hint")), |ui| {
            ui.add(
                TextInput::new(browser.pasted.clone())
                    .on_change(|typed| Msg::Accounts(AccountMsg::Pasted(typed)))
                    .on_submit(|_| Msg::Accounts(AccountMsg::PasteDone)),
            )
            .fill_width()
            .id("spotify-paste");
        })
        .fill_width();
    }
}

/// The login the dialog's values make.
fn auth_of(dialog: &Dialog) -> Auth {
    match dialog.login {
        Login::Password => Auth::Password(dialog.secret.clone()),
        Login::ApiKey => Auth::ApiKey(dialog.secret.trim().to_owned()),
    }
}

/// The host of `address`, to name an account the person left unnamed.
fn host_of(address: &str) -> String {
    let rest = address.trim().split_once("://").map_or(address.trim(), |(_, rest)| rest);
    rest.split(['/', ':', '?']).next().unwrap_or(rest).to_owned()
}

/// What went wrong, in the person's language.
pub(super) fn words(error: &SourceError) -> String {
    match error {
        SourceError::Unreachable(Reach::Refused) => t!("music.accounts.refused"),
        SourceError::Unreachable(Reach::NoSuchHost) => t!("music.accounts.no-host"),
        SourceError::Unreachable(Reach::Timeout) => t!("music.accounts.timeout"),
        SourceError::Unreachable(Reach::Secure) => t!("music.accounts.secure"),
        SourceError::Unreachable(Reach::Other) => t!("music.accounts.broken"),
        SourceError::Login => t!("music.accounts.login"),
        SourceError::NotFound => t!("music.accounts.not-found"),
        SourceError::Server(said) => t!("music.accounts.server", said = said.as_str()),
        SourceError::Address => t!("music.accounts.bad-address"),
        SourceError::Premium => t!("music.accounts.premium"),
    }
}
