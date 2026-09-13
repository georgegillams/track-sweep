use std::fs;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use crossterm::style::Stylize;

use crate::decisions::{self, Decision};
use crate::disliked::DislikedSet;
use crate::index_cache;
use crate::input::{self, Action, ActionFilter, CacheChoice, KeyReader};
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

    let mut disliked = DislikedSet::load()?;

    // Index (or load cache) before enabling raw mode so progress lines stay readable.
    let order = match load_or_build_index(provider, Arc::clone(&quit_flag))? {
        Some(order) => order,
        None => {
            offer_delete_disliked(provider, None, Arc::clone(&quit_flag), &mut disliked)?;
            return Ok(());
        }
    };
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
        offer_delete_disliked(provider, None, Arc::clone(&quit_flag), &mut disliked)?;
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
            offer_delete_disliked(provider, Some(&keys), Arc::clone(&quit_flag), &mut disliked)?;
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

        let action = loop {
            match keys.wait_for_action(filter)? {
                Action::SeekForward(0) => {}
                Action::SeekForward(seconds) => {
                    if let Err(err) = provider.seek_relative(seconds as f64) {
                        raw_println(&format!("  (seek warning: {err})"))?;
                    } else {
                        raw_println(&format!("  → {}", format!("+{seconds}s").cyan()))?;
                    }
                    print!("> ");
                    io::stdout().flush()?;
                }
                other => break other,
            }
        };
        match action {
            Action::Back => {
                raw_println(&format!("  → {}", "back".blue()))?;
                if pos == 0 {
                    continue;
                }
                pos -= 1;
                // Rewind progress so quit resumes on the track we're returning to.
                rewind_progress(&path, &order, pos)?;
                continue;
            }
            Action::Dislike => {
                raw_println(&format!("  → {}", "dislike".red()))?;
                provider.set_disliked(&track.id, true)?;
                disliked.add(&track)?;
                decisions::append(&decisions_path, &track, Decision::Dislike)?;
            }
            Action::Favourite => {
                raw_println(&format!("  → {}", "favourite".yellow()))?;
                provider.set_favorited(&track.id, true)?;
                disliked.remove_id(&track.id)?;
                decisions::append(&decisions_path, &track, Decision::Favourite)?;
            }
            Action::Unfavourite => {
                raw_println(&format!("  → {}", "unfavourite".magenta()))?;
                provider.set_favorited(&track.id, false)?;
                disliked.remove_id(&track.id)?;
                decisions::append(&decisions_path, &track, Decision::Unfavourite)?;
            }
            Action::Skip => {
                raw_println(&format!("  → {}", "keep".green()))?;
                if !track.favorited {
                    provider.set_disliked(&track.id, false)?;
                }
                disliked.remove_id(&track.id)?;
                decisions::append(&decisions_path, &track, Decision::Keep)?;
            }
            Action::Quit => {
                let _ = provider.pause();
                offer_delete_disliked(provider, Some(&keys), Arc::clone(&quit_flag), &mut disliked)?;
                raw_println("\nQuit. Run again to resume on this track.")?;
                return Ok(());
            }
            Action::SeekForward(_) => unreachable!("seek is handled before deciding the track"),
        }

        progress::save(&path, &ProgressEntry::from_track(&track))?;
        pos += 1;
    }

    let _ = provider.pause();
    offer_delete_disliked(provider, Some(&keys), Arc::clone(&quit_flag), &mut disliked)?;
    raw_println("\nDone — reached the end of the library.")?;
    Ok(())
}

/// Load a cached index, or re-index the library and write the cache.
/// Returns `None` if the user quits at the cache prompt.
fn load_or_build_index(
    provider: &dyn MusicProvider,
    quit_flag: Arc<AtomicBool>,
) -> Result<Option<Vec<ResumeCursor>>> {
    let path = index_cache::index_path();
    let cached = match index_cache::load(&path) {
        Ok(tracks) => tracks,
        Err(err) => {
            eprintln!("Warning: could not read index cache; will re-index.\n  {err:#}");
            None
        }
    };

    if let Some(tracks) = cached {
        match input::prompt_cache_or_reindex(quit_flag, tracks.len(), &path)? {
            CacheChoice::UseCache => {
                eprintln!(
                    "Using cached index ({} tracks, oldest → newest).",
                    tracks.len()
                );
                return Ok(Some(tracks));
            }
            CacheChoice::Quit => {
                println!("Quit.");
                return Ok(None);
            }
            CacheChoice::Reindex => {}
        }
    }

    let order = provider.list_library_order_oldest_first()?;
    if !order.is_empty() {
        index_cache::save(&path, &order)?;
        eprintln!("Wrote index cache to {}.", path.display());
    }
    Ok(Some(order))
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

fn offer_delete_disliked(
    provider: &dyn MusicProvider,
    keys: Option<&KeyReader>,
    quit_flag: Arc<AtomicBool>,
    pending: &mut DislikedSet,
) -> Result<()> {
    if pending.is_empty() {
        return Ok(());
    }

    match keys {
        Some(keys) => confirm_and_maybe_delete(provider, keys, pending),
        None => {
            let keys = KeyReader::new(quit_flag)?;
            confirm_and_maybe_delete(provider, &keys, pending)
        }
    }
}

fn confirm_and_maybe_delete(
    provider: &dyn MusicProvider,
    keys: &KeyReader,
    pending: &mut DislikedSet,
) -> Result<()> {
    let n = pending.len();
    let noun = if n == 1 { "track" } else { "tracks" };
    raw_println(&format!(
        "\nDelete {n} disliked {noun} from the library? [y/n]"
    ))?;
    for track in pending.tracks().iter().take(10) {
        raw_println(&format!("  {} — {}", track.name, track.artist))?;
    }
    if n > 10 {
        raw_println(&format!("  … and {} more", n - 10))?;
    }
    print!("> ");
    io::stdout().flush()?;

    if !keys.wait_for_yes_no()? {
        raw_println("  Left disliked tracks in the library.")?;
        return Ok(());
    }

    raw_println("  Removing from library…")?;
    let ids = pending.ids();
    let gone = provider.remove_from_library(&ids)?;
    pending.remove_ids(&gone)?;
    let leftover = pending.len();
    if leftover == 0 {
        raw_println(&format!("  Deleted {} {noun}.", gone.len()))?;
    } else {
        raw_println(&format!(
            "  Deleted {}; {leftover} could not be removed.",
            gone.len()
        ))?;
    }
    Ok(())
}

fn raw_println(msg: &str) -> Result<()> {
    print!("{msg}\r\n");
    io::stdout().flush()?;
    Ok(())
}

fn print_track(index: usize, total: usize, track: &TrackInfo, key_help: &str) -> Result<()> {
    let fav = if track.favorited {
        format!(" {}", "★".yellow())
    } else {
        String::new()
    };
    let title = track.name.as_str().bold().blue();
    let line = format!(
        "\r\n[{index}/{total}] {title} — {} ({}){fav}\r\n  {key_help}\r\n> ",
        track.artist, track.album
    );
    print!("{line}");
    io::stdout().flush()?;
    Ok(())
}
