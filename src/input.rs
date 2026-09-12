use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Remove,
    Favourite,
    Unfavourite,
    Skip,
    Back,
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
        let mut parts = vec!["[r] remove".to_string()];
        if self.allow_favourite {
            parts.push("[f] favourite".to_string());
        }
        if self.allow_unfavourite {
            parts.push("[n] unfavourite".to_string());
        }
        parts.push("[Space/Enter] keep".to_string());
        if self.allow_back {
            parts.push("[b] back".to_string());
        }
        parts.push("[q] quit".to_string());
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
                            KeyCode::Char('r') | KeyCode::Char('R') => return Ok(Action::Remove),
                            KeyCode::Char('f') | KeyCode::Char('F') if filter.allow_favourite => {
                                return Ok(Action::Favourite);
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') if filter.allow_unfavourite => {
                                return Ok(Action::Unfavourite);
                            }
                            KeyCode::Char('b') | KeyCode::Char('B') if filter.allow_back => {
                                return Ok(Action::Back);
                            }
                            KeyCode::Char(' ') | KeyCode::Enter => return Ok(Action::Skip),
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
