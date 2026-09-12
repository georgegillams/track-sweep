# track-sweep

CLI for quickly sorting and purging an Apple Music library on macOS.

Walks your Music.app library from oldest to newest (by date added), plays each
track starting at 0:30 when longer than 1 minute, and lets you remove, favourite, unfavourite, or skip —
with resume via a local `./progress.json` file. A lightweight id/date index is
built at startup (with progress), then each track’s full metadata is loaded
only when it is played.

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
| `r` | Remove track from library |
| `f` | Favourite (only if not already favourited) |
| `n` | Unfavourite (only if currently favourited) |
| `Space` / `Enter` | Keep (no library change) |
| `b` | Go back to the previous track (repeatable) |
| `q` / `Ctrl-C` | Quit (resume on current track next run) |

Each decision is appended to `./decisions.csv`:

```csv
title,album,artist,decision
Track Title,Album Name,Artist Name,keep
```

`decision` is one of `keep`, `favourite`, `unfavourite`, or `remove`.

After each action (except quit), progress is also written to `./progress.json`:

```json
{
  "id": "A1B2C3D4E5F60718",
  "date_added": "2019-04-12T15:30:00.000Z",
  "name": "Track Title",
  "artist": "Artist Name"
}
```

On the next run, sweeping continues from the track after that entry. If the
track id was removed but `date_added` is still present, resume uses the saved
date. Delete `./progress.json` to start over from the oldest track.

## Architecture

Music.app access is isolated behind a `MusicProvider` trait
(`src/provider/`). The macOS backend uses JXA via `osascript`. Other platforms
can be added by implementing the same trait.
