use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::style::Stylize;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheChoice {
    UseCache,
    Reindex,
    Quit,
}

/// Ask whether to reuse a disk index. Enables raw mode only while waiting for
/// a key so later indexing progress stays readable. Enter defaults to cache.
pub fn prompt_cache_or_reindex(
    quit_flag: Arc<AtomicBool>,
    track_count: usize,
    cache_path: &Path,
) -> Result<CacheChoice> {
    println!(
        "Found cached index ({track_count} tracks) at {}.",
        cache_path.display()
    );
    println!("  [c] use cache (default)");
    println!("  [r] re-index library");
    println!("  [q] quit");
    print!("Choice [c]: ");
    io::stdout().flush().context("flush cache prompt")?;

    let choice = {
        let keys = KeyReader::new(quit_flag)?;
        keys.wait_for_cache_choice()?
    };
    println!();
    Ok(choice)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Dislike,
    Favourite,
    Unfavourite,
    Skip,
    Back,
    /// Skip playback forward by this many seconds without deciding the track.
    SeekForward(u32),
    Quit,
}

#[derive(Debug, Clone, Copy)]
pub struct ActionFilter {
    pub allow_favourite: bool,
    pub allow_unfavourite: bool,
    pub allow_back: bool,
}

impl ActionFilter {
    pub fn for_track(favorited: bool, allow_back: bool) -> Self {
        Self {
            allow_favourite: !favorited,
            allow_unfavourite: favorited,
            allow_back,
        }
    }

    pub fn key_help(self) -> String {
        let mut parts = vec![format!("[r] {}", "dislike".red())];
        if self.allow_favourite {
            parts.push(format!("[f] {}", "favourite".yellow()));
        }
        if self.allow_unfavourite {
            parts.push(format!("[n] {}", "unfavourite".magenta()));
        }
        parts.push(format!("[Space/Enter/y] {}", "keep".green()));
        parts.push(format!("[0-9] {}", "+10s×n".cyan()));
        if self.allow_back {
            parts.push(format!("[b] {}", "back".blue()));
        }
        parts.push(format!("[q] {}", "quit".dark_grey()));
        parts.join("  ")
    }
}

pub struct KeyReader {
    quit_flag: Arc<AtomicBool>,
}

impl KeyReader {
    pub fn new(quit_flag: Arc<AtomicBool>) -> Result<Self> {
        enable_raw_mode().context("failed to enable raw terminal mode")?;
        Ok(Self { quit_flag })
    }

    /// Block until the user chooses cache, re-index, or quit.
    pub fn wait_for_cache_choice(&self) -> Result<CacheChoice> {
        loop {
            if self.quit_flag.load(Ordering::SeqCst) {
                return Ok(CacheChoice::Quit);
            }

            if event::poll(Duration::from_millis(100)).context("poll keyboard")? {
                match event::read().context("read keyboard")? {
                    Event::Key(KeyEvent {
                        code,
                        modifiers,
                        kind,
                        ..
                    }) if kind == KeyEventKind::Press || kind == KeyEventKind::Repeat => {
                        if modifiers.contains(KeyModifiers::CONTROL)
                            && matches!(code, KeyCode::Char('c') | KeyCode::Char('C'))
                        {
                            return Ok(CacheChoice::Quit);
                        }

                        match code {
                            KeyCode::Char('q') | KeyCode::Char('Q') => {
                                return Ok(CacheChoice::Quit);
                            }
                            KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Enter => {
                                return Ok(CacheChoice::UseCache);
                            }
                            KeyCode::Char('r') | KeyCode::Char('R') => {
                                return Ok(CacheChoice::Reindex);
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Block until the user chooses an allowed action. Ignores unrelated keys.
    pub fn wait_for_action(&self, filter: ActionFilter) -> Result<Action> {
        loop {
            if self.quit_flag.load(Ordering::SeqCst) {
                return Ok(Action::Quit);
            }

            if event::poll(Duration::from_millis(100)).context("poll keyboard")? {
                match event::read().context("read keyboard")? {
                    Event::Key(KeyEvent {
                        code,
                        modifiers,
                        kind,
                        ..
                    }) if kind == KeyEventKind::Press || kind == KeyEventKind::Repeat => {
                        if modifiers.contains(KeyModifiers::CONTROL)
                            && matches!(code, KeyCode::Char('c') | KeyCode::Char('C'))
                        {
                            return Ok(Action::Quit);
                        }

                        match code {
                            KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(Action::Quit),
                            KeyCode::Char('r') | KeyCode::Char('R') => return Ok(Action::Dislike),
                            KeyCode::Char('f') | KeyCode::Char('F') if filter.allow_favourite => {
                                return Ok(Action::Favourite);
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') if filter.allow_unfavourite => {
                                return Ok(Action::Unfavourite);
                            }
                            KeyCode::Char('b') | KeyCode::Char('B') if filter.allow_back => {
                                return Ok(Action::Back);
                            }
                            KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => return Ok(Action::Skip),
                            KeyCode::Char(c) if c.is_ascii_digit() => {
                                let n = c.to_digit(10).expect("ascii digit");
                                return Ok(Action::SeekForward(n * 10));
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Block until the user answers yes or no. `q` / Ctrl-C count as no.
    pub fn wait_for_yes_no(&self) -> Result<bool> {
        loop {
            if self.quit_flag.load(Ordering::SeqCst) {
                return Ok(false);
            }

            if event::poll(Duration::from_millis(100)).context("poll keyboard")? {
                match event::read().context("read keyboard")? {
                    Event::Key(KeyEvent {
                        code,
                        modifiers,
                        kind,
                        ..
                    }) if kind == KeyEventKind::Press || kind == KeyEventKind::Repeat => {
                        if modifiers.contains(KeyModifiers::CONTROL)
                            && matches!(code, KeyCode::Char('c') | KeyCode::Char('C'))
                        {
                            return Ok(false);
                        }

                        match code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => return Ok(true),
                            KeyCode::Char('n')
                            | KeyCode::Char('N')
                            | KeyCode::Char('q')
                            | KeyCode::Char('Q') => return Ok(false),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

impl Drop for KeyReader {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = io::stdout().flush();
    }
}
