use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::decisions::{self, Decision};
use crate::input::{Action, ActionFilter, KeyReader};
use crate::progress::{self, progress_path, ProgressEntry};
use crate::provider::{self, MusicProvider, ResumeCursor, TrackInfo};

const SEEK_SECONDS: f64 = 30.0;

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
    println!("Keys depend on each track (favourite vs unfavourite).\n");

    let keys = KeyReader::new(Arc::clone(&quit_flag))?;

    for (pos, entry) in order.iter().enumerate().skip(start) {
        if quit_flag.load(Ordering::SeqCst) {
            let _ = provider.pause();
            raw_println("\nQuit. Progress saved; run again to resume.")?;
            return Ok(());
        }

        let track = match provider.get_track(&entry.id) {
            Ok(t) => t,
            Err(err) => {
                raw_println(&format!(
                    "\r\n[{}/{}] skip missing track {}: {err}",
                    pos + 1,
                    total,
                    entry.id
                ))?;
                progress::save(
                    &path,
                    &ProgressEntry {
                        id: entry.id.clone(),
                        date_added: entry.date_added.clone(),
                        name: String::new(),
                        artist: String::new(),
                    },
                )?;
                continue;
            }
        };

        let filter = ActionFilter::for_favorited(track.favorited);
        print_track(pos + 1, total, &track, &filter.key_help())?;

        provider
            .play_track(&track.id)
            .with_context(|| format!("play failed for {}", track.id))?;

        let seek_to = if track.duration_secs > 0.0 && SEEK_SECONDS >= track.duration_secs {
            (track.duration_secs - 1.0).max(0.0)
        } else {
            SEEK_SECONDS
        };
        std::thread::sleep(std::time::Duration::from_millis(250));
        if let Err(err) = provider.seek(seek_to) {
            raw_println(&format!("  (seek warning: {err})"))?;
        }

        let action = keys.wait_for_action(filter)?;
        let decision = match action {
            Action::Remove => {
                raw_println("  → remove")?;
                provider.remove_from_library(&track.id)?;
                Some(Decision::Remove)
            }
            Action::Favourite => {
                raw_println("  → favourite")?;
                provider.set_favorited(&track.id, true)?;
                Some(Decision::Favourite)
            }
            Action::Unfavourite => {
                raw_println("  → unfavourite")?;
                provider.set_favorited(&track.id, false)?;
                Some(Decision::Unfavourite)
            }
            Action::Skip => {
                raw_println("  → keep")?;
                Some(Decision::Keep)
            }
            Action::Quit => {
                let _ = provider.pause();
                raw_println("\nQuit. Run again to resume on this track.")?;
                return Ok(());
            }
        };

        if let Some(decision) = decision {
            decisions::append(&decisions_path, &track, decision)?;
            progress::save(&path, &ProgressEntry::from_track(&track))?;
        }
    }

    let _ = provider.pause();
    raw_println("\nDone — reached the end of the library.")?;
    Ok(())
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
