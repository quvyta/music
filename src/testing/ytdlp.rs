//! A stand-in for the person's yt-dlp: a small script in the test's own folder that answers as
//! yt-dlp would and writes down every set of arguments it was given. Nothing reaches YouTube.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use super::Scratch;

/// How the stand-in behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Way {
    /// It answers.
    Answers,
    /// It finds no YouTube login in the browser.
    NoLogin,
    /// It never finishes.
    Hangs,
}

/// The stand-in, and where it writes its arguments.
pub struct Fake {
    /// The program.
    pub program: PathBuf,
    /// The file each run's arguments are written to, one run a line.
    pub calls: PathBuf,
}

impl Fake {
    /// Every run's arguments so far.
    pub fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.calls).unwrap_or_default().lines().map(str::to_owned).collect()
    }
}

/// A liked-music list of two tracks of Kalben's, as yt-dlp gives it.
pub const LIKED: &str = r#"{"id":"LM","entries":[{"id":"yt-aaaaaaa","title":"Gece Mavisi","channel":"Kalben - Topic","duration":187.0},{"id":"yt-bbbbbbb","title":"Sabah Treni","uploader":"Kalben","duration":201.5}]}"#;

/// Writes the stand-in into `scratch`: it lists `list`, gives `audio` as every track's sound, and
/// behaves as `way` says.
pub fn fake(scratch: &Scratch, list: &str, audio: &str, way: Way) -> Fake {
    let program = scratch.path("bin/yt-dlp");
    let calls = scratch.path("bin/calls");
    let listed = scratch.path("bin/list.json");
    std::fs::create_dir_all(scratch.path("bin")).expect("folder");
    std::fs::write(&listed, list).expect("the list");
    let failing = match way {
        Way::Answers => String::new(),
        Way::NoLogin => {
            "echo 'ERROR: [youtube:tab] LM: Sign in to confirm, use --cookies-from-browser' >&2; exit 1".to_owned()
        }
        Way::Hangs => "exec sleep 60".to_owned(),
    };
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{calls}'\ncase \" $* \" in\n  *' --version '*) echo 2025.09.26; exit 0 ;;\nesac\n{failing}\ncase \" $* \" in\n  *' -J '*) cat '{listed}' ;;\n  *' -g '*) echo '{audio}' ;;\nesac\n",
        calls = calls.display(),
        listed = listed.display(),
    );
    std::fs::write(&program, script).expect("the script");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).expect("runnable");
    Fake { program, calls }
}
