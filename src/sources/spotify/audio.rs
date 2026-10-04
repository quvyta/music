//! The sound of a Spotify track, through librespot: the file Spotify keeps for the track, fetched
//! as it is read, decrypted, and handed to qmus's own decoder as Ogg Vorbis.

use std::sync::Arc;

use symphonia::core::io::MediaSource;

/// What gives the sound of a Spotify track; librespot's, or one that stands in for it in a test.
pub trait SpotifyAudio: Send + Sync {
    /// The sound of the track `id`, from its start, as Ogg Vorbis.
    ///
    /// # Errors
    ///
    /// Why it cannot be had, in words for the person: not playable here, or Spotify unreachable.
    fn open(&self, id: &str) -> Result<Box<dyn MediaSource>, String>;
}

/// A shared source of Spotify sound.
pub type Audio = Arc<dyn SpotifyAudio>;

#[cfg(feature = "spotify")]
pub use librespot::Librespot;

#[cfg(feature = "spotify")]
mod librespot {
    use std::io::{self, Read, Seek, SeekFrom};
    use std::sync::Mutex;
    use std::time::Duration;

    use librespot_audio::{AudioDecrypt, AudioFile};
    use librespot_core::authentication::Credentials;
    use librespot_core::{Session, SessionConfig, SpotifyId, SpotifyUri};
    use librespot_metadata::audio::{AudioFileFormat, AudioItem};
    use symphonia::core::io::MediaSource;

    use super::SpotifyAudio;
    use crate::sources::SourceError;

    /// Where the Vorbis stream starts in a file of Spotify's: before it, Spotify's own header.
    const HEADER: u64 = 0xa7;

    /// The files of a track qmus asks for, best first: Ogg Vorbis, which qmus decodes itself.
    const FORMATS: [(AudioFileFormat, usize); 3] = [
        (AudioFileFormat::OGG_VORBIS_320, 40 * 1024),
        (AudioFileFormat::OGG_VORBIS_160, 20 * 1024),
        (AudioFileFormat::OGG_VORBIS_96, 12 * 1024),
    ];

    /// How long opening a track may take before Spotify is taken to be unreachable.
    const PATIENCE: Duration = Duration::from_secs(30);

    /// A librespot session of the person's, with the runtime it lives on.
    pub struct Librespot {
        /// The runtime librespot's work runs on, alive as long as the session.
        runtime: tokio::runtime::Runtime,
        /// The session.
        session: Session,
    }

    impl Librespot {
        /// A session logged in with the Web API's access token `access`, for a Premium account
        /// only.
        ///
        /// # Errors
        ///
        /// [`SourceError::Login`] when Spotify refuses the token, [`SourceError::Premium`] for an
        /// account that is not Premium, and [`SourceError::Unreachable`] when Spotify cannot be
        /// reached.
        pub fn connect(access: &str) -> Result<Self, SourceError> {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .map_err(|_| SourceError::Server("no runtime".to_owned()))?;
            let session = runtime.block_on(async {
                let session = Session::new(SessionConfig::default(), None);
                let connected =
                    tokio::time::timeout(PATIENCE, session.connect(Credentials::with_access_token(access), false))
                        .await;
                match connected {
                    Ok(Ok(())) => Ok(session),
                    Ok(Err(_)) => Err(SourceError::Login),
                    Err(_) => Err(SourceError::Unreachable(crate::sources::Reach::Timeout)),
                }
            })?;
            if session.get_user_attribute("type").is_some_and(|kind| kind != "premium") {
                return Err(SourceError::Premium);
            }
            Ok(Self { runtime, session })
        }
    }

    impl SpotifyAudio for Librespot {
        fn open(&self, id: &str) -> Result<Box<dyn MediaSource>, String> {
            let track = SpotifyId::from_base62(id).map_err(|_| "not a Spotify track".to_owned())?;
            let session = self.session.clone();
            let opened = self.runtime.block_on(async move {
                tokio::time::timeout(PATIENCE, async {
                    let item = AudioItem::get_file(&session, SpotifyUri::Track { id: track })
                        .await
                        .map_err(|error| error.to_string())?;
                    if item.availability.is_err() {
                        return Err("this track cannot be played here on Spotify".to_owned());
                    }
                    let (file, rate) = FORMATS
                        .iter()
                        .find_map(|(format, rate)| item.files.0.get(format).map(|file| (*file, *rate)))
                        .ok_or_else(|| "Spotify has no file of this track qmus can play".to_owned())?;
                    let fetched = AudioFile::open(&session, file, rate).await.map_err(|error| error.to_string())?;
                    let key = session.audio_key().request(track, file).await.map_err(|error| error.to_string())?;
                    Ok(AudioDecrypt::new(Some(key), fetched))
                })
                .await
                .map_err(|_| "Spotify did not answer in time".to_owned())?
            })?;
            let mut stream = Skipped { inner: Mutex::new(opened) };
            stream.seek(SeekFrom::Start(0)).map_err(|error| error.to_string())?;
            Ok(Box::new(stream))
        }
    }

    /// A decrypted file of Spotify's with its header left out, readable from more than one thread
    /// as the decoder asks.
    struct Skipped<T> {
        /// The file.
        inner: Mutex<T>,
    }

    impl<T: Read> Read for Skipped<T> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.inner.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner).read(buf)
        }
    }

    impl<T: Seek> Seek for Skipped<T> {
        fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
            let inner = self.inner.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner);
            let at = match to {
                SeekFrom::Start(at) => inner.seek(SeekFrom::Start(at + HEADER))?,
                other => inner.seek(other)?,
            };
            Ok(at.saturating_sub(HEADER))
        }
    }

    impl<T: Read + Seek + Send> MediaSource for Skipped<T> {
        fn is_seekable(&self) -> bool {
            true
        }

        fn byte_len(&self) -> Option<u64> {
            None
        }
    }
}
