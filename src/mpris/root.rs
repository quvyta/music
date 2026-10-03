//! The `org.mpris.MediaPlayer2` interface: who qmus is, and the one call that ends it.

use zbus::interface;

use super::{Request, Session};

/// What qmus can play, in the words the desktop file and the library already use.
const MIME_TYPES: [&str; 7] =
    ["audio/flac", "audio/mpeg", "audio/ogg", "audio/mp4", "audio/aac", "audio/x-wav", "audio/x-aiff"];

/// The `org.mpris.MediaPlayer2` interface, served at the one object path MPRIS names.
#[derive(Debug)]
pub(super) struct Root {
    /// Where a call that asks for something goes.
    pub(super) session: Session,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    /// qmus can be asked to end, like any other player.
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }

    /// qmus has no window to raise.
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
    }

    /// The tracks of the library are not a track list on the bus; the queue is qmus's own.
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    /// What qmus calls itself.
    #[zbus(property)]
    fn identity(&self) -> &'static str {
        "qmus"
    }

    /// The desktop file that starts qmus, so the desktop can put the right icon next to it.
    #[zbus(property)]
    fn desktop_entry(&self) -> &'static str {
        "quvyta-music"
    }

    /// The only things qmus can be given: files of the person's own music.
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> &'static [&'static str] {
        &["file"]
    }

    /// The sound files qmus plays, as the kinds of file the desktop knows them by.
    #[zbus(property)]
    fn supported_mime_types(&self) -> &'static [&'static str] {
        &MIME_TYPES
    }

    /// Ends qmus. Like every other call here, it only says what is asked for.
    fn quit(&self) {
        self.session.ask(Request::Quit);
    }
}
