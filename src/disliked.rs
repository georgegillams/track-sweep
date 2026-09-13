use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::provider::TrackInfo;

const DISLIKED_FILE: &str = "disliked.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DislikedTrack {
    pub id: String,
    pub name: String,
    pub artist: String,
}

impl DislikedTrack {
    fn from_track(track: &TrackInfo) -> Self {
        Self {
            id: track.id.clone(),
            name: track.name.clone(),
            artist: track.artist.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct DislikedFile {
    tracks: Vec<DislikedTrack>,
}

pub fn disliked_path() -> PathBuf {
    PathBuf::from(DISLIKED_FILE)
}

pub struct DislikedSet {
    path: PathBuf,
    tracks: Vec<DislikedTrack>,
}

impl DislikedSet {
    pub fn load() -> Result<Self> {
        let path = disliked_path();
        let tracks = load(&path)?.unwrap_or_default();
        Ok(Self { path, tracks })
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn tracks(&self) -> &[DislikedTrack] {
        &self.tracks
    }

    pub fn ids(&self) -> Vec<String> {
        self.tracks.iter().map(|t| t.id.clone()).collect()
    }

    pub fn add(&mut self, track: &TrackInfo) -> Result<()> {
        self.tracks.retain(|t| t.id != track.id);
        self.tracks.push(DislikedTrack::from_track(track));
        self.save()
    }

    pub fn remove_id(&mut self, id: &str) -> Result<()> {
        let before = self.tracks.len();
        self.tracks.retain(|t| t.id != id);
        if self.tracks.len() != before {
            self.save()?;
        }
        Ok(())
    }

    pub fn remove_ids(&mut self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let before = self.tracks.len();
        self.tracks.retain(|t| !ids.iter().any(|id| id == &t.id));
        if self.tracks.len() != before {
            self.save()?;
        }
        Ok(())
    }

    fn save(&self) -> Result<()> {
        save(&self.path, &self.tracks)
    }
}

fn load(path: &Path) -> Result<Option<Vec<DislikedTrack>>> {
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let parsed: DislikedFile = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse {} (expected JSON)", path.display()))?;
    Ok(Some(parsed.tracks))
}

fn save(path: &Path, tracks: &[DislikedTrack]) -> Result<()> {
    if tracks.is_empty() {
        if path.exists() {
            fs::remove_file(path)
                .with_context(|| format!("failed to clear {}", path.display()))?;
        }
        return Ok(());
    }

    let json = serde_json::to_string_pretty(&DislikedFile {
        tracks: tracks.to_vec(),
    })
    .context("failed to serialize disliked tracks")?;
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
        std::env::temp_dir().join(format!("track-sweep-disliked-{label}-{nanos}.json"))
    }

    fn track(id: &str, name: &str) -> TrackInfo {
        TrackInfo {
            id: id.into(),
            name: name.into(),
            artist: "Artist".into(),
            album: "Album".into(),
            date_added: None,
            duration_secs: 1.0,
            favorited: false,
        }
    }

    #[test]
    fn add_dedupes_and_round_trips() {
        let path = temp_path("round");
        let mut set = DislikedSet {
            path: path.clone(),
            tracks: Vec::new(),
        };
        set.add(&track("a", "One")).unwrap();
        set.add(&track("b", "Two")).unwrap();
        set.add(&track("a", "One renamed")).unwrap();
        assert_eq!(set.len(), 2);
        assert_eq!(set.tracks()[0].id, "b");
        assert_eq!(set.tracks()[1].name, "One renamed");

        let loaded = load(&path).unwrap().unwrap();
        assert_eq!(loaded, set.tracks);

        set.remove_id("b").unwrap();
        assert_eq!(set.ids(), vec!["a".to_string()]);
        set.remove_ids(&["a".into()]).unwrap();
        assert!(set.is_empty());
        assert!(!path.exists());
        let _ = fs::remove_file(&path);
    }
}
