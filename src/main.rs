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
    Command,
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
    command_buffer: String,
    pending_g: bool,
    pending_find: Option<FindPending>,
    last_find: Option<FindSpec>,
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
            command_buffer: String::new(),
            pending_g: false,
            pending_find: None,
            last_find: None,
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

    fn char_at(&self, row: usize, col: usize) -> Option<char> {
        self.lines.get(row).and_then(|l| l.chars().nth(col))
    }

    fn class_at(&self, row: usize, col: usize) -> Option<CharClass> {
        let len = self.line_len(row);
        if col == len {
            return Some(CharClass::Space);
        }
        if col > len {
            return None;
        }
        self.char_at(row, col).map(char_class)
    }

    fn advance_pos(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        let len = self.line_len(row);
        if col < len {
            Some((row, col + 1))
        } else if row + 1 < self.lines.len() {
            Some((row + 1, 0))
        } else {
            None
        }
    }

    fn prev_pos(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        if row == 0 && col == 0 {
            return None;
        }
        if col > 0 {
            return Some((row, col - 1));
        }
        if row == 0 {
            return None;
        }
        let prev_row = row - 1;
        let prev_len = self.line_len(prev_row);
        if prev_len == 0 {
            Some((prev_row, 0))
        } else {
            Some((prev_row, prev_len - 1))
        }
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

    fn move_line_start(&mut self) {
        self.cursor_col = 0;
    }

    fn move_line_end(&mut self) {
        let len = self.line_len(self.cursor_row);
        self.cursor_col = if len == 0 { 0 } else { len - 1 };
    }

    fn move_to_top(&mut self) {
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    fn move_to_bottom(&mut self) {
        if self.lines.is_empty() {
            self.cursor_row = 0;
            self.cursor_col = 0;
            return;
        }
        self.cursor_row = self.lines.len() - 1;
        self.cursor_col = 0;
    }

    fn move_word_forward(&mut self) {
        if let Some((row, col)) = self.next_word_start(self.cursor_row, self.cursor_col) {
            self.cursor_row = row;
            self.cursor_col = col;
        }
    }

    fn move_word_end(&mut self) {
        if let Some((row, col)) = self.next_word_end(self.cursor_row, self.cursor_col) {
            self.cursor_row = row;
            self.cursor_col = col;
        }
    }

    fn move_word_back(&mut self) {
        if let Some((row, col)) = self.prev_word_start(self.cursor_row, self.cursor_col) {
            self.cursor_row = row;
            self.cursor_col = col;
        }
    }

    fn find_forward(&mut self, target: char, until: bool) -> bool {
        let mut row = self.cursor_row;
        let mut col = self.cursor_col + 1;

        while row < self.lines.len() {
            let line = &self.lines[row];
            for (idx, ch) in line.chars().enumerate() {
                if row == self.cursor_row && idx < col {
                    continue;
                }
                if ch == target {
                    let mut target_pos = (row, idx);
                    if until {
                        if let Some(prev) = self.prev_pos(row, idx) {
                            target_pos = prev;
                        }
                    }
                    self.cursor_row = target_pos.0;
                    self.cursor_col = target_pos.1;
                    return true;
                }
            }
            row += 1;
            col = 0;
        }
        false
    }

    fn find_backward(&mut self, target: char, until: bool) -> bool {
        if self.lines.is_empty() {
            return false;
        }
        let mut row = self.cursor_row;
        let mut col = self.cursor_col;

        loop {
            let line = &self.lines[row];
            let mut last_match: Option<usize> = None;
            for (idx, ch) in line.chars().enumerate() {
                if row == self.cursor_row && idx >= col {
                    break;
                }
                if ch == target {
                    last_match = Some(idx);
                }
            }
            if let Some(idx) = last_match {
                let mut target_pos = (row, idx);
                if until {
                    if let Some(next) = self.advance_pos(row, idx) {
                        target_pos = next;
                    }
                }
                self.cursor_row = target_pos.0;
                self.cursor_col = target_pos.1;
                return true;
            }
            if row == 0 {
                break;
            }
            row -= 1;
            col = self.line_len(row);
        }
        false
    }

    fn next_word_start(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        let cur = self.class_at(row, col)?;
        if cur == CharClass::Space {
            return self.skip_spaces_forward(row, col);
        }

        let after = self.advance_to_next_class(row, col, cur)?;
        self.skip_spaces_forward(after.0, after.1)
    }

    fn next_word_end(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        let cur = self.class_at(row, col)?;
        if cur == CharClass::Space {
            let (sr, sc) = self.skip_spaces_forward(row, col)?;
            let cls = self.class_at(sr, sc)?;
            return Some(self.end_of_group(sr, sc, cls));
        }

        let end = self.end_of_group(row, col, cur);
        if end != (row, col) {
            return Some(end);
        }

        let next = self.advance_pos(row, col)?;
        let (sr, sc) = self.skip_spaces_forward(next.0, next.1)?;
        let cls = self.class_at(sr, sc)?;
        Some(self.end_of_group(sr, sc, cls))
    }

    fn prev_word_start(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        let cur = self.class_at(row, col)?;
        if cur == CharClass::Space {
            let (r, c) = self.skip_spaces_backward(row, col)?;
            let cls = self.class_at(r, c)?;
            return Some(self.start_of_group(r, c, cls));
        }

        if self.is_group_start(row, col, cur) {
            let prev = self.prev_pos(row, col)?;
            let (r, c) = self.skip_spaces_backward(prev.0, prev.1)?;
            let cls = self.class_at(r, c)?;
            return Some(self.start_of_group(r, c, cls));
        }

        Some(self.start_of_group(row, col, cur))
    }

    fn skip_spaces_forward(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        let mut r = row;
        let mut c = col;
        loop {
            match self.class_at(r, c) {
                Some(CharClass::Space) => {
                    let next = self.advance_pos(r, c)?;
                    r = next.0;
                    c = next.1;
                }
                Some(_) => return Some((r, c)),
                None => return None,
            }
        }
    }

    fn skip_spaces_backward(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        let mut r = row;
        let mut c = col;
        loop {
            match self.class_at(r, c) {
                Some(CharClass::Space) => {
                    let prev = self.prev_pos(r, c)?;
                    r = prev.0;
                    c = prev.1;
                }
                Some(_) => return Some((r, c)),
                None => return None,
            }
        }
    }

    fn advance_to_next_class(
        &self,
        row: usize,
        col: usize,
        class: CharClass,
    ) -> Option<(usize, usize)> {
        let mut r = row;
        let mut c = col;
        loop {
            let next = self.advance_pos(r, c)?;
            match self.class_at(next.0, next.1) {
                Some(next_class) if next_class == class => {
                    r = next.0;
                    c = next.1;
                }
                Some(_) => return Some(next),
                None => return None,
            }
        }
    }

    fn start_of_group(&self, row: usize, col: usize, class: CharClass) -> (usize, usize) {
        let mut r = row;
        let mut c = col;
        while let Some((pr, pc)) = self.prev_pos(r, c) {
            if self.class_at(pr, pc) == Some(class) {
                r = pr;
                c = pc;
            } else {
                break;
            }
        }
        (r, c)
    }

    fn end_of_group(&self, row: usize, col: usize, class: CharClass) -> (usize, usize) {
        let mut r = row;
        let mut c = col;
        while let Some((nr, nc)) = self.advance_pos(r, c) {
            if self.class_at(nr, nc) == Some(class) {
                r = nr;
                c = nc;
            } else {
                break;
            }
        }
        (r, c)
    }

    fn is_group_start(&self, row: usize, col: usize, class: CharClass) -> bool {
        match self.prev_pos(row, col) {
            Some((pr, pc)) => self.class_at(pr, pc) != Some(class),
            None => true,
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

    fn reload(&mut self, path: &PathBuf) -> Result<()> {
        let content = fs::read_to_string(path).unwrap_or_default();
        self.lines = content.lines().map(|s| s.to_string()).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.scroll_row = 0;
        self.scroll_col = 0;
        self.dirty = false;
        Ok(())
    }

    fn execute_command(&mut self) -> Result<bool> {
        let input = self.command_buffer.trim();
        if input.is_empty() {
            return Ok(false);
        }

        let mut parts = input.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        let arg = parts.next();

        match cmd {
            "w" | "write" => {
                self.save()?;
            }
            "q" | "quit" => {
                if self.dirty {
                    self.set_status("No write since last change (add ! to override)");
                    return Ok(false);
                }
                return Ok(true);
            }
            "q!" | "quit!" => {
                return Ok(true);
            }
            "wq" | "x" => {
                self.save()?;
                return Ok(true);
            }
            "e" | "edit" => {
                if let Some(path) = arg.map(PathBuf::from) {
                    self.file_path = Some(path.clone());
                    self.reload(&path)?;
                    self.set_status(format!("Opened {}", path.display()));
                } else {
                    self.set_status("Usage: :e <path>");
                }
            }
            _ => {
                self.set_status(format!("Not an editor command: {}", cmd));
            }
        }

        Ok(false)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Space,
    Word,
    Punct,
}

fn char_class(ch: char) -> CharClass {
    if ch.is_whitespace() {
        CharClass::Space
    } else if ch.is_alphanumeric() || ch == '_' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

#[derive(Debug, Clone, Copy)]
struct FindPending {
    until: bool,
    reverse: bool,
}

#[derive(Debug, Clone, Copy)]
struct FindSpec {
    ch: char,
    until: bool,
    reverse: bool,
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
    if app.pending_g && !(matches!(key.code, KeyCode::Char('g')) && key.modifiers == KeyModifiers::NONE)
    {
        app.pending_g = false;
    }
    if app.mode == Mode::Normal {
        if let Some(pending) = app.pending_find.take() {
            if let KeyCode::Char(ch) = key.code {
                let found = if pending.reverse {
                    app.find_backward(ch, pending.until)
                } else {
                    app.find_forward(ch, pending.until)
                };
                if !found {
                    app.set_status("Pattern not found");
                } else {
                    app.last_find = Some(FindSpec {
                        ch,
                        until: pending.until,
                        reverse: pending.reverse,
                    });
                }
            }
            return Ok(false);
        }
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
            (KeyCode::Char(':'), KeyModifiers::NONE) => {
                app.mode = Mode::Command;
                app.command_buffer.clear();
            }
            (KeyCode::Char('h'), KeyModifiers::NONE) | (KeyCode::Left, _) => app.move_left(),
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, _) => app.move_down(),
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, _) => app.move_up(),
            (KeyCode::Char('l'), KeyModifiers::NONE) | (KeyCode::Right, _) => app.move_right(),
            (KeyCode::Char('w'), KeyModifiers::NONE) => app.move_word_forward(),
            (KeyCode::Char('b'), KeyModifiers::NONE) => app.move_word_back(),
            (KeyCode::Char('e'), KeyModifiers::NONE) => app.move_word_end(),
            (KeyCode::Char('0'), KeyModifiers::NONE) => app.move_line_start(),
            (KeyCode::Char('$'), _) => app.move_line_end(),
            (KeyCode::Char('g'), KeyModifiers::NONE) => {
                if app.pending_g {
                    app.move_to_top();
                    app.pending_g = false;
                } else {
                    app.pending_g = true;
                }
            }
            (KeyCode::Char('G'), _) => app.move_to_bottom(),
            (KeyCode::Char('f'), KeyModifiers::NONE) => {
                app.pending_find = Some(FindPending {
                    until: false,
                    reverse: false,
                });
            }
            (KeyCode::Char('t'), KeyModifiers::NONE) => {
                app.pending_find = Some(FindPending {
                    until: true,
                    reverse: false,
                });
            }
            (KeyCode::Char('F'), _) => {
                app.pending_find = Some(FindPending {
                    until: false,
                    reverse: true,
                });
            }
            (KeyCode::Char('T'), _) => {
                app.pending_find = Some(FindPending {
                    until: true,
                    reverse: true,
                });
            }
            (KeyCode::Char(';'), KeyModifiers::NONE) => {
                if let Some(spec) = app.last_find {
                    let found = if spec.reverse {
                        app.find_backward(spec.ch, spec.until)
                    } else {
                        app.find_forward(spec.ch, spec.until)
                    };
                    if !found {
                        app.set_status("Pattern not found");
                    }
                }
            }
            (KeyCode::Char(','), KeyModifiers::NONE) => {
                if let Some(spec) = app.last_find {
                    let found = if spec.reverse {
                        app.find_forward(spec.ch, spec.until)
                    } else {
                        app.find_backward(spec.ch, spec.until)
                    };
                    if !found {
                        app.set_status("Pattern not found");
                    }
                }
            }
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
        Mode::Command => match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                app.mode = Mode::Normal;
                app.command_buffer.clear();
            }
            (KeyCode::Enter, _) => {
                let should_quit = app.execute_command()?;
                app.command_buffer.clear();
                app.mode = Mode::Normal;
                if should_quit {
                    return Ok(true);
                }
            }
            (KeyCode::Backspace, _) => {
                app.command_buffer.pop();
            }
            (KeyCode::Char(ch), KeyModifiers::NONE) => {
                app.command_buffer.push(ch);
            }
            (KeyCode::Char(ch), KeyModifiers::SHIFT) => {
                app.command_buffer.push(ch);
            }
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
        Mode::Command => "COMMAND",
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

    let message = if app.mode == Mode::Command {
        Paragraph::new(format!(":{}", app.command_buffer))
    } else {
        Paragraph::new(app.status_message.clone())
    };
    f.render_widget(message, message_area);

    if app.mode == Mode::Command {
        let cursor_x = message_area.x + 1 + app.command_buffer.chars().count() as u16;
        let cursor_y = message_area.y;
        if cursor_x < message_area.right() && cursor_y < message_area.bottom() {
            f.set_cursor_position(Position::new(cursor_x, cursor_y));
        }
    } else {
        let cursor_x = (app.cursor_col.saturating_sub(app.scroll_col)) as u16 + main_area.x;
        let cursor_y = (app.cursor_row.saturating_sub(app.scroll_row)) as u16 + main_area.y;
        if cursor_x < main_area.right() && cursor_y < main_area.bottom() {
            f.set_cursor_position(Position::new(cursor_x, cursor_y));
        }
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
