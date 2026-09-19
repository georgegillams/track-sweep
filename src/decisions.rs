use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::provider::TrackInfo;

const DECISIONS_FILE: &str = "decisions.csv";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Keep,
    Favourite,
    Unfavourite,
    Remove,
}

impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Favourite => "favourite",
            Self::Unfavourite => "unfavourite",
            Self::Remove => "remove",
        }
    }
}

pub fn decisions_path() -> PathBuf {
    PathBuf::from(DECISIONS_FILE)
}

/// Append one decision row. Creates the file with a header if it does not exist.
pub fn append(path: &Path, track: &TrackInfo, decision: Decision) -> Result<()> {
    let needs_header = !path.exists() || path.metadata().map(|m| m.len() == 0).unwrap_or(true);

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let mut out = BufWriter::new(file);

    if needs_header {
        writeln!(out, "title,album,artist,decision")?;
    }

    writeln!(
        out,
        "{},{},{},{}",
        csv_escape(&track.name),
        csv_escape(&track.album),
        csv_escape(&track.artist),
        decision.as_str()
    )?;
    out.flush()?;
    Ok(())
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("track-sweep-decisions-{label}-{nanos}.csv"))
    }

    fn sample_track() -> TrackInfo {
        TrackInfo {
            id: "id".into(),
            name: "Hello, World".into(),
            artist: "Artist \"X\"".into(),
            album: "Album".into(),
            date_added: None,
            duration_secs: 1.0,
            favorited: false,
        }
    }

    #[test]
    fn writes_header_and_escaped_row() {
        let path = temp_path("write");
        let _ = fs::remove_file(&path);
        append(&path, &sample_track(), Decision::Keep).unwrap();
        append(
            &path,
            &TrackInfo {
                name: "Plain".into(),
                artist: "A".into(),
                album: "B".into(),
                favorited: true,
                ..sample_track()
            },
            Decision::Favourite,
        )
        .unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = contents.lines().collect();
        assert_eq!(lines[0], "title,album,artist,decision");
        assert_eq!(lines[1], "\"Hello, World\",Album,\"Artist \"\"X\"\"\",keep");
        assert_eq!(lines[2], "Plain,B,A,favourite");
        let _ = fs::remove_file(&path);
    }
}
