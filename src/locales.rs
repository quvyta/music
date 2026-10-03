//! The language files and the keymap, compiled in so an installed binary needs nothing beside it.
//! The media icons come from the shared icon set.
//!
//! A new language is one more line in [`LOCALES`]: the runtime, the tests and the command line
//! all read the list.

use qframe::env::{AssetDirs, Env};
use qframe::i18n::I18n;

/// The compiled-in language files. English comes first: it is the fallback, and every other file
/// is held to its keys.
pub const LOCALES: &[(&str, &str)] = &[
    ("en.toml", include_str!("../assets/locales/en.toml")),
    ("tr.toml", include_str!("../assets/locales/tr.toml")),
    ("de.toml", include_str!("../assets/locales/de.toml")),
    ("es.toml", include_str!("../assets/locales/es.toml")),
    ("fr.toml", include_str!("../assets/locales/fr.toml")),
    ("ja.toml", include_str!("../assets/locales/ja.toml")),
    ("pt-BR.toml", include_str!("../assets/locales/pt-BR.toml")),
    ("ru.toml", include_str!("../assets/locales/ru.toml")),
    ("zh-Hans.toml", include_str!("../assets/locales/zh-Hans.toml")),
];

/// The compiled-in keymap: qmus's own keys, and `q` beside `ctrl+q` for quitting.
pub const KEYMAP: (&str, &str) = ("keymap.toml", include_str!("../assets/keymap.toml"));

/// The environment as the runtime loads it, used by the tests: the languages and the keymap.
#[must_use]
pub fn env() -> Env {
    let dirs = AssetDirs {
        locale_sources: LOCALES.iter().map(|(file, text)| ((*file).to_owned(), (*text).to_owned())).collect(),
        keymap_source: Some((KEYMAP.0.to_owned(), KEYMAP.1.to_owned())),
        ..AssetDirs::default()
    };
    Env::load(&dirs).expect("the compiled-in assets are readable")
}

/// The languages qmus speaks, for resolving the shared Quvyta language and for the command line
/// before the runtime has loaded them.
#[must_use]
pub fn i18n() -> I18n {
    let mut i18n = I18n::builtin();
    for (file, text) in LOCALES {
        i18n.add_source(file, text);
    }
    i18n
}

#[cfg(test)]
mod tests;
