use std::io::{self, Write};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use super::{MusicProvider, ResumeCursor, TrackInfo};

const INDEX_BATCH_SIZE: usize = 250;

#[derive(Debug, Deserialize)]
struct JxaTrack {
    id: String,
    name: String,
    artist: String,
    album: String,
    #[serde(rename = "dateAdded")]
    date_added: Option<String>,
    duration: f64,
    favorited: bool,
}

#[derive(Debug, Deserialize)]
struct JxaCursor {
    id: String,
    #[serde(rename = "dateAdded")]
    date_added: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CountResult {
    count: usize,
}

#[derive(Debug, Deserialize)]
struct GoneResult {
    gone: Vec<String>,
}

/// Apple Music / Music.app backend via `osascript` + JXA.
pub struct AppleMusicProvider;

impl AppleMusicProvider {
    pub fn new() -> Self {
        Self
    }

    fn run_jxa(script: &str) -> Result<String> {
        let output = Command::new("osascript")
            .args(["-l", "JavaScript", "-e", script])
            .output()
            .context("failed to run osascript (is this macOS?)")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("not allowed assistive")
                || stderr.contains("not authorized")
                || stderr.contains("(-1743)")
            {
                bail!(
                    "Music.app Automation permission denied.\n\
                     Grant access in System Settings → Privacy & Security → Automation \
                     (allow your terminal to control Music), then try again.\n\
                     Details: {stderr}"
                );
            }
            bail!(
                "osascript failed ({}): {}",
                output.status,
                stderr.trim()
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn ensure_running(&self) -> Result<()> {
        Self::run_jxa(
            r#"
            const app = Application('Music');
            app.run();
            JSON.stringify({ ok: true });
            "#,
        )?;
        Ok(())
    }

    fn js_string(value: &str) -> String {
        let escaped = value
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('\n', "\\n")
            .replace('\r', "\\r");
        format!("'{escaped}'")
    }

    fn find_track_script(id: &str, body: &str) -> String {
        let id_js = Self::js_string(id);
        format!(
            r#"
            const app = Application('Music');
            app.run();
            const id = {id_js};
            const tracks = app.libraryPlaylists[0].tracks.whose({{ persistentID: id }});
            if (tracks.length === 0) {{
                throw new Error('track not found: ' + id);
            }}
            const track = tracks[0];
            {body}
            JSON.stringify({{ ok: true }});
            "#
        )
    }

    fn track_count(&self) -> Result<usize> {
        let raw = Self::run_jxa(
            r#"
            const app = Application('Music');
            app.run();
            const tracks = app.libraryPlaylists[0].tracks;
            JSON.stringify({ count: tracks.length });
            "#,
        )?;
        let parsed: CountResult = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse track count: {raw}"))?;
        Ok(parsed.count)
    }

    fn fetch_cursor_batch(&self, offset: usize, limit: usize) -> Result<Vec<ResumeCursor>> {
        let script = format!(
            r#"
            const app = Application('Music');
            app.run();
            const tracks = app.libraryPlaylists[0].tracks;
            const offset = {offset};
            const limit = {limit};
            const end = Math.min(offset + limit, tracks.length);
            const out = [];
            for (let i = offset; i < end; i++) {{
                const t = tracks[i];
                let dateAdded = null;
                try {{
                    const d = t.dateAdded();
                    if (d) dateAdded = d.toISOString();
                }} catch (e) {{}}
                out.push({{
                    id: String(t.persistentID()),
                    dateAdded: dateAdded
                }});
            }}
            JSON.stringify(out);
            "#
        );
        let raw = Self::run_jxa(&script)?;
        let parsed: Vec<JxaCursor> = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse index batch at offset {offset}"))?;
        Ok(parsed
            .into_iter()
            .map(|c| ResumeCursor {
                id: c.id,
                date_added: c.date_added,
            })
            .collect())
    }

    fn jxa_to_track(t: JxaTrack) -> TrackInfo {
        TrackInfo {
            id: t.id,
            name: t.name,
            artist: t.artist,
            album: t.album,
            date_added: t.date_added,
            duration_secs: t.duration,
            favorited: t.favorited,
        }
    }

    fn remove_from_library_chunk(&self, ids: &[String]) -> Result<Vec<String>> {
        let ids_js = ids
            .iter()
            .map(|id| Self::js_string(id))
            .collect::<Vec<_>>()
            .join(",");
        let script = format!(
            r#"
            const app = Application('Music');
            app.run();
            const ids = [{ids_js}];
            const gone = [];
            for (const id of ids) {{
                try {{
                    const tracks = app.libraryPlaylists[0].tracks.whose({{ persistentID: id }});
                    if (tracks.length === 0) {{
                        gone.push(id);
                        continue;
                    }}
                    tracks[0].delete();
                    gone.push(id);
                }} catch (e) {{}}
            }}
            JSON.stringify({{ gone }});
            "#
        );
        let raw = Self::run_jxa(&script)
            .context("failed to remove disliked tracks from library")?;
        let parsed: GoneResult = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse remove result: {raw}"))?;
        Ok(parsed.gone)
    }

    fn set_player_position(&self, seconds: f64, relative: bool) -> Result<()> {
        let pos_expr = if relative {
            format!("Number(app.playerPosition()) + {seconds}")
        } else {
            seconds.to_string()
        };
        let script = format!(
            r#"
            const app = Application('Music');
            app.run();
            let pos = {pos_expr};
            if (pos < 0) {{
                pos = 0;
            }}
            try {{
                const t = app.currentTrack();
                const dur = Number(t.duration());
                if (dur > 0 && pos >= dur) {{
                    pos = Math.max(0, dur - 1);
                }}
            }} catch (e) {{}}
            app.playerPosition = pos;
            JSON.stringify({{ ok: true, position: pos }});
            "#
        );
        Self::run_jxa(&script)?;
        Ok(())
    }
}

impl Default for AppleMusicProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MusicProvider for AppleMusicProvider {
    fn list_library_order_oldest_first(&self) -> Result<Vec<ResumeCursor>> {
        self.ensure_running()?;
        let total = self.track_count()?;
        if total == 0 {
            return Ok(Vec::new());
        }

        eprintln!("Indexing library ({total} tracks)…");
        let _ = io::stderr().flush();

        let mut all = Vec::with_capacity(total);
        let mut offset = 0;
        while offset < total {
            let batch = self.fetch_cursor_batch(offset, INDEX_BATCH_SIZE)?;
            let n = batch.len();
            if n == 0 {
                break;
            }
            all.extend(batch);
            offset += n;
            eprint!("\r  indexed {offset}/{total}");
            let _ = io::stderr().flush();
        }
        eprintln!();

        all.sort_by(ResumeCursor::cmp_order);
        eprintln!("Index ready ({} tracks, oldest → newest).", all.len());
        Ok(all)
    }

    fn get_track(&self, id: &str) -> Result<TrackInfo> {
        let id_js = Self::js_string(id);
        let script = format!(
            r#"
            const app = Application('Music');
            app.run();
            const id = {id_js};
            const tracks = app.libraryPlaylists[0].tracks.whose({{ persistentID: id }});
            if (tracks.length === 0) {{
                throw new Error('track not found: ' + id);
            }}
            const t = tracks[0];
            let dateAdded = null;
            try {{
                const d = t.dateAdded();
                if (d) dateAdded = d.toISOString();
            }} catch (e) {{}}
            let favorited = false;
            try {{ favorited = !!t.favorited(); }} catch (e) {{}}
            JSON.stringify({{
                id: String(t.persistentID()),
                name: String(t.name()),
                artist: String(t.artist()),
                album: String(t.album()),
                dateAdded: dateAdded,
                duration: Number(t.duration()),
                favorited: favorited
            }});
            "#
        );
        let raw = Self::run_jxa(&script)?;
        let parsed: JxaTrack = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse track {id}"))?;
        Ok(Self::jxa_to_track(parsed))
    }

    fn play_track(&self, id: &str) -> Result<()> {
        let script = Self::find_track_script(id, "track.play();");
        Self::run_jxa(&script)?;
        Ok(())
    }

    fn seek(&self, seconds: f64) -> Result<()> {
        self.set_player_position(seconds, false)
    }

    fn seek_relative(&self, seconds: f64) -> Result<()> {
        self.set_player_position(seconds, true)
    }

    fn set_favorited(&self, id: &str, favorited: bool) -> Result<()> {
        let script = Self::find_track_script(
            id,
            &format!("track.favorited = {};", if favorited { "true" } else { "false" }),
        );
        Self::run_jxa(&script)?;
        Ok(())
    }

    fn set_disliked(&self, id: &str, disliked: bool) -> Result<()> {
        let script = Self::find_track_script(
            id,
            &format!("track.disliked = {};", if disliked { "true" } else { "false" }),
        );
        Self::run_jxa(&script)?;
        Ok(())
    }

    fn remove_from_library(&self, ids: &[String]) -> Result<Vec<String>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut gone = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(100) {
            gone.extend(self.remove_from_library_chunk(chunk)?);
        }
        Ok(gone)
    }

    fn pause(&self) -> Result<()> {
        Self::run_jxa(
            r#"
            const app = Application('Music');
            try { app.pause(); } catch (e) {}
            JSON.stringify({ ok: true });
            "#,
        )?;
        Ok(())
    }
}
