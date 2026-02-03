use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Command,
    VisualChar,
    VisualLine,
    VisualBlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operator {
    Delete,
    Yank,
    Change,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum YankType {
    Char,
    Line,
    Block,
}

#[derive(Debug, Clone, Copy)]
struct OperatorPending {
    op: Operator,
    start_row: usize,
    start_col: usize,
}

pub struct App {
    pub(crate) lines: Vec<String>,
    pub(crate) cursor_row: usize,
    pub(crate) cursor_col: usize,
    pub(crate) scroll_row: usize,
    pub(crate) scroll_col: usize,
    pub(crate) mode: Mode,
    pub(crate) file_path: Option<PathBuf>,
    pub(crate) dirty: bool,
    pub(crate) status_message: String,
    pub(crate) command_buffer: String,
    quit_confirm: bool,
    status_time: Option<Instant>,
    pending_g: bool,
    pending_find: Option<FindPending>,
    last_find: Option<FindSpec>,
    operator_pending: Option<OperatorPending>,
    yank_buffer: String,
    yank_type: YankType,
    find_cross_line: bool,
    visual_start: Option<(usize, usize)>,
    block_insert: Option<BlockInsert>,
    last_visual: Option<LastVisual>,
    pub(crate) undo_stack: Vec<EditorState>,
    pub(crate) redo_stack: Vec<EditorState>,
    is_restoring: bool,
    insert_undo_snapshot: bool,
    undo_limit: usize,
    line_undo: Option<LineUndo>,
    shift_width: usize,
    indent_colon: bool,
}

impl App {
    pub fn new(file_path: Option<PathBuf>, content: String) -> Self {
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
            status_message: String::new(),
            command_buffer: String::new(),
            quit_confirm: false,
            status_time: None,
            pending_g: false,
            pending_find: None,
            last_find: None,
            operator_pending: None,
            yank_buffer: String::new(),
            yank_type: YankType::Char,
            find_cross_line: true,
            visual_start: None,
            block_insert: None,
            last_visual: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            is_restoring: false,
            insert_undo_snapshot: false,
            undo_limit: 200,
            line_undo: None,
            shift_width: 4,
            indent_colon: false,
        }
    }

    pub fn clear_status_if_stale(&mut self) {
        if let Some(t) = self.status_time {
            if t.elapsed() > Duration::from_secs(5) {
                self.status_message.clear();
                self.status_time = None;
            }
        }
    }

    pub fn ensure_cursor_visible(&mut self, viewport_rows: usize, viewport_cols: usize) {
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

    pub fn visual_selection(&self) -> Option<VisualSelection> {
        if !matches!(self.mode, Mode::VisualChar | Mode::VisualLine | Mode::VisualBlock) {
            return None;
        }
        let start = self.visual_start?;
        let end = (self.cursor_row, self.cursor_col);
        match self.mode {
            Mode::VisualChar => Some(VisualSelection::Char(normalize_range(start, end))),
            Mode::VisualLine => {
                let (s, e) = normalize_range((start.0, 0), (end.0, 0));
                Some(VisualSelection::Line(s.0, e.0))
            }
            Mode::VisualBlock => {
                let (s, e) = normalize_range(start, end);
                Some(VisualSelection::Block { start: s, end: e })
            }
            _ => None,
        }
    }

    pub fn selection_summary(&self) -> Option<String> {
        let selection = self.visual_selection()?;
        let summary = match selection {
            VisualSelection::Char((start, end)) => {
                let count = char_count_in_range(self, start, end);
                format!("{} chars", count)
            }
            VisualSelection::Line(start, end) => format!("{} lines", end - start + 1),
            VisualSelection::Block { start, end } => {
                let rows = end.0 - start.0 + 1;
                let cols = end.1.saturating_sub(start.1) + 1;
                format!("{}x{}", rows, cols)
            }
        };
        Some(summary)
    }

    pub fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.record_undo();
        self.insert_undo_snapshot = true;
        self.clear_line_undo();
        for ch in text.chars() {
            if ch == '\n' {
                self.insert_newline_raw();
            } else {
                self.insert_char_raw(ch);
            }
        }
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = msg.into();
        self.status_time = Some(Instant::now());
    }

    fn snapshot(&self) -> EditorState {
        EditorState {
            lines: self.lines.clone(),
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
            scroll_row: self.scroll_row,
            scroll_col: self.scroll_col,
            dirty: self.dirty,
        }
    }

    fn restore(&mut self, state: EditorState) {
        self.is_restoring = true;
        self.lines = state.lines;
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor_row = state.cursor_row.min(self.lines.len().saturating_sub(1));
        let len = self.line_len(self.cursor_row);
        self.cursor_col = state.cursor_col.min(len);
        self.scroll_row = state.scroll_row;
        self.scroll_col = state.scroll_col;
        self.dirty = state.dirty;
        self.pending_g = false;
        self.pending_find = None;
        self.operator_pending = None;
        self.block_insert = None;
        self.visual_start = None;
        self.line_undo = None;
        self.is_restoring = false;
    }

    fn record_undo(&mut self) {
        if self.is_restoring {
            return;
        }
        if self.mode == Mode::Insert && self.insert_undo_snapshot {
            return;
        }
        self.undo_stack.push(self.snapshot());
        if self.undo_stack.len() > self.undo_limit {
            let overflow = self.undo_stack.len() - self.undo_limit;
            self.undo_stack.drain(0..overflow);
        }
        self.redo_stack.clear();
        if self.mode == Mode::Insert {
            self.insert_undo_snapshot = true;
        }
    }

    fn undo(&mut self) {
        if let Some(state) = self.undo_stack.pop() {
            let current = self.snapshot();
            self.redo_stack.push(current);
            self.restore(state);
            self.insert_undo_snapshot = false;
        }
    }

    fn redo(&mut self) {
        if let Some(state) = self.redo_stack.pop() {
            let current = self.snapshot();
            self.undo_stack.push(current);
            self.restore(state);
            self.insert_undo_snapshot = false;
        }
    }

    fn set_line_undo(&mut self, row: usize) {
        if row >= self.lines.len() {
            return;
        }
        match self.line_undo {
            Some(ref lu) if lu.row == row => {}
            _ => {
                self.line_undo = Some(LineUndo {
                    row,
                    line: self.lines[row].clone(),
                });
            }
        }
    }

    fn clear_line_undo(&mut self) {
        self.line_undo = None;
    }

    fn undo_line(&mut self) {
        let Some(lu) = self.line_undo.take() else {
            self.set_status("No line undo");
            return;
        };
        if lu.row >= self.lines.len() {
            return;
        }
        self.record_undo();
        self.lines[lu.row] = lu.line;
        self.cursor_row = lu.row;
        let len = self.line_len(self.cursor_row);
        self.cursor_col = self.cursor_col.min(len);
        self.dirty = true;
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
        let prev_row = self.cursor_row;
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.line_len(self.cursor_row);
        }
        if self.cursor_row != prev_row {
            self.clear_line_undo();
        }
    }

    fn move_right(&mut self) {
        let prev_row = self.cursor_row;
        let len = self.line_len(self.cursor_row);
        if self.cursor_col < len {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
        if self.cursor_row != prev_row {
            self.clear_line_undo();
        }
    }

    fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            let len = self.line_len(self.cursor_row);
            self.cursor_col = self.cursor_col.min(len);
            self.clear_line_undo();
        }
    }

    fn move_down(&mut self) {
        if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            let len = self.line_len(self.cursor_row);
            self.cursor_col = self.cursor_col.min(len);
            self.clear_line_undo();
        }
    }

    fn move_line_start(&mut self) {
        self.cursor_col = 0;
    }

    fn move_line_first_non_blank(&mut self) {
        let mut col = 0;
        if let Some(line) = self.lines.get(self.cursor_row) {
            for ch in line.chars() {
                if !ch.is_whitespace() {
                    break;
                }
                col += 1;
            }
        }
        self.cursor_col = col;
    }

    fn move_line_end(&mut self) {
        let len = self.line_len(self.cursor_row);
        self.cursor_col = if len == 0 { 0 } else { len - 1 };
    }

    fn move_line_end_insert(&mut self) {
        let len = self.line_len(self.cursor_row);
        self.cursor_col = len;
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

    fn move_big_word_forward(&mut self) {
        if let Some((row, col)) = self.next_big_word_start(self.cursor_row, self.cursor_col) {
            self.cursor_row = row;
            self.cursor_col = col;
        }
    }

    fn move_big_word_end(&mut self) {
        if let Some((row, col)) = self.next_big_word_end(self.cursor_row, self.cursor_col) {
            self.cursor_row = row;
            self.cursor_col = col;
        }
    }

    fn move_big_word_back(&mut self) {
        if let Some((row, col)) = self.prev_big_word_start(self.cursor_row, self.cursor_col) {
            self.cursor_row = row;
            self.cursor_col = col;
        }
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

    fn next_big_word_start(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        let cur = self.class_at(row, col)?;
        if cur == CharClass::Space {
            return self.skip_spaces_forward(row, col);
        }
        let after = self.advance_to_next_non_space_change(row, col)?;
        self.skip_spaces_forward(after.0, after.1)
    }

    fn next_big_word_end(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        let cur = self.class_at(row, col)?;
        if cur == CharClass::Space {
            let (sr, sc) = self.skip_spaces_forward(row, col)?;
            return Some(self.end_of_non_space_group(sr, sc));
        }
        let end = self.end_of_non_space_group(row, col);
        if end != (row, col) {
            return Some(end);
        }
        let next = self.advance_pos(row, col)?;
        let (sr, sc) = self.skip_spaces_forward(next.0, next.1)?;
        Some(self.end_of_non_space_group(sr, sc))
    }

    fn prev_big_word_start(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        let cur = self.class_at(row, col)?;
        if cur == CharClass::Space {
            let (r, c) = self.skip_spaces_backward(row, col)?;
            return Some(self.start_of_non_space_group(r, c));
        }

        if self.is_non_space_group_start(row, col) {
            let prev = self.prev_pos(row, col)?;
            let (r, c) = self.skip_spaces_backward(prev.0, prev.1)?;
            return Some(self.start_of_non_space_group(r, c));
        }

        Some(self.start_of_non_space_group(row, col))
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

    fn advance_to_next_non_space_change(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        let mut r = row;
        let mut c = col;
        loop {
            let next = self.advance_pos(r, c)?;
            match self.class_at(next.0, next.1) {
                Some(CharClass::Space) => return Some(next),
                Some(_) => {
                    r = next.0;
                    c = next.1;
                }
                None => return None,
            }
        }
    }

    fn start_of_non_space_group(&self, row: usize, col: usize) -> (usize, usize) {
        let mut r = row;
        let mut c = col;
        while let Some((pr, pc)) = self.prev_pos(r, c) {
            if self.class_at(pr, pc) != Some(CharClass::Space) {
                r = pr;
                c = pc;
            } else {
                break;
            }
        }
        (r, c)
    }

    fn end_of_non_space_group(&self, row: usize, col: usize) -> (usize, usize) {
        let mut r = row;
        let mut c = col;
        while let Some((nr, nc)) = self.advance_pos(r, c) {
            if self.class_at(nr, nc) != Some(CharClass::Space) {
                r = nr;
                c = nc;
            } else {
                break;
            }
        }
        (r, c)
    }

    fn is_non_space_group_start(&self, row: usize, col: usize) -> bool {
        match self.prev_pos(row, col) {
            Some((pr, pc)) => self.class_at(pr, pc) == Some(CharClass::Space),
            None => true,
        }
    }

    fn find_forward(&mut self, target: char, until: bool) -> bool {
        let prev_row = self.cursor_row;
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
                    if self.cursor_row != prev_row {
                        self.clear_line_undo();
                    }
                    return true;
                }
            }
            if !self.find_cross_line {
                break;
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
        let prev_row = self.cursor_row;
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
                if self.cursor_row != prev_row {
                    self.clear_line_undo();
                }
                return true;
            }
            if row == 0 || !self.find_cross_line {
                break;
            }
            row -= 1;
            col = self.line_len(row);
        }
        false
    }

    fn delete_range(&mut self, start: (usize, usize), end: (usize, usize)) {
        self.record_undo();
        let (start, end) = normalize_range(start, end);
        if start.0 == end.0 {
            self.set_line_undo(start.0);
            let row = start.0;
            let line = &mut self.lines[row];
            let len = line.chars().count();
            if len == 0 {
                return;
            }
            let start_idx = char_to_byte_idx(line, start.1);
            let end_col = end.1.min(len.saturating_sub(1));
            let end_idx = if end_col + 1 <= len {
                char_to_byte_idx(line, end_col + 1)
            } else {
                line.len()
            };
            line.replace_range(start_idx..end_idx, "");
        } else {
            self.clear_line_undo();
            let start_line = &self.lines[start.0];
            let start_prefix = &start_line[..char_to_byte_idx(start_line, start.1)];

            let end_line = &self.lines[end.0];
            let end_len = end_line.chars().count();
            let end_col = end.1.min(end_len.saturating_sub(1));
            let end_suffix = if end_len == 0 {
                ""
            } else {
                &end_line[char_to_byte_idx(end_line, end_col + 1)..]
            };

            let merged = format!("{}{}", start_prefix, end_suffix);
            self.lines[start.0] = merged;
            self.lines.drain(start.0 + 1..=end.0);
        }

        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor_row = start.0.min(self.lines.len() - 1);
        let len = self.line_len(self.cursor_row);
        self.cursor_col = start.1.min(len);
        self.dirty = true;
    }

    fn yank_range(&mut self, start: (usize, usize), end: (usize, usize)) {
        let (start, end) = normalize_range(start, end);
        let mut out = String::new();
        if start.0 == end.0 {
            let line = &self.lines[start.0];
            let len = line.chars().count();
            if len == 0 {
                self.yank_buffer.clear();
                self.yank_type = YankType::Char;
                return;
            }
            let start_idx = char_to_byte_idx(line, start.1);
            let end_col = end.1.min(len.saturating_sub(1));
            let end_idx = if end_col + 1 <= len {
                char_to_byte_idx(line, end_col + 1)
            } else {
                line.len()
            };
            out.push_str(&line[start_idx..end_idx]);
        } else {
            let start_line = &self.lines[start.0];
            out.push_str(&start_line[char_to_byte_idx(start_line, start.1)..]);
            out.push('\n');
            for row in (start.0 + 1)..end.0 {
                out.push_str(&self.lines[row]);
                out.push('\n');
            }
            let end_line = &self.lines[end.0];
            let end_len = end_line.chars().count();
            let end_col = end.1.min(end_len.saturating_sub(1));
            let end_idx = if end_len == 0 {
                0
            } else {
                char_to_byte_idx(end_line, end_col + 1)
            };
            out.push_str(&end_line[..end_idx]);
        }
        self.yank_buffer = out;
        self.yank_type = YankType::Char;
    }

    fn delete_lines(&mut self, start_row: usize, end_row: usize) {
        self.record_undo();
        self.clear_line_undo();
        if self.lines.is_empty() {
            return;
        }
        let start = start_row.min(self.lines.len() - 1);
        let end = end_row.min(self.lines.len() - 1);
        self.lines.drain(start..=end);
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor_row = start.min(self.lines.len() - 1);
        self.cursor_col = 0;
        self.dirty = true;
    }

    fn yank_lines(&mut self, start_row: usize, end_row: usize) {
        if self.lines.is_empty() {
            self.yank_buffer.clear();
            self.yank_type = YankType::Line;
            return;
        }
        let start = start_row.min(self.lines.len() - 1);
        let end = end_row.min(self.lines.len() - 1);
        let mut out = String::new();
        for row in start..=end {
            out.push_str(&self.lines[row]);
            if row != end {
                out.push('\n');
            }
        }
        self.yank_buffer = out;
        self.yank_type = YankType::Line;
    }

    fn delete_block(&mut self, start: (usize, usize), end: (usize, usize)) {
        self.record_undo();
        self.clear_line_undo();
        let (start, end) = normalize_range(start, end);
        for row in start.0..=end.0 {
            if row >= self.lines.len() {
                break;
            }
            let line = &mut self.lines[row];
            let len = line.chars().count();
            if len == 0 || start.1 >= len {
                continue;
            }
            let end_col = end.1.min(len.saturating_sub(1));
            let start_idx = char_to_byte_idx(line, start.1);
            let end_idx = if end_col + 1 <= len {
                char_to_byte_idx(line, end_col + 1)
            } else {
                line.len()
            };
            line.replace_range(start_idx..end_idx, "");
        }
        self.cursor_row = start.0.min(self.lines.len().saturating_sub(1));
        self.cursor_col = start.1.min(self.line_len(self.cursor_row));
        self.dirty = true;
    }

    fn yank_block(&mut self, start: (usize, usize), end: (usize, usize)) {
        let (start, end) = normalize_range(start, end);
        let mut out = String::new();
        for row in start.0..=end.0 {
            if row >= self.lines.len() {
                break;
            }
            let line = &self.lines[row];
            let len = line.chars().count();
            if len == 0 || start.1 >= len {
                if row != end.0 {
                    out.push('\n');
                }
                continue;
            }
            let end_col = end.1.min(len.saturating_sub(1));
            let start_idx = char_to_byte_idx(line, start.1);
            let end_idx = if end_col + 1 <= len {
                char_to_byte_idx(line, end_col + 1)
            } else {
                line.len()
            };
            out.push_str(&line[start_idx..end_idx]);
            if row != end.0 {
                out.push('\n');
            }
        }
        self.yank_buffer = out;
        self.yank_type = YankType::Block;
    }

    fn delete_line(&mut self, row: usize) {
        self.record_undo();
        self.clear_line_undo();
        if self.lines.is_empty() {
            return;
        }
        self.lines.remove(row.min(self.lines.len() - 1));
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor_row = row.min(self.lines.len() - 1);
        self.cursor_col = 0;
        self.dirty = true;
    }

    fn yank_line(&mut self, row: usize) {
        let row = row.min(self.lines.len().saturating_sub(1));
        self.yank_buffer = self.lines.get(row).cloned().unwrap_or_default();
        self.yank_type = YankType::Line;
    }

    fn paste_after(&mut self) {
        self.record_undo();
        if self.yank_buffer.is_empty() {
            return;
        }
        match self.yank_type {
            YankType::Line => {
                self.clear_line_undo();
                let insert_at = (self.cursor_row + 1).min(self.lines.len());
                let lines: Vec<String> =
                    self.yank_buffer.split('\n').map(|s| s.to_string()).collect();
                self.lines.splice(insert_at..insert_at, lines);
                self.cursor_row = insert_at;
                self.cursor_col = 0;
            }
            YankType::Block => {
                self.clear_line_undo();
                self.paste_block_at(self.cursor_row, self.cursor_col);
            }
            YankType::Char => {
                self.set_line_undo(self.cursor_row);
                let line = &mut self.lines[self.cursor_row];
                let byte_idx = char_to_byte_idx(line, self.cursor_col + 1);
                line.insert_str(byte_idx, &self.yank_buffer);
                self.cursor_col += self.yank_buffer.chars().count();
            }
        }
        self.dirty = true;
    }

    fn paste_before(&mut self) {
        self.record_undo();
        if self.yank_buffer.is_empty() {
            return;
        }
        match self.yank_type {
            YankType::Line => {
                self.clear_line_undo();
                let insert_at = self.cursor_row.min(self.lines.len());
                let lines: Vec<String> =
                    self.yank_buffer.split('\n').map(|s| s.to_string()).collect();
                self.lines.splice(insert_at..insert_at, lines);
                self.cursor_row = insert_at;
                self.cursor_col = 0;
            }
            YankType::Block => {
                self.clear_line_undo();
                self.paste_block_at(self.cursor_row, self.cursor_col);
            }
            YankType::Char => {
                self.set_line_undo(self.cursor_row);
                let line = &mut self.lines[self.cursor_row];
                let byte_idx = char_to_byte_idx(line, self.cursor_col);
                line.insert_str(byte_idx, &self.yank_buffer);
                self.cursor_col += self.yank_buffer.chars().count();
            }
        }
        self.dirty = true;
    }

    fn paste_block_at(&mut self, row: usize, col: usize) {
        let mut r = row;
        for line_text in self.yank_buffer.split('\n') {
            if r >= self.lines.len() {
                self.lines.push(String::new());
            }
            let line = &mut self.lines[r];
            let insert_col = col.min(line.chars().count());
            let byte_idx = char_to_byte_idx(line, insert_col);
            line.insert_str(byte_idx, line_text);
            r += 1;
        }
    }

    fn insert_char(&mut self, ch: char) {
        self.record_undo();
        if let Some(block) = &mut self.block_insert {
            for row in block.start_row..=block.end_row {
                if row >= self.lines.len() {
                    break;
                }
                let line = &mut self.lines[row];
                let col = if block.append {
                    line.chars().count()
                } else {
                    block.col
                };
                let len = line.chars().count();
                if col > len {
                    line.push_str(&" ".repeat(col - len));
                }
                let byte_idx = char_to_byte_idx(line, col);
                line.insert(byte_idx, ch);
            }
            block.col += 1;
            self.cursor_col = block.col;
            self.dirty = true;
            return;
        }
        self.set_line_undo(self.cursor_row);
        let line = &mut self.lines[self.cursor_row];
        let byte_idx = char_to_byte_idx(line, self.cursor_col);
        line.insert(byte_idx, ch);
        self.cursor_col += 1;
        self.dirty = true;
    }

    fn insert_char_raw(&mut self, ch: char) {
        if let Some(block) = &mut self.block_insert {
            for row in block.start_row..=block.end_row {
                if row >= self.lines.len() {
                    break;
                }
                let line = &mut self.lines[row];
                let col = if block.append {
                    line.chars().count()
                } else {
                    block.col
                };
                let len = line.chars().count();
                if col > len {
                    line.push_str(&" ".repeat(col - len));
                }
                let byte_idx = char_to_byte_idx(line, col);
                line.insert(byte_idx, ch);
            }
            block.col += 1;
            self.cursor_col = block.col;
            self.dirty = true;
            return;
        }
        let line = &mut self.lines[self.cursor_row];
        let byte_idx = char_to_byte_idx(line, self.cursor_col);
        line.insert(byte_idx, ch);
        self.cursor_col += 1;
        self.dirty = true;
    }

    fn insert_newline_raw(&mut self) {
        if self.block_insert.is_some() {
            self.block_insert_newline();
            return;
        }
        let line = &mut self.lines[self.cursor_row];
        let byte_idx = char_to_byte_idx(line, self.cursor_col);
        let right = line.split_off(byte_idx);
        self.lines.insert(self.cursor_row + 1, right);
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.dirty = true;
    }

    fn insert_newline(&mut self) {
        self.record_undo();
        if self.block_insert.is_some() {
            self.block_insert_newline();
            return;
        }
        self.clear_line_undo();
        let line = &mut self.lines[self.cursor_row];
        let byte_idx = char_to_byte_idx(line, self.cursor_col);
        let right = line.split_off(byte_idx);
        let mut indent = Self::leading_whitespace(line);
        if Self::should_increase_indent(line, self.indent_colon) {
            indent = Self::increase_indent(&indent, self.shift_width);
        } else if Self::should_decrease_indent(&right) {
            if let Some(target) = self.matching_indent_for_closer(self.cursor_row + 1, 0) {
                indent = " ".repeat(target);
            } else {
                indent = Self::decrease_indent(&indent, self.shift_width);
            }
        }
        let mut new_line = indent.clone();
        new_line.push_str(&right);
        self.lines.insert(self.cursor_row + 1, new_line);
        self.cursor_row += 1;
        self.cursor_col = indent.chars().count();
        self.dirty = true;
    }

    fn backspace(&mut self) {
        self.record_undo();
        if let Some(block) = &mut self.block_insert {
            if block.col == 0 {
                return;
            }
            let target_col = block.col - 1;
            for row in block.start_row..=block.end_row {
                if row >= self.lines.len() {
                    break;
                }
                let line = &mut self.lines[row];
                let len = line.chars().count();
                if target_col >= len {
                    continue;
                }
                let byte_idx = char_to_byte_idx(line, target_col);
                let next_idx = char_to_byte_idx(line, target_col + 1);
                line.replace_range(byte_idx..next_idx, "");
            }
            block.col -= 1;
            self.cursor_col = block.col;
            self.dirty = true;
            return;
        }
        if self.cursor_col > 0 {
            self.set_line_undo(self.cursor_row);
            let line = &mut self.lines[self.cursor_row];
            let byte_idx = char_to_byte_idx(line, self.cursor_col);
            let prev_idx = char_to_byte_idx(line, self.cursor_col - 1);
            line.replace_range(prev_idx..byte_idx, "");
            self.cursor_col -= 1;
            self.dirty = true;
        } else if self.cursor_row > 0 {
            self.clear_line_undo();
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
        self.record_undo();
        if self.block_insert.is_some() {
            return;
        }
        let len = self.line_len(self.cursor_row);
        if self.cursor_col < len {
            self.set_line_undo(self.cursor_row);
            let line = &mut self.lines[self.cursor_row];
            let byte_idx = char_to_byte_idx(line, self.cursor_col);
            let next_idx = char_to_byte_idx(line, self.cursor_col + 1);
            line.replace_range(byte_idx..next_idx, "");
            self.dirty = true;
        } else if self.cursor_row + 1 < self.lines.len() {
            self.clear_line_undo();
            let next = self.lines.remove(self.cursor_row + 1);
            let line = &mut self.lines[self.cursor_row];
            line.push_str(&next);
            self.dirty = true;
        }
    }

    fn open_line_below(&mut self) {
        self.record_undo();
        self.clear_line_undo();
        let line = &self.lines[self.cursor_row];
        let mut indent = Self::leading_whitespace(line);
        if Self::should_increase_indent(line, self.indent_colon) {
            indent = Self::increase_indent(&indent, self.shift_width);
        }
        self.lines.insert(self.cursor_row + 1, indent.clone());
        self.cursor_row += 1;
        self.cursor_col = indent.chars().count();
        self.dirty = true;
    }

    fn open_line_above(&mut self) {
        self.record_undo();
        self.clear_line_undo();
        let line = &self.lines[self.cursor_row];
        let mut indent = Self::leading_whitespace(line);
        if Self::should_decrease_indent(line) {
            if let Some(target) = self.matching_indent_for_closer(self.cursor_row, 0) {
                indent = " ".repeat(target);
            } else {
                indent = Self::decrease_indent(&indent, self.shift_width);
            }
        }
        self.lines.insert(self.cursor_row, indent.clone());
        self.cursor_col = indent.chars().count();
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
            "set" => {
                if let Some(setting) = arg {
                    if let Some(value) = setting.strip_prefix("shiftwidth=") {
                        if let Ok(width) = value.parse::<usize>() {
                            if width > 0 {
                                self.shift_width = width;
                                self.set_status(format!("shiftwidth={}", width));
                            } else {
                                self.set_status("shiftwidth must be > 0");
                            }
                        } else {
                            self.set_status("shiftwidth expects a number");
                        }
                        return Ok(false);
                    }
                    match setting {
                        "findcross" => {
                            self.find_cross_line = true;
                            self.set_status("findcross");
                        }
                        "nofindcross" => {
                            self.find_cross_line = false;
                            self.set_status("nofindcross");
                        }
                        "findcross?" => {
                            let value = if self.find_cross_line {
                                "findcross"
                            } else {
                                "nofindcross"
                            };
                            self.set_status(value);
                        }
                        "shiftwidth?" => {
                            self.set_status(format!("shiftwidth={}", self.shift_width));
                        }
                        "indentcolon" => {
                            self.indent_colon = true;
                            self.set_status("indentcolon");
                        }
                        "noindentcolon" => {
                            self.indent_colon = false;
                            self.set_status("noindentcolon");
                        }
                        "indentcolon?" => {
                            let value = if self.indent_colon {
                                "indentcolon"
                            } else {
                                "noindentcolon"
                            };
                            self.set_status(value);
                        }
                        _ => self.set_status("Unknown option"),
                    }
                } else {
                    self.set_status("Usage: :set findcross|nofindcross|shiftwidth=4|indentcolon");
                }
            }
            _ => {
                self.set_status(format!("Not an editor command: {}", cmd));
            }
        }

        Ok(false)
    }

    fn apply_operator(&mut self, op: Operator, start: (usize, usize), end: (usize, usize)) {
        match op {
            Operator::Delete => self.delete_range(start, end),
            Operator::Yank => self.yank_range(start, end),
            Operator::Change => self.delete_range(start, end),
        }
    }

    fn leading_whitespace(line: &str) -> String {
        line.chars().take_while(|c| c.is_whitespace()).collect()
    }

    fn should_increase_indent(line: &str, indent_colon: bool) -> bool {
        let trimmed = line.trim_end();
        trimmed.ends_with('{')
            || trimmed.ends_with('[')
            || trimmed.ends_with('(')
            || (indent_colon && trimmed.ends_with(':'))
    }

    fn should_decrease_indent(line: &str) -> bool {
        let trimmed = line.trim_start();
        trimmed.starts_with('}') || trimmed.starts_with(']') || trimmed.starts_with(')')
    }

    fn increase_indent(indent: &str, shift_width: usize) -> String {
        let mut out = indent.to_string();
        out.push_str(&" ".repeat(shift_width));
        out
    }

    fn decrease_indent(indent: &str, shift_width: usize) -> String {
        if indent.ends_with('\t') {
            return indent[..indent.len().saturating_sub(1)].to_string();
        }
        let mut trimmed = indent.to_string();
        let mut remove = 0;
        while remove < shift_width && trimmed.ends_with(' ') {
            trimmed.pop();
            remove += 1;
        }
        trimmed
    }

    fn matching_indent_for_closer(&self, row: usize, col: usize) -> Option<usize> {
        let ch = self.char_at(row, col)?;
        let (open, close) = match ch {
            '}' => ('{', '}'),
            ']' => ('[', ']'),
            ')' => ('(', ')'),
            _ => return None,
        };

        let mut depth = 0i32;
        let mut r = row;
        let mut c = col;
        loop {
            if let Some((pr, pc)) = self.prev_pos(r, c) {
                r = pr;
                c = pc;
            } else {
                break;
            }
            if let Some(ch2) = self.char_at(r, c) {
                if ch2 == close {
                    depth += 1;
                } else if ch2 == open {
                    if depth == 0 {
                        let indent = Self::leading_whitespace(&self.lines[r]).chars().count();
                        return Some(indent);
                    } else {
                        depth -= 1;
                    }
                }
            }
        }
        None
    }

    fn block_insert_newline(&mut self) {
        self.record_undo();
        let Some(block) = self.block_insert.take() else {
            return;
        };
        let mut row = block.end_row;
        loop {
            if row >= self.lines.len() {
                if row == 0 {
                    break;
                }
                row -= 1;
                continue;
            }
            let line = &mut self.lines[row];
            let col = block.col.min(line.chars().count());
            let byte_idx = char_to_byte_idx(line, col);
            let right = line.split_off(byte_idx);
            self.lines.insert(row + 1, right);
            if row == block.start_row {
                break;
            }
            row = row.saturating_sub(1);
        }
        self.cursor_row = block.start_row + 1;
        self.cursor_col = 0;
        self.dirty = true;
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> Result<bool> {
    let is_quit = matches!(key.code, KeyCode::Char('q')) && key.modifiers == KeyModifiers::CONTROL;
    if !is_quit {
        app.quit_confirm = false;
    }
    if app.pending_g
        && !matches!(key.code, KeyCode::Char('g') | KeyCode::Char('v'))
        && key.modifiers == KeyModifiers::NONE
    {
        app.pending_g = false;
    }
    if matches!(
        app.mode,
        Mode::Normal | Mode::VisualChar | Mode::VisualLine | Mode::VisualBlock
    ) {
        if let Some(pending) = app.pending_find.take() {
            if let KeyCode::Char(ch) = key.code {
                let found = if pending.reverse {
                    app.find_backward(ch, pending.until)
                } else {
                    app.find_forward(ch, pending.until)
                };
                if !found {
                    app.set_status(format!(
                        "Pattern not found: {}{}",
                        if pending.reverse { "F" } else { "f" },
                        ch
                    ));
                } else {
                    app.last_find = Some(FindSpec {
                        ch,
                        until: pending.until,
                        reverse: pending.reverse,
                    });
                }
            }
            if app.mode == Mode::Normal {
                if let Some(op) = app.operator_pending.take() {
                    app.apply_operator(
                        op.op,
                        (op.start_row, op.start_col),
                        (app.cursor_row, app.cursor_col),
                    );
                    if op.op == Operator::Change {
                        app.mode = Mode::Insert;
                        app.set_status("-- INSERT --");
                    }
                }
            }
            return Ok(false);
        }
    }

    match app.mode {
        Mode::Normal => match (key.code, key.modifiers) {
            (KeyCode::Char('r'), KeyModifiers::CONTROL) => {
                app.redo();
            }
            (KeyCode::Char('z'), KeyModifiers::CONTROL) => {
                app.undo();
            }
            (KeyCode::Char('u'), KeyModifiers::NONE) => {
                app.undo();
            }
            (KeyCode::Char('U'), _) => {
                app.undo_line();
            }
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
                app.operator_pending = None;
                app.insert_undo_snapshot = false;
                app.set_status("-- INSERT --");
            }
            (KeyCode::Char('a'), KeyModifiers::NONE) => {
                let len = app.line_len(app.cursor_row);
                if app.cursor_col < len {
                    app.cursor_col += 1;
                }
                app.mode = Mode::Insert;
                app.operator_pending = None;
                app.insert_undo_snapshot = false;
                app.set_status("-- INSERT --");
            }
            (KeyCode::Char('I'), _) => {
                app.move_line_first_non_blank();
                app.mode = Mode::Insert;
                app.operator_pending = None;
                app.insert_undo_snapshot = false;
                app.set_status("-- INSERT --");
            }
            (KeyCode::Char('A'), _) => {
                app.move_line_end_insert();
                app.mode = Mode::Insert;
                app.operator_pending = None;
                app.insert_undo_snapshot = false;
                app.set_status("-- INSERT --");
            }
            (KeyCode::Char('v'), KeyModifiers::NONE) => {
                if app.pending_g {
                    app.pending_g = false;
                    if let Some(last) = app.last_visual {
                        app.mode = last.mode;
                        app.visual_start = Some(last.start);
                        app.cursor_row = last.end.0;
                        app.cursor_col = last.end.1;
                        let label = match app.mode {
                            Mode::VisualChar => "-- VISUAL --",
                            Mode::VisualLine => "-- VISUAL LINE --",
                            Mode::VisualBlock => "-- VISUAL BLOCK --",
                            _ => "-- VISUAL --",
                        };
                        app.set_status(label);
                    } else {
                        app.set_status("No previous visual selection");
                    }
                } else {
                    app.mode = Mode::VisualChar;
                    app.visual_start = Some((app.cursor_row, app.cursor_col));
                    app.operator_pending = None;
                    app.set_status("-- VISUAL --");
                }
            }
            (KeyCode::Char('V'), _) => {
                app.mode = Mode::VisualLine;
                app.visual_start = Some((app.cursor_row, app.cursor_col));
                app.operator_pending = None;
                app.set_status("-- VISUAL LINE --");
            }
            (KeyCode::Char('v'), KeyModifiers::CONTROL) => {
                app.mode = Mode::VisualBlock;
                app.visual_start = Some((app.cursor_row, app.cursor_col));
                app.operator_pending = None;
                app.set_status("-- VISUAL BLOCK --");
            }
            (KeyCode::Char(':'), KeyModifiers::NONE) => {
                app.mode = Mode::Command;
                app.command_buffer.clear();
                app.operator_pending = None;
            }
            (KeyCode::Char('d'), KeyModifiers::NONE) => {
                if let Some(op) = app.operator_pending.take() {
                    if op.op == Operator::Delete {
                        app.delete_line(app.cursor_row);
                        return Ok(false);
                    }
                }
                app.operator_pending = Some(OperatorPending {
                    op: Operator::Delete,
                    start_row: app.cursor_row,
                    start_col: app.cursor_col,
                });
            }
            (KeyCode::Char('y'), KeyModifiers::NONE) => {
                if let Some(op) = app.operator_pending.take() {
                    if op.op == Operator::Yank {
                        app.yank_line(app.cursor_row);
                        return Ok(false);
                    }
                }
                app.operator_pending = Some(OperatorPending {
                    op: Operator::Yank,
                    start_row: app.cursor_row,
                    start_col: app.cursor_col,
                });
            }
            (KeyCode::Char('c'), KeyModifiers::NONE) => {
                if let Some(op) = app.operator_pending.take() {
                    if op.op == Operator::Change {
                        app.delete_line(app.cursor_row);
                        app.mode = Mode::Insert;
                        app.insert_undo_snapshot = false;
                        app.set_status("-- INSERT --");
                        return Ok(false);
                    }
                }
                app.operator_pending = Some(OperatorPending {
                    op: Operator::Change,
                    start_row: app.cursor_row,
                    start_col: app.cursor_col,
                });
            }
            (KeyCode::Char('p'), KeyModifiers::NONE) => {
                app.paste_after();
            }
            (KeyCode::Char('P'), _) => {
                app.paste_before();
            }
            (KeyCode::Char('h'), KeyModifiers::NONE) | (KeyCode::Left, _) => app.move_left(),
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, _) => app.move_down(),
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, _) => app.move_up(),
            (KeyCode::Char('l'), KeyModifiers::NONE) | (KeyCode::Right, _) => app.move_right(),
            (KeyCode::Char('w'), KeyModifiers::NONE) => {
                if let Some(op) = app.operator_pending.take() {
                    app.move_word_forward();
                    let mut end = (app.cursor_row, app.cursor_col);
                    if let Some(prev) = app.prev_pos(end.0, end.1) {
                        end = prev;
                    }
                    app.apply_operator(op.op, (op.start_row, op.start_col), end);
                    if op.op == Operator::Change {
                        app.mode = Mode::Insert;
                        app.insert_undo_snapshot = false;
                        app.set_status("-- INSERT --");
                    }
                } else {
                    app.move_word_forward();
                }
            }
            (KeyCode::Char('b'), KeyModifiers::NONE) => app.move_word_back(),
            (KeyCode::Char('e'), KeyModifiers::NONE) => app.move_word_end(),
            (KeyCode::Char('W'), _) => {
                if let Some(op) = app.operator_pending.take() {
                    app.move_big_word_forward();
                    let mut end = (app.cursor_row, app.cursor_col);
                    if let Some(prev) = app.prev_pos(end.0, end.1) {
                        end = prev;
                    }
                    app.apply_operator(op.op, (op.start_row, op.start_col), end);
                    if op.op == Operator::Change {
                        app.mode = Mode::Insert;
                        app.insert_undo_snapshot = false;
                        app.set_status("-- INSERT --");
                    }
                } else {
                    app.move_big_word_forward();
                }
            }
            (KeyCode::Char('B'), _) => app.move_big_word_back(),
            (KeyCode::Char('E'), _) => app.move_big_word_end(),
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
                        app.set_status(format!(
                            "Pattern not found: {}{}",
                            if spec.reverse { "F" } else { "f" },
                            spec.ch
                        ));
                    }
                } else {
                    app.set_status("No previous find");
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
                        app.set_status(format!(
                            "Pattern not found: {}{}",
                            if spec.reverse { "f" } else { "F" },
                            spec.ch
                        ));
                    }
                } else {
                    app.set_status("No previous find");
                }
            }
            (KeyCode::Char('x'), KeyModifiers::NONE) => app.delete_at_cursor(),
            (KeyCode::Char('o'), KeyModifiers::NONE) => {
                app.open_line_below();
                app.mode = Mode::Insert;
                app.insert_undo_snapshot = false;
            }
            (KeyCode::Char('O'), _) => {
                app.open_line_above();
                app.mode = Mode::Insert;
                app.insert_undo_snapshot = false;
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
            (KeyCode::Char('r'), KeyModifiers::CONTROL) => {
                app.redo();
            }
            (KeyCode::Char('z'), KeyModifiers::CONTROL) => {
                app.undo();
            }
            (KeyCode::Esc, _) => {
                app.mode = Mode::Normal;
                app.block_insert = None;
                app.insert_undo_snapshot = false;
                app.set_status("-- NORMAL --");
            }
            (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                app.save()?;
            }
            (KeyCode::Enter, _) => {
                app.insert_undo_snapshot = false;
                app.insert_newline()
            }
            (KeyCode::Backspace, _) => {
                app.insert_undo_snapshot = false;
                app.backspace()
            }
            (KeyCode::Delete, _) => {
                app.insert_undo_snapshot = false;
                app.delete_at_cursor()
            }
            (KeyCode::Tab, _) => {
                app.insert_undo_snapshot = false;
                for _ in 0..4 {
                    app.insert_char(' ');
                }
            }
            (KeyCode::Char(ch), KeyModifiers::NONE) => {
                if is_undo_break_char(ch) {
                    app.insert_undo_snapshot = false;
                }
                app.insert_char(ch)
            }
            (KeyCode::Char(ch), KeyModifiers::SHIFT) => {
                if is_undo_break_char(ch) {
                    app.insert_undo_snapshot = false;
                }
                app.insert_char(ch)
            }
            (KeyCode::Left, _) => {
                app.insert_undo_snapshot = false;
                app.move_left()
            }
            (KeyCode::Right, _) => {
                app.insert_undo_snapshot = false;
                app.move_right()
            }
            (KeyCode::Up, _) => {
                app.insert_undo_snapshot = false;
                app.move_up()
            }
            (KeyCode::Down, _) => {
                app.insert_undo_snapshot = false;
                app.move_down()
            }
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
        Mode::VisualChar | Mode::VisualLine | Mode::VisualBlock => match (key.code, key.modifiers) {
            (KeyCode::Esc, _)
            | (KeyCode::Char('v'), KeyModifiers::NONE)
            | (KeyCode::Char('V'), _) => {
                if let Some(selection) = app.visual_selection() {
                    app.last_visual = Some(selection_to_last_visual(selection, app.mode));
                }
                app.mode = Mode::Normal;
                app.visual_start = None;
                app.set_status("-- NORMAL --");
            }
            (KeyCode::Char('y'), KeyModifiers::NONE) => {
                if let Some(selection) = app.visual_selection() {
                    match selection {
                        VisualSelection::Char((start, end)) => app.yank_range(start, end),
                        VisualSelection::Line(start, end) => app.yank_lines(start, end),
                        VisualSelection::Block { start, end } => app.yank_block(start, end),
                    }
                    app.last_visual = Some(selection_to_last_visual(selection, app.mode));
                }
                app.mode = Mode::Normal;
                app.visual_start = None;
            }
            (KeyCode::Char('d'), KeyModifiers::NONE) => {
                if let Some(selection) = app.visual_selection() {
                    match selection {
                        VisualSelection::Char((start, end)) => app.delete_range(start, end),
                        VisualSelection::Line(start, end) => app.delete_lines(start, end),
                        VisualSelection::Block { start, end } => app.delete_block(start, end),
                    }
                    app.last_visual = Some(selection_to_last_visual(selection, app.mode));
                }
                app.mode = Mode::Normal;
                app.visual_start = None;
            }
            (KeyCode::Char('c'), KeyModifiers::NONE) => {
                if let Some(selection) = app.visual_selection() {
                    match selection {
                        VisualSelection::Char((start, end)) => app.delete_range(start, end),
                        VisualSelection::Line(start, end) => app.delete_lines(start, end),
                        VisualSelection::Block { start, end } => app.delete_block(start, end),
                    }
                    app.last_visual = Some(selection_to_last_visual(selection, app.mode));
                }
                app.mode = Mode::Insert;
                app.insert_undo_snapshot = false;
                app.visual_start = None;
                app.set_status("-- INSERT --");
            }
            (KeyCode::Char('p'), KeyModifiers::NONE) | (KeyCode::Char('P'), _) => {
                let selection = app.visual_selection();
                let start = if let Some(selection) = selection {
                    match selection {
                        VisualSelection::Char((start, end)) => {
                            app.delete_range(start, end);
                            start
                        }
                        VisualSelection::Line(start, end) => {
                            app.delete_lines(start, end);
                            (start, 0)
                        }
                        VisualSelection::Block { start, end } => {
                            app.delete_block(start, end);
                            start
                        }
                    }
                } else {
                    (app.cursor_row, app.cursor_col)
                };
                app.cursor_row = start.0;
                app.cursor_col = start.1;
                app.paste_before();
                if let Some(selection) = selection {
                    app.last_visual = Some(selection_to_last_visual(selection, app.mode));
                }
                app.mode = Mode::Normal;
                app.visual_start = None;
            }
            (KeyCode::Char('I'), _) => {
                if matches!(app.mode, Mode::VisualBlock) {
                    if let Some(VisualSelection::Block { start, end }) = app.visual_selection() {
                        app.block_insert = Some(BlockInsert {
                            start_row: start.0,
                            end_row: end.0,
                            col: start.1,
                            append: false,
                        });
                        app.cursor_row = start.0;
                        app.cursor_col = start.1;
                        app.mode = Mode::Insert;
                        app.insert_undo_snapshot = false;
                        app.visual_start = None;
                        app.set_status("-- INSERT --");
                    }
                }
            }
            (KeyCode::Char('A'), _) => {
                if matches!(app.mode, Mode::VisualBlock) {
                    if let Some(VisualSelection::Block { start, end }) = app.visual_selection() {
                        app.block_insert = Some(BlockInsert {
                            start_row: start.0,
                            end_row: end.0,
                            col: end.1 + 1,
                            append: false,
                        });
                        app.cursor_row = start.0;
                        app.cursor_col = end.1 + 1;
                        app.mode = Mode::Insert;
                        app.insert_undo_snapshot = false;
                        app.visual_start = None;
                        app.set_status("-- INSERT --");
                    }
                }
            }
            (KeyCode::Char('h'), KeyModifiers::NONE) | (KeyCode::Left, _) => app.move_left(),
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, _) => app.move_down(),
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, _) => app.move_up(),
            (KeyCode::Char('l'), KeyModifiers::NONE) | (KeyCode::Right, _) => app.move_right(),
            (KeyCode::Char('w'), KeyModifiers::NONE) => app.move_word_forward(),
            (KeyCode::Char('b'), KeyModifiers::NONE) => app.move_word_back(),
            (KeyCode::Char('e'), KeyModifiers::NONE) => app.move_word_end(),
            (KeyCode::Char('W'), _) => app.move_big_word_forward(),
            (KeyCode::Char('B'), _) => app.move_big_word_back(),
            (KeyCode::Char('E'), _) => app.move_big_word_end(),
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
                        app.set_status(format!(
                            "Pattern not found: {}{}",
                            if spec.reverse { "F" } else { "f" },
                            spec.ch
                        ));
                    }
                } else {
                    app.set_status("No previous find");
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
                        app.set_status(format!(
                            "Pattern not found: {}{}",
                            if spec.reverse { "f" } else { "F" },
                            spec.ch
                        ));
                    }
                } else {
                    app.set_status("No previous find");
                }
            }
            _ => {}
        },
    }

    if app.mode == Mode::Normal {
        if let Some(op) = app.operator_pending.take() {
            let end = (app.cursor_row, app.cursor_col);
            if (op.start_row, op.start_col) != end {
                app.apply_operator(op.op, (op.start_row, op.start_col), end);
                if op.op == Operator::Change {
                    app.mode = Mode::Insert;
                    app.insert_undo_snapshot = false;
                    app.set_status("-- INSERT --");
                }
            } else {
                app.operator_pending = Some(op);
            }
        }
    }

    Ok(false)
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

