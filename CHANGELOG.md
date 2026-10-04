# Changelog

Every release of quvyta-music, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## 0.1.2 - 2026-10-04

### Added

- More folders in one library: the settings' new Library section adds a folder through a folder picker, takes one out again (its music stays where it is) and reads every folder again on demand. A folder whose music is already read is not added twice.
- A start with a large library is quick: qmus keeps an index of the tags it has read (in `~/.local/state/quvyta/music/library.index`), shows the library from it at once, and opens only the files that are new or have changed since.

## 0.1.1 - 2026-10-03

### Fixed

- The queue picked up at start could show its track at 0:00 instead of the moment it was left at, on a busy machine.

## 0.1.0 - 2026-10-03

### Added

- The first qmus: the music of a folder (your Music folder, or the folder given) in a table by artist, album and track number, named by the tags of the files. Enter plays a track, `p` and the play button hold and go on, `n` and `b` step through the list, and a track heard to its end gives way to the next.
- The sound is decoded and played by qmus itself on the system's default sound device; FLAC, MP3, Ogg Vorbis, WAV, AIFF and M4A (AAC and ALAC) play.
- A visualizer at the start of the player bar, drawn from the sound as it goes out: eight bands of frequencies rising at once and falling slowly under thin caps, coming down to rest when the sound is held. With reduced motion it reads the sound four times a second, without caps.
- `shift+→` and `shift+←` move the track five seconds forward and back; `+` and `-` set the volume (on a curve the ear hears as even, the system's volume left alone), and the volume button at the end of the player bar silences the sound and brings it back.
- A queue: the track chosen and the rest of the list after it. `x` and the shuffle button shuffle what is still to come and put it back; `r` and the repeat button go round off, the whole queue and this track. Next still goes on while one track repeats, and previous goes back through the order heard.
- A now-playing page (`alt+1`, or the note button on top; Esc goes back): the track's name drawn large, who and what album it is from, the visualizer across the page and how far the track has got.
- The queue is kept when qmus closes (in `~/.local/state/quvyta/music/queue`) and comes back on the next start held at the same track and the same second, with shuffle, repeat and the volume as they were.
- Gapless playback: the next track's first sample follows the last one's with nothing between, and the screen names it the moment it is heard. A track of another rate is converted to the rate of the sound going out, so it follows without a stop too. Holding and going on fade the sound over a few milliseconds instead of cutting it.
- The album's cover on the now-playing page: the front cover in the track's tags, else the first picture there, else a `cover`, `folder`, `front` or `album` picture in its folder. Drawn in real pixels where the terminal can, in half blocks elsewhere.
- A side bar of pages: now playing, the queue, the tracks, the albums and the artists (`alt+1` to `alt+5`; below 100 columns it folds away and `ctrl+b` opens it). An album or artist opens to its tracks and plays as its own queue; the queue page lists what comes and `Delete` takes a track out.
- Search (`/` or `ctrl+f`): every word typed narrows the tracks, albums and artists, without regard to case, Turkish dotted and dotless i or accents.
- qmus offers itself to the desktop over MPRIS: media keys, `playerctl` and a desktop's "now playing" corner see the track, its cover and how far it has got, and can play, hold, step, seek and set the volume.
- Playlists: `ctrl+s` keeps the queue as a plain M3U8 playlist, and a playlists page (`alt+6`) opens one and plays from it. A playlist is removed only after asking, and only the playlist goes, never the music it lists.
- A track's menu on its row: play it, play it next, put it at the end of the queue, add it to a playlist, go to its album or its artist.
- Settings: pick up where you left off or not, the visualizer as bars, mirrored or off, and kept moving when motion is reduced; the shared Quvyta appearance (language, theme, icons, reduced motion) and the update notice. Nine languages.
