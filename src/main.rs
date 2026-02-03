use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, terminal};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Insert,
}

struct App {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
    scroll_row: usize,
    scroll_col: usize,
    mode: Mode,
    file_path: Option<PathBuf>,
    dirty: bool,
    quit_confirm: bool,
    status_message: String,
    status_time: Option<Instant>,
}

impl App {
    fn new(file_path: Option<PathBuf>, content: String) -> Self {
        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Self {
            lines,
            cursor_row: 0,
            cursor_col: 0,
            scroll_row: 0,
            scroll_col: 0,
            mode: Mode::Normal,
            file_path,
            dirty: false,
            quit_confirm: false,
            status_message: String::new(),
            status_time: None,
        }
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = msg.into();
        self.status_time = Some(Instant::now());
    }

    fn clear_status_if_stale(&mut self) {
        if let Some(t) = self.status_time {
            if t.elapsed() > Duration::from_secs(5) {
                self.status_message.clear();
                self.status_time = None;
            }
        }
    }

    fn line_len(&self, row: usize) -> usize {
        self.lines
            .get(row)
            .map(|l| l.chars().count())
            .unwrap_or(0)
    }

    fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.line_len(self.cursor_row);
        }
    }

    fn move_right(&mut self) {
        let len = self.line_len(self.cursor_row);
        if self.cursor_col < len {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
    }

    fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            let len = self.line_len(self.cursor_row);
            self.cursor_col = self.cursor_col.min(len);
        }
    }

    fn move_down(&mut self) {
        if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            let len = self.line_len(self.cursor_row);
            self.cursor_col = self.cursor_col.min(len);
        }
    }

    fn insert_char(&mut self, ch: char) {
        let line = &mut self.lines[self.cursor_row];
        let byte_idx = char_to_byte_idx(line, self.cursor_col);
        line.insert(byte_idx, ch);
        self.cursor_col += 1;
        self.dirty = true;
    }

    fn insert_newline(&mut self) {
        let line = &mut self.lines[self.cursor_row];
        let byte_idx = char_to_byte_idx(line, self.cursor_col);
        let right = line.split_off(byte_idx);
        self.lines.insert(self.cursor_row + 1, right);
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.dirty = true;
    }

    fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let line = &mut self.lines[self.cursor_row];
            let byte_idx = char_to_byte_idx(line, self.cursor_col);
            let prev_idx = char_to_byte_idx(line, self.cursor_col - 1);
            line.replace_range(prev_idx..byte_idx, "");
            self.cursor_col -= 1;
            self.dirty = true;
        } else if self.cursor_row > 0 {
            let current = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            let prev_line = &mut self.lines[self.cursor_row];
            let prev_len = prev_line.chars().count();
            prev_line.push_str(&current);
            self.cursor_col = prev_len;
            self.dirty = true;
        }
    }

    fn delete_at_cursor(&mut self) {
        let len = self.line_len(self.cursor_row);
        if self.cursor_col < len {
            let line = &mut self.lines[self.cursor_row];
            let byte_idx = char_to_byte_idx(line, self.cursor_col);
            let next_idx = char_to_byte_idx(line, self.cursor_col + 1);
            line.replace_range(byte_idx..next_idx, "");
            self.dirty = true;
        } else if self.cursor_row + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor_row + 1);
            let line = &mut self.lines[self.cursor_row];
            line.push_str(&next);
            self.dirty = true;
        }
    }

    fn open_line_below(&mut self) {
        self.lines.insert(self.cursor_row + 1, String::new());
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.dirty = true;
    }

    fn open_line_above(&mut self) {
        self.lines.insert(self.cursor_row, String::new());
        self.cursor_col = 0;
        self.dirty = true;
    }

    fn save(&mut self) -> Result<()> {
        let Some(path) = self.file_path.clone() else {
            self.set_status("No file name (open with a path)");
            return Ok(());
        };
        let content = self.lines.join("\n");
        fs::write(&path, content)?;
        self.dirty = false;
        self.set_status(format!("Wrote {}", path.display()));
        Ok(())
    }

    fn ensure_cursor_visible(&mut self, viewport_rows: usize, viewport_cols: usize) {
        if self.cursor_row < self.scroll_row {
            self.scroll_row = self.cursor_row;
        } else if self.cursor_row >= self.scroll_row + viewport_rows {
            self.scroll_row = self.cursor_row.saturating_sub(viewport_rows - 1);
        }

        if self.cursor_col < self.scroll_col {
            self.scroll_col = self.cursor_col;
        } else if self.cursor_col >= self.scroll_col + viewport_cols {
            self.scroll_col = self.cursor_col.saturating_sub(viewport_cols - 1);
        }
    }
}

fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or_else(|| s.len())
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
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

    loop {
        app.clear_status_if_stale();
        terminal.draw(|f| ui(f, &mut app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if handle_key(&mut app, key)? {
                    break;
                }
            }
        }
    }

    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent) -> Result<bool> {
    let is_quit = matches!(key.code, KeyCode::Char('q')) && key.modifiers == KeyModifiers::CONTROL;
    if !is_quit {
        app.quit_confirm = false;
    }

    match app.mode {
        Mode::Normal => match (key.code, key.modifiers) {
            (KeyCode::Char('q'), KeyModifiers::CONTROL) => {
                if app.dirty && !app.quit_confirm {
                    app.quit_confirm = true;
                    app.set_status("Unsaved changes. Press Ctrl+Q again to quit.");
                    return Ok(false);
                }
                return Ok(true);
            }
            (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                app.save()?;
            }
            (KeyCode::Char('i'), KeyModifiers::NONE) => {
                app.mode = Mode::Insert;
                app.set_status("-- INSERT --");
            }
            (KeyCode::Char('h'), KeyModifiers::NONE) | (KeyCode::Left, _) => app.move_left(),
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, _) => app.move_down(),
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, _) => app.move_up(),
            (KeyCode::Char('l'), KeyModifiers::NONE) | (KeyCode::Right, _) => app.move_right(),
            (KeyCode::Char('x'), KeyModifiers::NONE) => app.delete_at_cursor(),
            (KeyCode::Char('o'), KeyModifiers::NONE) => {
                app.open_line_below();
                app.mode = Mode::Insert;
            }
            (KeyCode::Char('O'), _) => {
                app.open_line_above();
                app.mode = Mode::Insert;
            }
            _ => {}
        },
        Mode::Insert => match (key.code, key.modifiers) {
            (KeyCode::Char('q'), KeyModifiers::CONTROL) => {
                if app.dirty && !app.quit_confirm {
                    app.quit_confirm = true;
                    app.set_status("Unsaved changes. Press Ctrl+Q again to quit.");
                    return Ok(false);
                }
                return Ok(true);
            }
            (KeyCode::Esc, _) => {
                app.mode = Mode::Normal;
                app.set_status("-- NORMAL --");
            }
            (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                app.save()?;
            }
            (KeyCode::Enter, _) => app.insert_newline(),
            (KeyCode::Backspace, _) => app.backspace(),
            (KeyCode::Delete, _) => app.delete_at_cursor(),
            (KeyCode::Tab, _) => {
                for _ in 0..4 {
                    app.insert_char(' ');
                }
            }
            (KeyCode::Char(ch), KeyModifiers::NONE) => app.insert_char(ch),
            (KeyCode::Char(ch), KeyModifiers::SHIFT) => app.insert_char(ch),
            (KeyCode::Left, _) => app.move_left(),
            (KeyCode::Right, _) => app.move_right(),
            (KeyCode::Up, _) => app.move_up(),
            (KeyCode::Down, _) => app.move_down(),
            _ => {}
        },
    }

    Ok(false)
}

fn ui(f: &mut Frame<'_>, app: &mut App) {
    let size = f.area();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)])
        .split(size);

    let main_area = rows[0];
    let status_area = rows[1];
    let message_area = rows[2];

    let viewport_rows = main_area.height as usize;
    let viewport_cols = main_area.width as usize;
    app.ensure_cursor_visible(viewport_rows, viewport_cols);

    let mut text_lines: Vec<Line> = Vec::with_capacity(viewport_rows);
    for i in 0..viewport_rows {
        let idx = app.scroll_row + i;
        if let Some(line) = app.lines.get(idx) {
            let slice = slice_line(line, app.scroll_col, viewport_cols);
            text_lines.push(Line::from(slice));
        } else {
            text_lines.push(Line::from("~"));
        }
    }

    let paragraph = Paragraph::new(text_lines).block(Block::default().borders(Borders::NONE));
    f.render_widget(paragraph, main_area);

    let mode_label = match app.mode {
        Mode::Normal => "NORMAL",
        Mode::Insert => "INSERT",
    };
    let file_label = app
        .file_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "[No Name]".to_string());
    let dirty = if app.dirty { " [+]" } else { "" };
    let status = format!(
        "{} | {}{} | {}:{}",
        mode_label,
        file_label,
        dirty,
        app.cursor_row + 1,
        app.cursor_col + 1
    );

    let status_paragraph = Paragraph::new(status)
        .style(Style::default().fg(Color::Black).bg(Color::White));
    f.render_widget(status_paragraph, status_area);

    let message = Paragraph::new(app.status_message.clone());
    f.render_widget(message, message_area);

    let cursor_x = (app.cursor_col.saturating_sub(app.scroll_col)) as u16 + main_area.x;
    let cursor_y = (app.cursor_row.saturating_sub(app.scroll_row)) as u16 + main_area.y;
    if cursor_x < main_area.right() && cursor_y < main_area.bottom() {
        f.set_cursor_position(Position::new(cursor_x, cursor_y));
    }
}

fn slice_line(line: &str, start_col: usize, max_cols: usize) -> String {
    let mut out = String::new();
    let mut col = 0;
    for ch in line.chars() {
        if col >= start_col && out.chars().count() < max_cols {
            out.push(ch);
        }
        col += 1;
        if out.chars().count() >= max_cols {
            break;
        }
    }
    out
}
