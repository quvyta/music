//! Every language file against the English one: the same keys, the same placeholders, and the
//! plural forms its language needs. The checks read the files themselves, so a key a language
//! borrows from English on screen still counts as missing here.

use std::collections::{BTreeMap, BTreeSet};

use qframe::i18n::PluralCategory;

use super::{LOCALES, env};

/// One message of a file: a plain text, or its plural forms by category name.
#[derive(Debug)]
enum Entry {
    Plain(String),
    Plural(BTreeMap<String, String>),
}

/// A parsed language file: its code and every message under its dotted key.
struct File {
    code: String,
    entries: BTreeMap<String, Entry>,
}

fn parse(name: &str, text: &str) -> File {
    let table: toml::Table = text.parse().unwrap_or_else(|error| panic!("{name} is not TOML: {error}"));
    let code = table
        .get("meta")
        .and_then(|meta| meta.get("code"))
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("{name} has no meta.code"))
        .to_owned();
    let mut entries = BTreeMap::new();
    for (section, value) in &table {
        if section != "meta" {
            flatten(name, section, value, &mut entries);
        }
    }
    File { code, entries }
}

/// Whether an inline table is a plural message rather than a section: all its keys are
/// plural category names.
fn is_plural(table: &toml::Table) -> bool {
    !table.is_empty() && table.keys().all(|key| PluralCategory::from_name(key).is_some())
}

fn flatten(name: &str, prefix: &str, value: &toml::Value, out: &mut BTreeMap<String, Entry>) {
    match value {
        toml::Value::String(text) => {
            out.insert(prefix.to_owned(), Entry::Plain(text.clone()));
        }
        toml::Value::Table(table) if is_plural(table) => {
            let forms = table
                .iter()
                .map(|(category, text)| {
                    let text = text.as_str().unwrap_or_else(|| panic!("{name}: {prefix}.{category} is not text"));
                    (category.clone(), text.to_owned())
                })
                .collect();
            out.insert(prefix.to_owned(), Entry::Plural(forms));
        }
        toml::Value::Table(table) => {
            for (key, inner) in table {
                flatten(name, &format!("{prefix}.{key}"), inner, out);
            }
        }
        other => panic!("{name}: {prefix} is neither text nor a table: {other:?}"),
    }
}

fn files() -> Vec<File> {
    LOCALES.iter().map(|(name, text)| parse(name, text)).collect()
}

fn english() -> File {
    files().into_iter().find(|file| file.code == "en").expect("the English file is compiled in")
}

/// The `{name}` placeholders of a text.
fn placeholders(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else { break };
        found.insert(after[..end].to_owned());
        rest = &after[end + 1..];
    }
    found
}

/// The plural categories whole counts fall into in a language, by the framework's own rule, so a
/// file is asked for exactly the forms the screen can pick.
fn categories_for(code: &str) -> BTreeSet<String> {
    let mut needed: BTreeSet<String> = (0..=200).map(|n| PluralCategory::of(code, n).name().to_owned()).collect();
    // `other` is what every lookup falls back to, so it is always written.
    needed.insert("other".to_owned());
    needed
}

/// Every text of a file, the plural forms included.
fn texts(entry: &Entry) -> Vec<&String> {
    match entry {
        Entry::Plain(text) => vec![text],
        Entry::Plural(forms) => forms.values().collect(),
    }
}

#[test]
fn every_file_loads_cleanly_and_names_its_language() {
    let env = env();
    assert_eq!(env.diagnostics(), &[], "a language file has a problem");
    let listed = env.i18n().list();
    for file in files() {
        let name = listed.iter().find(|(code, _)| *code == file.code).map(|(_, name)| name.clone());
        assert!(name.is_some_and(|name| !name.trim().is_empty()), "`{}` has no name to show", file.code);
    }
}

#[test]
fn every_language_carries_every_english_key_and_nothing_else() {
    let english = english();
    let env = env();
    for file in files() {
        let missing: Vec<&String> = english.entries.keys().filter(|key| !file.entries.contains_key(*key)).collect();
        assert!(missing.is_empty(), "`{}` lacks {missing:?}", file.code);
        let extra: Vec<&String> = file.entries.keys().filter(|key| !english.entries.contains_key(*key)).collect();
        assert!(extra.is_empty(), "`{}` has keys English does not: {extra:?}", file.code);
        for key in english.entries.keys() {
            assert!(env.i18n().has(&file.code, key), "`{key}` has no text of its own in `{}`", file.code);
        }
    }
}

/// Having every key says nothing about the words: a file whose texts are all English passes the
/// test above. This one asks that nearly every text differs from the English one; the few that may
/// stay the same (a program's name, a key's name) are far fewer than a tenth.
#[test]
fn every_language_is_written_in_its_own_words() {
    let english = english();
    for file in files().into_iter().filter(|file| file.code != "en") {
        let total = file.entries.len();
        let same: Vec<&String> = file
            .entries
            .iter()
            .filter(|(key, entry)| english.entries.get(*key).is_some_and(|source| texts(source) == texts(entry)))
            .map(|(key, _)| key)
            .collect();
        assert!(same.len() * 10 < total, "`{}` keeps the English words of {same:?}", file.code);
    }
}

#[test]
fn every_translation_keeps_the_english_placeholders() {
    let english = english();
    for file in files() {
        for (key, entry) in &file.entries {
            let Some(source) = english.entries.get(key) else { continue };
            let wanted: BTreeSet<String> = texts(source).into_iter().flat_map(|text| placeholders(text)).collect();
            for text in texts(entry) {
                assert!(!text.trim().is_empty(), "`{key}` is empty in `{}`", file.code);
                assert_eq!(placeholders(text), wanted, "`{key}` in `{}` changes the placeholders: {text}", file.code);
            }
        }
    }
}

#[test]
fn every_plural_has_exactly_the_forms_its_language_uses() {
    for file in files() {
        let needed = categories_for(&file.code);
        for (key, entry) in &file.entries {
            if let Entry::Plural(forms) = entry {
                let written: BTreeSet<String> = forms.keys().cloned().collect();
                assert_eq!(written, needed, "`{key}` in `{}` has the wrong plural forms", file.code);
            }
        }
    }
}

#[test]
fn the_framework_speaks_every_language_the_application_does() {
    // Dialog buttons, key names and pickers come from the framework; a gap there would put
    // English words on an otherwise translated screen.
    let env = env();
    for file in files() {
        let missing = env.i18n().missing_keys(&file.code, "en");
        assert!(missing.is_empty(), "`{}` falls back to English for {missing:?}", file.code);
    }
}
