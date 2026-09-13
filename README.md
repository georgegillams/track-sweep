# track-sweep

CLI for quickly sorting an Apple Music library on macOS.

Walks your Music.app library from oldest to newest (by date added), plays each
track starting at 0:30 when longer than 1 minute, and lets you dislike, favourite, unfavourite, or skip —
with resume via a local `./progress.json` file. A lightweight id/date index is
built at startup (with progress) and saved to `./index.json`. On later runs you
are asked whether to reuse that cache or re-index the library. Each track’s
full metadata is loaded only when it is played.

## Requirements

- macOS with the Music app
- Rust toolchain (`cargo`)
- **Automation permission**: the first run will prompt macOS to allow your
  terminal (Terminal, iTerm, Cursor, etc.) to control Music. Allow it under
  **System Settings → Privacy & Security → Automation**.

## Build & run

```bash
cargo run --release
```

Or install a local binary:

```bash
cargo install --path .
track-sweep
```

## Keys

Shown options depend on the track:

| Key | Action |
|-----|--------|
| `r` | Dislike track (keeps it in the library) |
| `f` | Favourite (only if not already favourited) |
| `n` | Unfavourite (only if currently favourited) |
| `Space` / `Enter` / `y` | Keep (clears dislike in Music.app) |
| `0`–`9` | Skip forward 0–90 seconds (`3` = +30s) |
| `b` | Go back to the previous track (repeatable) |
| `q` / `Ctrl-C` | Quit (asks whether to delete disliked tracks; resume on current track next run) |

Each decision is appended to `./decisions.csv`:

```csv
title,album,artist,decision
Track Title,Album Name,Artist Name,keep
```

`decision` is one of `keep`, `favourite`, `unfavourite`, or `dislike`.

After each action (except quit), progress is also written to `./progress.json`:

```json
{
  "id": "A1B2C3D4E5F60718",
  "date_added": "2019-04-12T15:30:00.000Z",
  "name": "Track Title",
  "artist": "Artist Name"
}
```

On quit (and when the sweep finishes), if any tracks were disliked, you are
asked whether to delete them from the library (`y` / `n`). Disliked track ids
are kept in `./disliked.json` until they are deleted, so a later quit can still
flush them. Keep also clears the Music.app dislike. Keep, favourite, or unfavourite
on a previously disliked track also removes it from that pending-delete list.

On the next run, sweeping continues from the track after that entry. If the
track id is missing but `date_added` is still present, resume uses the saved
date. Delete `./progress.json` to start over from the oldest track.

The library order is cached in `./index.json` after indexing. If that file is
present at startup:

| Key | Action |
|-----|--------|
| `c` / Enter | Use the cached index |
| `r` | Re-index Music.app and overwrite the cache |
| `q` | Quit |

Delete `./index.json` (or choose re-index) after adding or removing many tracks
so the sweep list matches the current library. Missing tracks in a stale cache
are skipped when played.

## Architecture

Music.app access is isolated behind a `MusicProvider` trait
(`src/provider/`). The macOS backend uses JXA via `osascript`. Other platforms
can be added by implementing the same trait.
