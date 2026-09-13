use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::provider::ResumeCursor;

const INDEX_FILE: &str = "index.json";

#[derive(Debug, Deserialize)]
struct IndexFile {
    tracks: Vec<ResumeCursor>,
}

pub fn index_path() -> PathBuf {
    PathBuf::from(INDEX_FILE)
}

pub fn load(path: &Path) -> Result<Option<Vec<ResumeCursor>>> {
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let parsed: IndexFile = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse {} (expected JSON)", path.display()))?;
    Ok(Some(parsed.tracks))
}

pub fn save(path: &Path, tracks: &[ResumeCursor]) -> Result<()> {
    #[derive(Serialize)]
    struct IndexFileRef<'a> {
        tracks: &'a [ResumeCursor],
    }

    let json = serde_json::to_string(&IndexFileRef { tracks })
        .context("failed to serialize index cache")?;
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
        std::env::temp_dir().join(format!("track-sweep-index-{label}-{nanos}.json"))
    }

    fn sample_tracks() -> Vec<ResumeCursor> {
        vec![
            ResumeCursor {
                id: "AAA".into(),
                date_added: Some("2020-01-01T00:00:00.000Z".into()),
            },
            ResumeCursor {
                id: "BBB".into(),
                date_added: None,
            },
        ]
    }

    #[test]
    fn round_trip_index_json() {
        let path = temp_path("round");
        let tracks = sample_tracks();
        save(&path, &tracks).unwrap();
        let loaded = load(&path).unwrap().unwrap();
        assert_eq!(loaded, tracks);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn missing_file_is_none() {
        let path = temp_path("missing");
        let _ = fs::remove_file(&path);
        assert!(load(&path).unwrap().is_none());
    }

    #[test]
    fn omits_missing_date_added() {
        let path = temp_path("omit-date");
        save(
            &path,
            &[ResumeCursor {
                id: "only-id".into(),
                date_added: None,
            }],
        )
        .unwrap();
        let contents = fs::read_to_string(&path).unwrap();
        assert!(!contents.contains("date_added"));
        let _ = fs::remove_file(&path);
    }
}
