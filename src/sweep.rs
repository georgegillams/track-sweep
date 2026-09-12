use std::fs;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::decisions::{self, Decision};
use crate::input::{Action, ActionFilter, KeyReader};
use crate::progress::{self, progress_path, ProgressEntry};
use crate::provider::{self, MusicProvider, ResumeCursor, TrackInfo};

const SEEK_SECONDS: f64 = 30.0;
const SEEK_MIN_DURATION_SECS: f64 = 60.0;

pub fn run(provider: &dyn MusicProvider) -> Result<()> {
    let quit_flag = Arc::new(AtomicBool::new(false));
    {
        let flag = Arc::clone(&quit_flag);
        ctrlc::set_handler(move || {
            flag.store(true, Ordering::SeqCst);
        })
        .context("failed to install Ctrl-C handler")?;
    }

    // Index before enabling raw mode so progress lines stay readable.
    let order = provider.list_library_order_oldest_first()?;
    if order.is_empty() {
        println!("Library is empty — nothing to sweep.");
        return Ok(());
    }

    let path = progress_path();
    let decisions_path = decisions::decisions_path();
    let saved = progress::load(&path)?;
    let cursor = saved.as_ref().map(|e| ResumeCursor {
        id: e.id.clone(),
        date_added: e.date_added.clone(),
    });

    if let Some(entry) = &saved {
        let label = if entry.name.is_empty() {
            entry.id.clone()
        } else {
            format!("{} — {}", entry.name, entry.artist)
        };
        eprintln!("Resuming after: {label}");
    }

    let start = provider::resume_start_index(&order, cursor.as_ref());
    if start >= order.len() {
        println!("Nothing left after saved progress — reached the end of the library.");
        return Ok(());
    }

    let total = order.len();
    println!(
        "Sweeping {} of {total} tracks (oldest → newest).",
        total - start
    );
    println!("Decisions append to {}.", decisions_path.display());
    println!("Keys depend on each track (favourite vs unfavourite). [b] goes back.\n");

    let keys = KeyReader::new(Arc::clone(&quit_flag))?;
    let mut pos = start;

    while pos < total {
        if quit_flag.load(Ordering::SeqCst) {
            let _ = provider.pause();
            raw_println("\nQuit. Progress saved; run again to resume.")?;
            return Ok(());
        }

        let entry = &order[pos];
        let track = match provider.get_track(&entry.id) {
            Ok(t) => t,
            Err(err) => {
                raw_println(&format!(
                    "\r\n[{}/{}] skip missing track {}: {err}",
                    pos + 1,
                    total,
                    entry.id
                ))?;
                // Missing track: treat as completed and move forward (unless we came from back,
                // still advance so we don't get stuck).
                progress::save(
                    &path,
                    &ProgressEntry {
                        id: entry.id.clone(),
                        date_added: entry.date_added.clone(),
                        name: String::new(),
                        artist: String::new(),
                    },
                )?;
                pos += 1;
                continue;
            }
        };

        let filter = ActionFilter::for_track(track.favorited, pos > 0);
        print_track(pos + 1, total, &track, &filter.key_help())?;

        provider
            .play_track(&track.id)
            .with_context(|| format!("play failed for {}", track.id))?;

        if track.duration_secs > SEEK_MIN_DURATION_SECS {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if let Err(err) = provider.seek(SEEK_SECONDS) {
                raw_println(&format!("  (seek warning: {err})"))?;
            }
        }

        let action = keys.wait_for_action(filter)?;
        match action {
            Action::Back => {
                raw_println("  → back")?;
                if pos == 0 {
                    continue;
                }
                pos -= 1;
                // Rewind progress so quit resumes on the track we're returning to.
                rewind_progress(&path, &order, pos)?;
                continue;
            }
            Action::Remove => {
                raw_println("  → remove")?;
                provider.remove_from_library(&track.id)?;
                decisions::append(&decisions_path, &track, Decision::Remove)?;
            }
            Action::Favourite => {
                raw_println("  → favourite")?;
                provider.set_favorited(&track.id, true)?;
                decisions::append(&decisions_path, &track, Decision::Favourite)?;
            }
            Action::Unfavourite => {
                raw_println("  → unfavourite")?;
                provider.set_favorited(&track.id, false)?;
                decisions::append(&decisions_path, &track, Decision::Unfavourite)?;
            }
            Action::Skip => {
                raw_println("  → keep")?;
                decisions::append(&decisions_path, &track, Decision::Keep)?;
            }
            Action::Quit => {
                let _ = provider.pause();
                raw_println("\nQuit. Run again to resume on this track.")?;
                return Ok(());
            }
        }

        progress::save(&path, &ProgressEntry::from_track(&track))?;
        pos += 1;
    }

    let _ = provider.pause();
    raw_println("\nDone — reached the end of the library.")?;
    Ok(())
}

/// Set progress so resume starts at `current_pos` (last completed = prior track, or cleared).
fn rewind_progress(
    path: &std::path::Path,
    order: &[ResumeCursor],
    current_pos: usize,
) -> Result<()> {
    if current_pos == 0 {
        if path.exists() {
            fs::remove_file(path)
                .with_context(|| format!("failed to clear {}", path.display()))?;
        }
        return Ok(());
    }

    let prev = &order[current_pos - 1];
    progress::save(
        path,
        &ProgressEntry {
            id: prev.id.clone(),
            date_added: prev.date_added.clone(),
            name: String::new(),
            artist: String::new(),
        },
    )
}

fn raw_println(msg: &str) -> Result<()> {
    print!("{msg}\r\n");
    io::stdout().flush()?;
    Ok(())
}

fn print_track(index: usize, total: usize, track: &TrackInfo, key_help: &str) -> Result<()> {
    let fav = if track.favorited { " ★" } else { "" };
    let line = format!(
        "\r\n[{index}/{total}] {} — {} ({}){fav}\r\n  {key_help}\r\n> ",
        track.name, track.artist, track.album
    );
    print!("{line}");
    io::stdout().flush()?;
    Ok(())
}
