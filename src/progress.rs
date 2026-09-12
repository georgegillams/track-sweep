use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::provider::TrackInfo;

const PROGRESS_FILE: &str = "progress.json";

/// Last completed track — enough to resume and to read by eye.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProgressEntry {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_added: Option<String>,
    pub name: String,
    pub artist: String,
}

impl ProgressEntry {
    pub fn from_track(track: &TrackInfo) -> Self {
        Self {
            id: track.id.clone(),
            date_added: track.date_added.clone(),
            name: track.name.clone(),
            artist: track.artist.clone(),
        }
    }
}

pub fn progress_path() -> PathBuf {
    PathBuf::from(PROGRESS_FILE)
}

pub fn load(path: &Path) -> Result<Option<ProgressEntry>> {
    if path.exists() {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let entry: ProgressEntry = serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse {} (expected JSON)", path.display()))?;
        return Ok(Some(entry));
    }

    // Migrate legacy plain-ID `./progress` only when loading the default file.
    if path == Path::new(PROGRESS_FILE) {
        let legacy = Path::new("progress");
        if legacy.exists() {
            let contents = fs::read_to_string(legacy)
                .with_context(|| format!("failed to read {}", legacy.display()))?;
            let id = contents.trim();
            if !id.is_empty() {
                eprintln!(
                    "Note: found legacy ./progress; using id only. Will rewrite as {PROGRESS_FILE}."
                );
                return Ok(Some(ProgressEntry {
                    id: id.to_string(),
                    date_added: None,
                    name: String::new(),
                    artist: String::new(),
                }));
            }
        }
    }

    Ok(None)
}

pub fn save(path: &Path, entry: &ProgressEntry) -> Result<()> {
    let json = serde_json::to_string_pretty(entry).context("failed to serialize progress")?;
    fs::write(path, format!("{json}\n"))
        .with_context(|| format!("failed to write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("track-sweep-progress-{label}-{nanos}.json"))
    }

    #[test]
    fn round_trip_progress_json() {
        let path = temp_path("round");
        let entry = ProgressEntry {
            id: "ABC123".into(),
            date_added: Some("2020-01-02T03:04:05.000Z".into()),
            name: "Song Title".into(),
            artist: "An Artist".into(),
        };
        save(&path, &entry).unwrap();
        let loaded = load(&path).unwrap().unwrap();
        assert_eq!(loaded, entry);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn missing_file_is_none() {
        let path = temp_path("missing");
        let _ = fs::remove_file(&path);
        assert!(load(&path).unwrap().is_none());
    }
}
