//! qmus: a music player for the terminal, playing your own library with gapless playback, album
//! art and a visualizer drawn from the sound itself.

pub mod accounts;
pub mod app;
pub mod art;
pub mod audio;
pub mod cli;
pub mod library;
pub mod locales;
pub mod mpris;
pub mod playlist;
pub mod queue;
pub mod sources;
#[cfg(test)]
pub(crate) mod testing;
pub mod vis;

use std::io::{self, Write};
use std::process::ExitCode;
use std::sync::Arc;

use qframe::prelude::*;

use app::{Machine, Opening};
use cli::Invocation;

/// Runs one `qmus` invocation on this machine. Both commands, `qmus` and `quvyta-music`, start
/// here.
///
/// # Errors
///
/// Returns the terminal's error when the screen cannot be opened or drawn.
pub fn run() -> io::Result<ExitCode> {
    let machine = Machine::here();
    let music = Machine::music_folder();
    let cwd = std::env::current_dir().unwrap_or_else(|_| music.clone());
    let folder = match cli::parse(std::env::args_os().skip(1), &music, &cwd) {
        Invocation::Screen(folder) => folder,
        other => return Ok(answer(&other)),
    };
    let config = machine.config.clone();
    runtime(Opening::new(machine, folder), config).run()?;
    Ok(ExitCode::SUCCESS)
}

/// The runtime `qmus` runs: the languages, the keymap, the settings and preferences `opening` read,
/// and, when the shared settings have a `config` folder, the look the other Quvyta applications
/// change while qmus is open.
pub(crate) fn runtime(opening: Opening, config: Option<std::path::PathBuf>) -> Runtime<app::Music> {
    let runtime = locales::LOCALES
        .iter()
        .fold(Runtime::new(opening.music), |runtime, (file, text)| runtime.locale_source(*file, *text))
        .keymap_source(locales::KEYMAP.0, locales::KEYMAP.1)
        .settings(&opening.settings)
        .preferences(&opening.preferences);
    // The settings and preferences given above are used as they are and not read twice.
    match config {
        Some(folder) => runtime.member_in(qframe::storage::Ecosystem::QUVYTA, folder, app::APP),
        None => runtime,
    }
}

/// Says what a command line that opens no screen asks for, in the person's language, and gives the
/// exit code: 0 for the version and the help, 2 for a path that is not a folder or an unknown
/// option.
#[must_use]
pub fn answer(invocation: &Invocation) -> ExitCode {
    let env = locales::env();
    let detected = env.i18n().detect(|name| std::env::var(name).ok());
    let mut i18n = env.i18n().clone();
    if let Some(code) = detected {
        i18n.set_active(&code);
    }
    qframe::i18n::scope(Arc::new(i18n), || {
        let mut out = io::stdout().lock();
        match invocation {
            Invocation::Screen(_) => ExitCode::SUCCESS,
            Invocation::Version => {
                let _ = writeln!(out, "qmus {}", env!("CARGO_PKG_VERSION"));
                ExitCode::SUCCESS
            }
            Invocation::Help => {
                let _ = writeln!(out, "{}", t!("music.cli.help", version = env!("CARGO_PKG_VERSION")));
                ExitCode::SUCCESS
            }
            Invocation::Missing(path) => {
                eprintln!("{}", t!("music.cli.missing", path = path.display().to_string()));
                ExitCode::from(2)
            }
            Invocation::Unknown(argument) => {
                eprintln!("{}", t!("music.cli.unknown", argument = argument.as_str()));
                ExitCode::from(2)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_that_is_not_a_folder_or_an_unknown_option_exits_with_2_and_the_rest_with_0() {
        assert_eq!(answer(&Invocation::Missing("/nowhere/at/all".into())), ExitCode::from(2));
        assert_eq!(answer(&Invocation::Unknown("--frobnicate".into())), ExitCode::from(2));
        assert_eq!(answer(&Invocation::Version), ExitCode::SUCCESS);
        assert_eq!(answer(&Invocation::Help), ExitCode::SUCCESS);
    }
}
