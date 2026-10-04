# qmus

A music player for the terminal, part of the Quvyta ecosystem. qmus plays your own music: the files in your Music folder, named by their tags, and the music on a server of your own, played by qmus itself on your sound device.

qmus is young. Today it shows the music of a folder by track, album and artist, finds it as you type, and plays it as a queue you can shuffle and repeat, with a visualizer drawn from the sound itself in the player bar and across a now-playing page. The queue is kept when qmus closes and comes back where it was left. The desktop's media keys, `playerctl` and any "now playing" corner that speaks MPRIS drive it over the session bus; where there is no session bus, as over SSH, qmus plays on without it. `ctrl+s` keeps the queue as a playlist, a plain M3U8 file another player can read too, and the playlists page plays them back. A music server you run yourself (Jellyfin, or Navidrome, Gonic, Airsonic and others that speak the Subsonic API) joins the library from the settings, its tracks beside your files.

The package is `quvyta-music`; the commands are `qmus` and `quvyta-music`.

```
cargo install quvyta-music
```

Building needs the ALSA library (`alsa-lib` on Arch Linux, `libasound2-dev` on Debian and Ubuntu); every Linux desktop with sound already has it at run time, and PipeWire or PulseAudio receive the sound through it.

## Using it

```
qmus [FOLDER]
```

- Without a folder, qmus shows your Music folder: the one your desktop names in `user-dirs.dirs`, or `~/Music`.
- With a folder, qmus shows the music in it and every folder below it.
- More folders can join the library from the settings (`ctrl+,`, Library): add one, take one out, or read them all again.
- A path that is not a folder is a one-line message and exit code 2; no screen opens.
- `qmus --version` and `qmus --help` print and leave.

qmus reads your music and never changes it: it does not write to, rename, move or delete any of your files. What it keeps of its own, the queue, the moment you left it at, the volume and an index of the tags it has read (so a start opens only the files that are new or changed), is in `~/.local/state/quvyta/music/`; a copy of each album cover it has shown, for the desktop's "now playing" corner, is in `~/.cache/quvyta/music/art/`, and the folder can be emptied at any time. Playlists are kept in `~/.local/share/quvyta/music/playlists/`; removing one asks first and takes only the playlist file.

## Your own music server

Add the server in the settings (`ctrl+,`, Accounts): its address, your user name and your password, or, for a Subsonic server, an API key where the server gives one. A Jellyfin server is logged in to with your password once; after that only the session the server gives is used, and it too is held only while qmus is open. qmus logs in, lists everything the server has, and from then on its tracks are in the tracks, albums, artists and search like your files, each marked with where it is from; a picker over the lists shows one source at a time. A track from the server is played from the server, never matched by name against another source; "Find on another source" in a track's menu searches the others for it and leaves the choice to you.

- Your password is never written to disk. qmus holds it only while it is open, so after a restart the account asks for it again (the server's tracks are shown from the last listing until then).
- A track is fetched as it plays into `~/.cache/quvyta/music/stream/`, played from there the next time, and that folder is held to 500 MB by letting the oldest tracks go. What each server listed is kept in `~/.cache/quvyta/music/sources/`. Both folders can be emptied at any time.
- The server's own playlists are on the playlists page; "Copy to a qmus playlist" makes one of yours from one. A playlist of yours can hold the server's tracks too: they are written as `qmus://` lines, which other players pass over.
- qmus tells the server which tracks you heard, for its own play counts, unless you turn "Report plays" off for that account.
- The account list is in `~/.config/quvyta/music.accounts`: the name, address and user name of each account, never a password. Removing an account asks first and forgets it in qmus only; nothing on the server changes.
- An address that starts with `http://` and is not on your home network is marked unencrypted in the settings: your login would cross the network readable.

## Spotify Premium (not built by default)

Spotify support is new and has not yet been tried on enough real accounts to be built by default. To try it, install with `cargo install quvyta-music --features spotify`.

qmus plays your Spotify liked songs and playlists through [librespot](https://github.com/librespot-org/librespot), an unofficial Spotify client: Spotify gives no way for a new player to play its music, so qmus logs in the way Spotify's own players do. Spotify may change that at any time, and it is between you and Spotify whether you use it.

- It needs a Premium account: Spotify lets no other account play in another player. qmus checks when you log in, and says so.
- Log in from the settings (`ctrl+,`, Accounts, Spotify Premium): Spotify's own login page opens in your browser, you log in there, and the browser comes back to qmus by itself. qmus never sees your Spotify password. When the browser is on another machine (qmus over SSH), paste the address the browser ended on into the dialog.
- The session is held only while qmus is open; after a restart the account asks you to log in again (one click when the browser remembers you). Of Spotify's, qmus writes to disk only the names, artists, albums and lengths of your liked songs, so the next start can show them at once, and a copy of each album cover shown, for the desktop's "now playing" corner; never the sound.
- The sound is Spotify's own file of the track, decoded by qmus, so the visualizer and the gapless queue work as with your files. qmus does not skip or change anything Spotify plays, and plays no advertisements because Premium has none.
- Spotify's play history does not hear from other players, so there is nothing to report there.
- Without the `spotify` feature, which is how qmus is built by default, the account is not offered at all.

## Keys

| Key | What it does |
|---|---|
| ↑ ↓, Home, End | move through the tracks |
| Enter | play the track under the cursor |
| `space`, `p` | play or pause (in the search field space is typed) |
| `n`, `ctrl+→` | next track |
| `b`, `ctrl+←` | previous track, or the start of this one after three seconds |
| `shift+→`, `shift+←` | five seconds forward, back |
| `+`, `-` | louder, quieter (qmus's own volume; the system's is left alone) |
| `x` | shuffle what is still to come, or put it back in order |
| `r` | repeat: off, the whole queue, this track |
| `alt+1` … `alt+6` | now playing, queue, tracks, albums, artists, playlists |
| `/`, `ctrl+f` | search |
| `ctrl+b` | the side bar, on a narrow terminal |
| `ctrl+s` | keep the queue as a playlist |
| right click, `menu` | a track's menu: play it next, add it to the queue or a playlist, go to its album or artist, find it on another source |
| `Delete` | take the track under the cursor out of the queue (on the queue page), or remove a playlist after asking (on the playlists page) |
| `ctrl+,` | settings |
| `?` | every key |
| `q`, `ctrl+q` | quit |

Everything the keys do, the mouse does too: click a track to play it, and use the buttons in the bar below.

## Network

Once a day at start qmus asks crates.io whether a newer version is out, and says so when one is. The question sends the program's name and version in the `User-Agent` header and nothing else. It is on by default and turned off in the settings, for every Quvyta application at once.

The only other network traffic is to the accounts you add, and only after you add them: your own servers (logging in, listing the music, fetching the tracks you play and their covers, the server's playlists, and the plays you report) and, when built with Spotify, Spotify (its login page in your browser, its Web API for your liked songs, playlists and covers, and its servers for the sound). Without an account, qmus talks to nothing but crates.io.

## License

MIT
