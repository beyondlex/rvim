mod app;
mod ui;

use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, event::EnableBracketedPaste, event::DisableBracketedPaste};
use ratatui::prelude::*;

use crate::app::{handle_key, load_config, App, Mode};
use crate::ui::apply_cursor_style;

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableBracketedPaste, LeaveAlternateScreen);
    }
}

fn main() -> Result<()> {
    let path = std::env::args().nth(1).map(PathBuf::from);
    let content = match &path {
        Some(p) => fs::read_to_string(p).unwrap_or_default(),
        None => String::new(),
    };

    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;

    let mut app = App::new(path, content);
    if let Ok(cfg) = load_config() {
        app.apply_config(&cfg);
    }
    apply_cursor_style(&app)?;

    loop {
        app.clear_status_if_stale();
        terminal.draw(|f| ui::ui(f, &mut app))?;

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    if handle_key(&mut app, key)? {
                        break;
                    }
                }
                Event::Paste(text) => {
                    if app.mode == Mode::Insert {
                        app.insert_text(&text);
                    }
                }
                _ => {}
            }
            apply_cursor_style(&app)?;
        }
    }

    Ok(())
}
