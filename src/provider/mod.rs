pub mod apple_music;

use anyhow::Result;
use std::cmp::Ordering;

/// Platform-agnostic track metadata used by the sweep loop.
#[derive(Debug, Clone)]
pub struct TrackInfo {
    /// Stable identifier (Apple Music persistent ID).
    pub id: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    /// Sortable date-added string (ISO-like); missing dates sort last.
    pub date_added: Option<String>,
    pub duration_secs: f64,
    pub favorited: bool,
}

/// Cursor for ordering / resuming (id + date added).
#[derive(Debug, Clone)]
pub struct ResumeCursor {
    pub id: String,
    pub date_added: Option<String>,
}

impl ResumeCursor {
    pub fn cmp_order(a: &Self, b: &Self) -> Ordering {
        match (&a.date_added, &b.date_added) {
            (Some(x), Some(y)) => match x.cmp(y) {
                Ordering::Equal => a.id.cmp(&b.id),
                other => other,
            },
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => a.id.cmp(&b.id),
        }
    }
}

/// Abstraction over a music library / player so other platforms can be added later.
pub trait MusicProvider {
    /// Lightweight index of all library tracks, oldest date-added first.
    /// May print progress to stderr while loading.
    fn list_library_order_oldest_first(&self) -> Result<Vec<ResumeCursor>>;

    /// Full metadata for a single track (by persistent id).
    fn get_track(&self, id: &str) -> Result<TrackInfo>;

    fn play_track(&self, id: &str) -> Result<()>;
    fn seek(&self, seconds: f64) -> Result<()>;
    fn set_favorited(&self, id: &str, favorited: bool) -> Result<()>;
    /// Remove from the music library only (do not delete local files via Finder).
    fn remove_from_library(&self, id: &str) -> Result<()>;
    fn pause(&self) -> Result<()>;
}

/// Index of the first track to process after an optional progress cursor.
pub fn resume_start_index(order: &[ResumeCursor], after: Option<&ResumeCursor>) -> usize {
    let Some(after) = after else {
        return 0;
    };

    if let Some(i) = order.iter().position(|t| t.id == after.id) {
        return i + 1;
    }

    if after.date_added.is_some() {
        eprintln!(
            "Warning: progress track id not in library; resuming after saved date added."
        );
        return order
            .iter()
            .position(|t| ResumeCursor::cmp_order(t, after) == Ordering::Greater)
            .unwrap_or(order.len());
    }

    eprintln!(
        "Warning: progress track id not in library and no date added saved; \
         starting from oldest remaining."
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cur(id: &str, date: Option<&str>) -> ResumeCursor {
        ResumeCursor {
            id: id.to_string(),
            date_added: date.map(str::to_string),
        }
    }

    #[test]
    fn resume_from_start() {
        let order = vec![cur("a", Some("2020")), cur("b", Some("2021"))];
        assert_eq!(resume_start_index(&order, None), 0);
    }

    #[test]
    fn resume_after_id() {
        let order = vec![cur("a", Some("2020")), cur("b", Some("2021")), cur("c", Some("2022"))];
        assert_eq!(resume_start_index(&order, Some(&cur("a", Some("2020")))), 1);
        assert_eq!(resume_start_index(&order, Some(&cur("b", Some("2021")))), 2);
    }

    #[test]
    fn resume_missing_id_uses_date() {
        let order = vec![cur("a", Some("2020")), cur("c", Some("2022"))];
        let after = cur("gone", Some("2021"));
        assert_eq!(resume_start_index(&order, Some(&after)), 1);
    }
}