fn is_undo_break_char(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | '(' | ')' | '[' | ']' | '{' | '}')
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

#[derive(Debug, Clone, Copy)]
pub(crate) enum VisualSelection {
    Char(((usize, usize), (usize, usize))),
    Line(usize, usize),
    Block { start: (usize, usize), end: (usize, usize) },
}

#[derive(Debug, Clone)]
pub struct EditorState {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
    scroll_row: usize,
    scroll_col: usize,
    dirty: bool,
}

#[derive(Debug, Clone, Copy)]
struct BlockInsert {
    start_row: usize,
    end_row: usize,
    col: usize,
    append: bool,
}

#[derive(Debug, Clone)]
struct LineUndo {
    row: usize,
    line: String,
}

#[derive(Debug, Clone, Copy)]
struct LastVisual {
    mode: Mode,
    start: (usize, usize),
    end: (usize, usize),
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

fn normalize_range(a: (usize, usize), b: (usize, usize)) -> ((usize, usize), (usize, usize)) {
    if pos_le(a, b) {
        (a, b)
    } else {
        (b, a)
    }
}

fn pos_le(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.0 || (a.0 == b.0 && a.1 <= b.1)
}

fn char_count_in_range(app: &App, start: (usize, usize), end: (usize, usize)) -> usize {
    let (start, end) = normalize_range(start, end);
    if start.0 == end.0 {
        return end.1.saturating_sub(start.1) + 1;
    }
    let mut count = 0;
    let start_len = app.line_len(start.0);
    count += start_len.saturating_sub(start.1);
    for row in (start.0 + 1)..end.0 {
        count += app.line_len(row);
    }
    count += end.1 + 1;
    count
}

fn selection_to_last_visual(selection: VisualSelection, mode: Mode) -> LastVisual {
    match selection {
        VisualSelection::Char((start, end)) => LastVisual { mode, start, end },
        VisualSelection::Line(start, end) => LastVisual {
            mode,
            start: (start, 0),
            end: (end, 0),
        },
        VisualSelection::Block { start, end } => LastVisual { mode, start, end },
    }
}
