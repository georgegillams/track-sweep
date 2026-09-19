use std::io::{self, IsTerminal, Write};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event;
use crossterm::style::{Color, Stylize};
use crossterm::{cursor, execute, terminal};

pub const MILESTONES: &[u32] = &[30, 100, 500, 1000];

const FRAME_COUNT: usize = 12;
const FRAME_MS: u64 = 65;
const FRAME_LINES: u16 = 4;

const SPARKLES: &[char] = &['✦', '✧', '★', '⋆', '*', '·'];
const COLORS: &[Color] = &[
    Color::Yellow,
    Color::Magenta,
    Color::Cyan,
    Color::Green,
    Color::DarkYellow,
    Color::Red,
];

pub fn is_milestone(n: u32) -> bool {
    MILESTONES.contains(&n)
}

pub fn message(n: u32) -> String {
    match n {
        30 => "Nice rhythm — 30 tracks this session!".into(),
        100 => "Century — 100 tracks sorted this session!".into(),
        500 => "On a tear — 500 tracks this session!".into(),
        1000 => "Unstoppable — 1,000 tracks this session!".into(),
        _ => format!("{n} tracks sorted this session!"),
    }
}

pub fn maybe_celebrate(n: u32) -> Result<()> {
    if !is_milestone(n) {
        return Ok(());
    }
    play(&message(n))
}

fn play(headline: &str) -> Result<()> {
    if !io::stdout().is_terminal() {
        print!("\r\n  {headline}\r\n");
        io::stdout().flush()?;
        return Ok(());
    }

    let _cursor = CursorGuard::hide()?;
    let mut out = io::stdout();
    for frame in 0..FRAME_COUNT {
        if frame > 0 {
            execute!(out, cursor::MoveUp(FRAME_LINES)).context("move cursor for celebration")?;
        }
        draw_frame(&mut out, frame, headline)?;
        out.flush()?;
        thread::sleep(Duration::from_millis(FRAME_MS));
    }
    drain_keys();
    Ok(())
}

fn draw_frame(out: &mut impl Write, frame: usize, headline: &str) -> Result<()> {
    cleared_line(out, "")?;
    cleared_line(
        out,
        &format!(
            "      {}     {}     {}",
            spark(frame, 0),
            spark(frame, 1),
            spark(frame, 2)
        ),
    )?;
    cleared_line(
        out,
        &format!(
            "   {}  {}  {}",
            spark(frame, 3),
            headline.bold().yellow(),
            spark(frame, 4)
        ),
    )?;
    cleared_line(
        out,
        &format!(
            "      {}     {}     {}",
            spark(frame, 5),
            spark(frame, 6),
            spark(frame, 7)
        ),
    )?;
    Ok(())
}

fn spark(frame: usize, slot: usize) -> crossterm::style::StyledContent<char> {
    let ch = SPARKLES[(frame.wrapping_mul(3) + slot * 5) % SPARKLES.len()];
    let color = COLORS[(frame + slot) % COLORS.len()];
    ch.with(color)
}

fn cleared_line(out: &mut impl Write, content: &str) -> Result<()> {
    execute!(
        out,
        cursor::MoveToColumn(0),
        terminal::Clear(terminal::ClearType::CurrentLine)
    )
    .context("clear celebration line")?;
    write!(out, "{content}\r\n")?;
    Ok(())
}

fn drain_keys() {
    while event::poll(Duration::from_millis(0)).unwrap_or(false) {
        let _ = event::read();
    }
}

struct CursorGuard;

impl CursorGuard {
    fn hide() -> Result<Self> {
        execute!(io::stdout(), cursor::Hide).context("hide cursor")?;
        Ok(Self)
    }
}

impl Drop for CursorGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), cursor::Show);
        let _ = io::stdout().flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milestones_are_the_expected_counts() {
        assert_eq!(MILESTONES, &[30, 100, 500, 1000]);
        for n in MILESTONES {
            assert!(is_milestone(*n));
        }
        for n in [0, 1, 29, 31, 99, 101, 499, 501, 999, 1001] {
            assert!(!is_milestone(n));
        }
    }

    #[test]
    fn messages_include_the_count() {
        assert!(message(30).contains("30"));
        assert!(message(100).contains("100"));
        assert!(message(500).contains("500"));
        assert!(message(1000).contains("1,000"));
    }
}
