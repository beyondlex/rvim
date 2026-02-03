use anyhow::Result;

use super::motion::char_count_in_range;
use super::types::{
    char_class, char_to_byte_idx, normalize_range, CharClass, EditorState, LastVisual, LineUndo,
    Mode, Operator, OperatorPending, VisualSelection, VisualSelectionKind, YankType,
};
use super::App;

impl App {
    pub fn new(file_path: Option<std::path::PathBuf>, content: String) -> Self {
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
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            find_cross_line: true,
            shift_width: 4,
            indent_colon: false,
            yank_buffer: String::new(),
            yank_type: YankType::Char,
            visual_start: None,
            block_insert: None,
            last_visual: None,
            insert_undo_snapshot: false,
            pending_find: None,
            pending_g: false,
            operator_pending: None,
            last_find: None,
            quit_confirm: false,
            status_time: None,
            undo_limit: 200,
            line_undo: None,
            is_restoring: false,
        }
    }

    pub fn clear_status_if_stale(&mut self) {
        if let Some(t) = self.status_time {
            if t.elapsed() > std::time::Duration::from_secs(5) {
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
        let kind = match self.mode {
            Mode::VisualChar => {
                let (s, e) = normalize_range(start, end);
                VisualSelectionKind::Char(s, e)
            }
            Mode::VisualLine => {
                let (s, e) = normalize_range((start.0, 0), (end.0, 0));
                VisualSelectionKind::Line(s.0, e.0)
            }
            Mode::VisualBlock => {
                let (s, e) = normalize_range(start, end);
                VisualSelectionKind::Block { start: s, end: e }
            }
            _ => return None,
        };
        Some(VisualSelection { kind })
    }

    pub fn selection_summary(&self) -> Option<String> {
        let selection = self.visual_selection()?;
        let summary = match selection.kind {
            VisualSelectionKind::Char(start, end) => {
                let count = char_count_in_range(self, start, end);
                format!("{} chars", count)
            }
            VisualSelectionKind::Line(start, end) => format!("{} lines", end - start + 1),
            VisualSelectionKind::Block { start, end } => {
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

    pub(super) fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = msg.into();
        self.status_time = Some(std::time::Instant::now());
    }

    pub(super) fn snapshot(&self) -> EditorState {
        EditorState {
            lines: self.lines.clone(),
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
            scroll_row: self.scroll_row,
            scroll_col: self.scroll_col,
            dirty: self.dirty,
        }
    }

    pub(super) fn restore(&mut self, state: EditorState) {
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

    pub(super) fn record_undo(&mut self) {
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

    pub(super) fn undo(&mut self) {
        if let Some(state) = self.undo_stack.pop() {
            let current = self.snapshot();
            self.redo_stack.push(current);
            self.restore(state);
            self.insert_undo_snapshot = false;
        }
    }

    pub(super) fn redo(&mut self) {
        if let Some(state) = self.redo_stack.pop() {
            let current = self.snapshot();
            self.undo_stack.push(current);
            self.restore(state);
            self.insert_undo_snapshot = false;
        }
    }

    pub(super) fn set_line_undo(&mut self, row: usize) {
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

    pub(super) fn clear_line_undo(&mut self) {
        self.line_undo = None;
    }

    pub(super) fn undo_line(&mut self) {
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

    pub(super) fn insert_char(&mut self, ch: char) {
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

    pub(super) fn insert_char_raw(&mut self, ch: char) {
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

    pub(super) fn insert_newline_raw(&mut self) {
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

    pub(super) fn insert_newline(&mut self) {
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

    pub(super) fn backspace(&mut self) {
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

    pub(super) fn delete_at_cursor(&mut self) {
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

    pub(super) fn open_line_below(&mut self) {
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

    pub(super) fn open_line_above(&mut self) {
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

    pub(super) fn delete_range(&mut self, start: (usize, usize), end: (usize, usize)) {
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

    pub(super) fn delete_lines(&mut self, start_row: usize, end_row: usize) {
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

    pub(super) fn delete_block(&mut self, start: (usize, usize), end: (usize, usize)) {
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

    pub(super) fn yank_range(&mut self, start: (usize, usize), end: (usize, usize)) {
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

    pub(super) fn yank_lines(&mut self, start_row: usize, end_row: usize) {
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

    pub(super) fn yank_block(&mut self, start: (usize, usize), end: (usize, usize)) {
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

    pub(super) fn delete_line(&mut self, row: usize) {
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

    pub(super) fn yank_line(&mut self, row: usize) {
        let row = row.min(self.lines.len().saturating_sub(1));
        self.yank_buffer = self.lines.get(row).cloned().unwrap_or_default();
        self.yank_type = YankType::Line;
    }

    pub(super) fn apply_operator(&mut self, op: Operator, start: (usize, usize), end: (usize, usize)) {
        match op {
            Operator::Delete => self.delete_range(start, end),
            Operator::Yank => self.yank_range(start, end),
            Operator::Change => self.delete_range(start, end),
        }
    }

    pub fn undo_len(&self) -> usize {
        self.undo_stack.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo_stack.len()
    }

    pub(super) fn paste_after(&mut self) {
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

    pub(super) fn paste_before(&mut self) {
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

    pub(super) fn paste_block_at(&mut self, row: usize, col: usize) {
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

    pub(super) fn delete_block_range(&mut self, start: (usize, usize), end: (usize, usize)) {
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
    }

    pub(super) fn delete_range_no_undo(&mut self, start: (usize, usize), end: (usize, usize)) {
        let (start, end) = normalize_range(start, end);
        if start.0 == end.0 {
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
    }

    pub(super) fn delete_lines_no_undo(&mut self, start_row: usize, end_row: usize) {
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
    }

    pub(super) fn block_insert_newline(&mut self) {
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

    pub(super) fn line_len(&self, row: usize) -> usize {
        self.lines
            .get(row)
            .map(|l| l.chars().count())
            .unwrap_or(0)
    }
}

impl App {
    pub(super) fn leading_whitespace(line: &str) -> String {
        line.chars().take_while(|c| c.is_whitespace()).collect()
    }

    pub(super) fn should_increase_indent(line: &str, indent_colon: bool) -> bool {
        let trimmed = line.trim_end();
        trimmed.ends_with('{')
            || trimmed.ends_with('[')
            || trimmed.ends_with('(')
            || (indent_colon && trimmed.ends_with(':'))
    }

    pub(super) fn should_decrease_indent(line: &str) -> bool {
        let trimmed = line.trim_start();
        trimmed.starts_with('}') || trimmed.starts_with(']') || trimmed.starts_with(')')
    }

    pub(super) fn increase_indent(indent: &str, shift_width: usize) -> String {
        let mut out = indent.to_string();
        out.push_str(&" ".repeat(shift_width));
        out
    }

    pub(super) fn decrease_indent(indent: &str, shift_width: usize) -> String {
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

    pub(super) fn matching_indent_for_closer(&self, row: usize, col: usize) -> Option<usize> {
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
}

pub(super) fn selection_to_last_visual(selection: VisualSelection, mode: Mode) -> LastVisual {
    match selection.kind {
        VisualSelectionKind::Char(start, end) => LastVisual { mode, start, end },
        VisualSelectionKind::Line(start, end) => LastVisual {
            mode,
            start: (start, 0),
            end: (end, 0),
        },
        VisualSelectionKind::Block { start, end } => LastVisual { mode, start, end },
    }
}
