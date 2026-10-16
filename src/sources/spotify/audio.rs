//! The sound of a Spotify track, through librespot: the file Spotify keeps for the track, fetched
//! as it is read, decrypted, and handed to qmus's own decoder as Ogg Vorbis.

use std::sync::Arc;

use symphonia::core::io::MediaSource;

use crate::sources::{RemoteTrack, SourceError};

/// What gives the sound of a Spotify track; librespot's, or one that stands in for it in a test.
pub trait SpotifyAudio: Send + Sync {
    /// The sound of the track `id`, from its start, as Ogg Vorbis.
    ///
    /// # Errors
    ///
    /// Why it cannot be had, in words for the person: not playable here, or Spotify unreachable.
    fn open(&self, id: &str) -> Result<Box<dyn MediaSource>, String>;

    /// The person's liked songs, when the session itself can give them, which spares the Web API
    /// Spotify rations; `None` leaves them to the Web API.
    fn liked(&self) -> Option<Result<Vec<RemoteTrack>, SourceError>> {
        None
    }
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
    use librespot_metadata::{Metadata, Track};
    use symphonia::core::io::MediaSource;

    use super::SpotifyAudio;
    use crate::sources::{RemoteTrack, SourceError};

    /// How many tracks' details are asked for at once.
    const AT_ONCE: usize = 16;

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

    impl Librespot {
        /// The ids of the person's liked songs, newest first, from the session's own view of the
        /// collection.
        async fn liked_ids(session: &Session) -> Result<Vec<SpotifyUri>, String> {
            let user = session.username();
            let context = session
                .spclient()
                .get_context(&format!("spotify:user:{user}:collection"))
                .await
                .map_err(|error| error.to_string())?;
            let mut ids = Vec::new();
            for page in &context.pages {
                ids.extend(page.tracks.iter().filter_map(|track| SpotifyUri::from_uri(track.uri()).ok()));
                // A page named only by its address comes on its own.
                if page.tracks.is_empty() && !page.page_url().is_empty() {
                    let more =
                        session.spclient().get_next_page(page.page_url()).await.map_err(|error| error.to_string())?;
                    let more: serde_json::Value = serde_json::from_slice(&more).map_err(|error| error.to_string())?;
                    let uris = more.get("tracks").and_then(serde_json::Value::as_array).into_iter().flatten();
                    ids.extend(
                        uris.filter_map(|track| track.get("uri")?.as_str())
                            .filter_map(|uri| SpotifyUri::from_uri(uri).ok()),
                    );
                }
            }
            Ok(ids)
        }

        /// The track `id` as qmus lists it.
        fn track_of(track: &Track) -> Option<RemoteTrack> {
            let SpotifyUri::Track { id } = &track.id else { return None };
            let artists: Vec<&str> = track.artists.iter().map(|artist| artist.name.as_str()).collect();
            let cover = track.album.covers.first().and_then(|image| image.id.to_base16().ok());
            Some(RemoteTrack {
                id: id.to_base62().ok()?,
                title: track.name.clone(),
                artist: artists.join(", "),
                album: track.album.name.clone(),
                number: u32::try_from(track.number).ok().filter(|number| *number > 0),
                duration: u64::try_from(track.duration).ok().filter(|ms| *ms > 0).map(Duration::from_millis),
                cover: cover.map(|file| format!("https://i.scdn.co/image/{file}")),
            })
        }
    }

    impl SpotifyAudio for Librespot {
        fn liked(&self) -> Option<Result<Vec<RemoteTrack>, SourceError>> {
            let session = self.session.clone();
            let listed: Result<Vec<RemoteTrack>, String> = self.runtime.block_on(async move {
                let ids = Self::liked_ids(&session).await?;
                let mut tracks = Vec::with_capacity(ids.len());
                for chunk in ids.chunks(AT_ONCE) {
                    let mut asked = tokio::task::JoinSet::new();
                    for (at, id) in chunk.iter().cloned().enumerate() {
                        let session = session.clone();
                        asked.spawn(async move { (at, Track::get(&session, &id).await) });
                    }
                    let mut got: Vec<(usize, RemoteTrack)> = Vec::new();
                    while let Some(answer) = asked.join_next().await {
                        if let Ok((at, Ok(track))) = answer
                            && let Some(track) = Self::track_of(&track)
                        {
                            got.push((at, track));
                        }
                    }
                    // In the collection's own order.
                    got.sort_by_key(|(at, _)| *at);
                    tracks.extend(got.into_iter().map(|(_, track)| track));
                }
                Ok(tracks)
            });
            // The session could not list them: the Web API is asked instead.
            listed.ok().map(Ok)
        }

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
