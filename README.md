# qmus

A music player for the terminal, part of the Quvyta ecosystem. qmus plays your own music: the files in your Music folder, named by their tags, played by qmus itself on your sound device.

qmus is young. Today it shows the music of a folder by track, album and artist, finds it as you type, and plays it as a queue you can shuffle and repeat, with a visualizer drawn from the sound itself in the player bar and across a now-playing page. The queue is kept when qmus closes and comes back where it was left. The desktop's media keys, `playerctl` and any "now playing" corner that speaks MPRIS drive it over the session bus; where there is no session bus, as over SSH, qmus plays on without it. `ctrl+s` keeps the queue as a playlist, a plain M3U8 file another player can read too, and the playlists page plays them back.

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

## Keys

| Key | What it does |
|---|---|
| ↑ ↓, Home, End | move through the tracks |
| Enter | play the track under the cursor |
| `p` | play or pause (space too, while the tracks do not have the keyboard) |
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
| right click, `menu` | a track's menu: play it next, add it to the queue or a playlist, go to its album or artist |
| `Delete` | take the track under the cursor out of the queue (on the queue page), or remove a playlist after asking (on the playlists page) |
| `ctrl+,` | settings |
| `?` | every key |
| `q`, `ctrl+q` | quit |

Everything the keys do, the mouse does too: click a track to play it, and use the buttons in the bar below.

## Network

Once a day at start qmus asks crates.io whether a newer version is out, and says so when one is. The question sends the program's name and version in the `User-Agent` header and nothing else. It is on by default and turned off in the settings, for every Quvyta application at once. Nothing else in qmus uses the network.

## License

MIT
