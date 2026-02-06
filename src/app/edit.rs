use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use super::motion::char_count_in_range;
use crossterm::event::{KeyCode, KeyModifiers};

use super::types::{
    char_class, char_to_byte_idx, char_to_screen_col, normalize_range, screen_col_to_char_idx,
    CharClass, CommandPrompt, EditorState, LastVisual, LineUndo, Mode, Operator,
    RepeatKey, VisualSelection, VisualSelectionKind, YankType, char_display_width,
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
            relative_number: false,
            pending_count: None,
            theme: super::theme::Theme::default_theme(),
            theme_name: "light".to_string(),
            theme_overrides: None,
            yank_buffer: String::new(),
            yank_type: YankType::Char,
            visual_start: None,
            block_insert: None,
            last_visual: None,
            insert_undo_snapshot: false,
            pending_find: None,
            pending_g: false,
            pending_bracket: None,
            operator_pending: None,
            last_find: None,
            pending_textobj: None,
            quit_confirm: false,
            status_time: None,
            undo_limit: 200,
            line_undo: None,
            is_restoring: false,
            command_prompt: CommandPrompt::Command,
            command_history: Vec::new(),
            command_history_index: None,
            command_candidates: default_command_candidates(),
            command_cursor: 0,
            keymaps: super::keymap::Keymaps::default(),
            keymap_debug: false,
            last_search: None,
            search_history: Vec::new(),
            search_history_index: None,
            repeat_recording: false,
            repeat_replaying: false,
            repeat_changed: false,
            repeat_buffer: Vec::new(),
            last_change: Vec::new(),
            change_tick: 0,
            buffers: Vec::new(),
            current_buffer_id: 1,
            next_buffer_id: 2,
            completion_candidates: Vec::new(),
            completion_index: None,
            completion_cmd_prefix: None,
            completion_anchor_fixed: false,
            completion_anchor_col: None,
            edit_tick: 0,
            syntax_by_buffer: HashMap::new(),
        }
    }

    pub fn buffer_count(&self) -> usize {
        1 + self.buffers.len()
    }

    pub fn buffer_display_name(path: &Option<std::path::PathBuf>) -> String {
        path.as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "[No Name]".to_string())
    }

    pub fn list_buffers(&self) -> String {
        let mut entries: Vec<(usize, bool, bool, String)> = Vec::with_capacity(self.buffers.len() + 1);
        entries.push((
            self.current_buffer_id,
            true,
            self.dirty,
            Self::buffer_display_name(&self.file_path),
        ));
        for slot in &self.buffers {
            entries.push((
                slot.id,
                false,
                slot.state.dirty,
                Self::buffer_display_name(&slot.state.file_path),
            ));
        }
        entries.sort_by_key(|(id, _, _, _)| *id);
        let parts: Vec<String> = entries
            .into_iter()
            .map(|(id, current, dirty, name)| {
                let mut tag = String::new();
                if current {
                    tag.push('%');
                }
                if dirty {
                    tag.push('+');
                }
                if tag.is_empty() {
                    format!("{} {}", id, name)
                } else {
                    format!("{}{} {}", id, tag, name)
                }
            })
            .collect();
        parts.join(" | ")
    }

    pub fn capture_buffer_state(&self) -> super::types::BufferState {
        super::types::BufferState {
            lines: self.lines.clone(),
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
            scroll_row: self.scroll_row,
            scroll_col: self.scroll_col,
            file_path: self.file_path.clone(),
            dirty: self.dirty,
            undo_stack: self.undo_stack.clone(),
            redo_stack: self.redo_stack.clone(),
            line_undo: self.line_undo.clone(),
            is_restoring: self.is_restoring,
            change_tick: self.change_tick,
            edit_tick: self.edit_tick,
        }
    }

    pub fn load_buffer_state(&mut self, state: super::types::BufferState) {
        self.lines = state.lines;
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor_row = state.cursor_row.min(self.lines.len().saturating_sub(1));
        let current_line_len = self
            .lines
            .get(self.cursor_row)
            .map(|line| line.chars().count())
            .unwrap_or(0);
        self.cursor_col = state.cursor_col.min(current_line_len);
        self.scroll_row = state
            .scroll_row
            .min(self.lines.len().saturating_sub(1));
        self.scroll_col = state.scroll_col;
        self.file_path = state.file_path;
        self.dirty = state.dirty;
        self.undo_stack = state.undo_stack;
        self.redo_stack = state.redo_stack;
        self.line_undo = state.line_undo;
        self.is_restoring = state.is_restoring;
        self.change_tick = state.change_tick;
        self.edit_tick = state.edit_tick;
    }

    pub fn reset_transient_for_switch(&mut self) {
        self.mode = Mode::Normal;
        self.command_prompt = CommandPrompt::Command;
        self.command_buffer.clear();
        self.command_cursor = 0;
        self.search_history_index = None;
        self.pending_count = None;
        self.visual_start = None;
        self.block_insert = None;
        self.last_visual = None;
        self.insert_undo_snapshot = false;
        self.pending_find = None;
        self.pending_g = false;
        self.pending_bracket = None;
        self.operator_pending = None;
        self.last_find = None;
        self.pending_textobj = None;
        self.repeat_recording = false;
        self.repeat_replaying = false;
        self.repeat_changed = false;
        self.repeat_buffer.clear();
        self.clear_completion();
    }

    pub(crate) fn touch_edit(&mut self) {
        self.edit_tick = self.edit_tick.wrapping_add(1);
    }

    pub fn clear_completion(&mut self) {
        self.completion_candidates.clear();
        self.completion_index = None;
        self.completion_cmd_prefix = None;
        self.completion_anchor_fixed = false;
        self.completion_anchor_col = None;
    }

    pub fn insert_command_text(&mut self, text: &str) {
        if !matches!(self.command_prompt, CommandPrompt::Command | CommandPrompt::SearchForward | CommandPrompt::SearchBackward) {
            return;
        }
        if text.is_empty() {
            return;
        }
        let sanitized = text.replace(['\r', '\n', '\t'], " ");
        if sanitized.is_empty() {
            return;
        }
        let idx = self.command_cursor.min(self.command_buffer.chars().count());
        let byte_idx = char_to_byte_idx(&self.command_buffer, idx);
        self.command_buffer.insert_str(byte_idx, &sanitized);
        self.command_cursor = idx + sanitized.chars().count();
    }

    pub(crate) fn log_key_event(&self, label: &str) {
        if !self.keymap_debug {
            return;
        }
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let mut path = PathBuf::from(home);
        path.push(".config/rvim");
        if fs::create_dir_all(&path).is_err() {
            return;
        }
        path.push("rvim.log");
        let _ = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "keymap {}", label)
            });
    }

    #[allow(dead_code)]
    pub fn register_command_candidate(&mut self, name: impl Into<String>) {
        let name = name.into();
        if !self.command_candidates.iter().any(|c| c == &name) {
            self.command_candidates.push(name);
            self.command_candidates.sort();
        }
    }

    pub fn apply_config(&mut self, config: &super::config::Config) {
        self.theme_overrides = config.themes.clone();
        if let Some(name) = config.theme.as_deref() {
            if let Some(theme) = super::theme::Theme::from_name(name) {
                self.set_theme_named(name, theme);
            }
        }
        self.keymap_debug = config.keymap_debug.unwrap_or(false);
        if self.keymap_debug {
            self.set_status("Keymap debug: on");
        }
        let (keymaps, errors) = super::keymap::Keymaps::from_config(config.keymap.as_ref());
        self.keymaps = keymaps;
        if let Some(err) = errors.first() {
            self.set_status(format!("Keymap error: {}", err));
        }
    }

    #[allow(dead_code)]
    pub fn set_theme(&mut self, theme: super::theme::Theme) {
        self.theme = theme;
    }

    pub fn set_theme_named(&mut self, name: &str, theme: super::theme::Theme) {
        self.theme = theme;
        self.theme_name = name.to_ascii_lowercase();
        if let Some(overrides) = self
            .theme_overrides
            .as_ref()
            .and_then(|m| m.get(&self.theme_name))
        {
            super::config::apply_theme_overrides(&mut self.theme, overrides);
        }
    }

    #[allow(dead_code)]
    pub fn theme_mut(&mut self) -> &mut super::theme::Theme {
        &mut self.theme
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

        let line = self.lines.get(self.cursor_row).map(|s| s.as_str()).unwrap_or("");
        let cursor_screen = char_to_screen_col(line, self.cursor_col, self.shift_width);
        let scroll_screen = char_to_screen_col(line, self.scroll_col, self.shift_width);
        if cursor_screen < scroll_screen {
            self.scroll_col = screen_col_to_char_idx(line, cursor_screen, self.shift_width);
        } else if cursor_screen >= scroll_screen + viewport_cols {
            let target = cursor_screen.saturating_sub(viewport_cols - 1);
            self.scroll_col = screen_col_to_char_idx(line, target, self.shift_width);
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
                let base_line = self.lines.get(start.0).map(|s| s.as_str()).unwrap_or("");
                let start_sc = char_to_screen_col(base_line, start.1, self.shift_width);
                let end_sc = char_to_screen_col(base_line, end.1, self.shift_width);
                let end_char = base_line.chars().nth(end.1).unwrap_or(' ');
                let end_w = char_display_width(end_char, end_sc, self.shift_width);
                let (a, b) = if start_sc <= end_sc {
                    (start_sc, end_sc.saturating_add(end_w))
                } else {
                    (end_sc, start_sc.saturating_add(end_w))
                };
                let cols = b.saturating_sub(a).max(1);
                format!("{}x{}", rows, cols)
            }
        };
        Some(summary)
    }

    pub fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.repeat_recording && !self.repeat_replaying {
            for ch in text.chars() {
                let code = if ch == '\n' {
                    KeyCode::Enter
                } else {
                    KeyCode::Char(ch)
                };
                self.repeat_buffer.push(RepeatKey {
                    code,
                    modifiers: KeyModifiers::NONE,
                });
            }
        }
        self.record_undo();
        self.touch_edit();
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

    pub(super) fn textobj_word_range(&self) -> Option<((usize, usize), (usize, usize))> {
        let line = self.lines.get(self.cursor_row)?;
        let chars: Vec<char> = line.chars().collect();
        if self.cursor_col >= chars.len() {
            return None;
        }
        if char_class(chars[self.cursor_col]) != CharClass::Word {
            return None;
        }
        let mut start = self.cursor_col;
        while start > 0 && char_class(chars[start - 1]) == CharClass::Word {
            start -= 1;
        }
        let mut end = self.cursor_col;
        while end + 1 < chars.len() && char_class(chars[end + 1]) == CharClass::Word {
            end += 1;
        }
        Some(((self.cursor_row, start), (self.cursor_row, end)))
    }

    pub(super) fn textobj_word_range_around(&self) -> Option<((usize, usize), (usize, usize))> {
        let ((row, start), (row2, end)) = self.textobj_word_range()?;
        if row != row2 {
            return Some(((row, start), (row2, end)));
        }
        let line = self.lines.get(row)?;
        let chars: Vec<char> = line.chars().collect();
        let len = chars.len();
        let mut new_start = start;
        let mut new_end = end;

        // Vim-like: prefer trailing whitespace, otherwise leading whitespace.
        if end + 1 < len && chars[end + 1].is_whitespace() {
            new_end = end + 1;
            return Some(((row, new_start), (row, new_end)));
        }
        if start > 0 && chars[start - 1].is_whitespace() {
            new_start = start - 1;
        }
        Some(((row, new_start), (row, new_end)))
    }

    pub(super) fn textobj_pair_range(
        &self,
        open: char,
        close: char,
        kind: super::types::TextObjectKind,
    ) -> Option<((usize, usize), (usize, usize))> {
        let (l_row, l_col) = self.find_enclosing_pair_left(open, close)?;
        let (r_row, r_col) = self.find_matching_close_from(open, close, l_row, l_col)?;
        match kind {
            super::types::TextObjectKind::Inner => {
                let start = if let Some(next) = self.advance_pos(l_row, l_col) {
                    next
                } else {
                    return None;
                };
                let end = if let Some(prev) = self.prev_pos(r_row, r_col) {
                    prev
                } else {
                    return None;
                };
                Some((start, end))
            }
            super::types::TextObjectKind::Around => Some(((l_row, l_col), (r_row, r_col))),
        }
    }

    pub(super) fn textobj_quote_range(
        &self,
        quote: char,
        kind: super::types::TextObjectKind,
    ) -> Option<((usize, usize), (usize, usize))> {
        let (l_row, l_col) = self.find_enclosing_quote_left(quote)?;
        let (r_row, r_col) = self.find_matching_quote_from(quote, l_row, l_col)?;
        match kind {
            super::types::TextObjectKind::Inner => {
                let start = if let Some(next) = self.advance_pos(l_row, l_col) {
                    next
                } else {
                    return None;
                };
                let end = if let Some(prev) = self.prev_pos(r_row, r_col) {
                    prev
                } else {
                    return None;
                };
                Some((start, end))
            }
            super::types::TextObjectKind::Around => Some(((l_row, l_col), (r_row, r_col))),
        }
    }

    pub(super) fn textobj_tag_range(
        &self,
        kind: super::types::TextObjectKind,
    ) -> Option<((usize, usize), (usize, usize))> {
        let tag = self.find_enclosing_tag()?;
        match kind {
            super::types::TextObjectKind::Inner => {
                let start = self.advance_pos(tag.open_end.0, tag.open_end.1)?;
                let end = self.prev_pos(tag.close_start.0, tag.close_start.1)?;
                Some((start, end))
            }
            super::types::TextObjectKind::Around => Some((tag.open_start, tag.close_end)),
        }
    }

    fn find_enclosing_pair_left(&self, open: char, close: char) -> Option<(usize, usize)> {
        let mut depth = 0i32;
        let mut r = self.cursor_row;
        let mut c = self.cursor_col;
        loop {
            if let Some(ch) = self.char_at(r, c) {
                if ch == close {
                    depth += 1;
                } else if ch == open {
                    if depth == 0 {
                        return Some((r, c));
                    }
                    depth -= 1;
                }
            }
            if let Some((pr, pc)) = self.prev_pos(r, c) {
                r = pr;
                c = pc;
            } else {
                break;
            }
        }
        None
    }

    fn find_matching_close_from(
        &self,
        open: char,
        close: char,
        row: usize,
        col: usize,
    ) -> Option<(usize, usize)> {
        let mut depth = 0i32;
        let mut r = row;
        let mut c = col;
        loop {
            if let Some((nr, nc)) = self.advance_pos(r, c) {
                r = nr;
                c = nc;
            } else {
                break;
            }
            if let Some(ch) = self.char_at(r, c) {
                if ch == open {
                    depth += 1;
                } else if ch == close {
                    if depth == 0 {
                        return Some((r, c));
                    }
                    depth -= 1;
                }
            }
        }
        None
    }

    fn find_enclosing_tag(&self) -> Option<TagMatch> {
        let mut r = self.cursor_row;
        let mut c = self.cursor_col;
        loop {
            if let Some(ch) = self.char_at(r, c) {
                if ch == '<' {
                    if let Some(open) = self.parse_tag_at(r, c) {
                        if open.is_closing || open.is_self_closing {
                            // skip closing/self-closing
                        } else {
                            if let Some(close) =
                                self.find_matching_tag_close_from(&open.name, open.end)
                            {
                                let cursor = (self.cursor_row, self.cursor_col);
                                if pos_le(open.start, cursor) && pos_le(cursor, close.start) {
                                    return Some(TagMatch {
                                        open_start: open.start,
                                        open_end: open.end,
                                        close_start: close.start,
                                        close_end: close.end,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            if let Some((pr, pc)) = self.prev_pos(r, c) {
                r = pr;
                c = pc;
            } else {
                break;
            }
        }
        None
    }

    fn find_matching_tag_close_from(&self, name: &str, start: (usize, usize)) -> Option<TagClose> {
        let mut depth = 0i32;
        let mut r = start.0;
        let mut c = start.1;
        loop {
            if let Some((nr, nc)) = self.advance_pos(r, c) {
                r = nr;
                c = nc;
            } else {
                break;
            }
            if let Some(ch) = self.char_at(r, c) {
                if ch == '<' {
                    if let Some(tag) = self.parse_tag_at(r, c) {
                        if tag.name != name {
                            continue;
                        }
                        if tag.is_self_closing {
                            continue;
                        }
                        if tag.is_closing {
                            if depth == 0 {
                                return Some(TagClose {
                                    start: tag.start,
                                    end: tag.end,
                                });
                            }
                            depth -= 1;
                        } else {
                            depth += 1;
                        }
                    }
                }
            }
        }
        None
    }

    fn parse_tag_at(&self, row: usize, col: usize) -> Option<TagOpen> {
        if self.char_at(row, col)? != '<' {
            return None;
        }
        let (end_row, end_col) = self.find_tag_end(row, col)?;
        let inner = self.collect_range_to_string((row, col + 1), (end_row, end_col))?;
        let inner_trim = inner.trim();
        if inner_trim.is_empty() || inner_trim.starts_with('!') || inner_trim.starts_with('?') {
            return None;
        }
        let mut s = inner_trim;
        let mut is_closing = false;
        if let Some(rest) = s.strip_prefix('/') {
            is_closing = true;
            s = rest.trim_start();
        }
        let is_self_closing = s.ends_with('/');
        let s = s.trim_end_matches('/');
        let name: String = s
            .chars()
            .take_while(|ch| ch.is_alphanumeric() || *ch == '_' || *ch == '-' || *ch == ':')
            .collect();
        if name.is_empty() {
            return None;
        }
        Some(TagOpen {
            name,
            start: (row, col),
            end: (end_row, end_col),
            is_closing,
            is_self_closing,
        })
    }

    fn find_tag_end(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        let mut r = row;
        let mut c = col;
        let mut in_quote: Option<char> = None;
        loop {
            if let Some(ch) = self.char_at(r, c) {
                if let Some(q) = in_quote {
                    if ch == q && !self.is_escaped_at(r, c) {
                        in_quote = None;
                    }
                } else if ch == '"' || ch == '\'' {
                    in_quote = Some(ch);
                } else if ch == '>' {
                    return Some((r, c));
                }
            }
            if let Some((nr, nc)) = self.advance_pos(r, c) {
                r = nr;
                c = nc;
            } else {
                break;
            }
        }
        None
    }

    fn collect_range_to_string(
        &self,
        start: (usize, usize),
        end: (usize, usize),
    ) -> Option<String> {
        let (start, end) = super::types::normalize_range(start, end);
        if start.0 == end.0 {
            let line = self.lines.get(start.0)?;
            let chars: Vec<char> = line.chars().collect();
            if start.1 >= chars.len() || end.1 > chars.len() {
                return None;
            }
            return Some(chars[start.1..end.1].iter().collect());
        }
        let mut out = String::new();
        let first = self.lines.get(start.0)?;
        let first_chars: Vec<char> = first.chars().collect();
        if start.1 < first_chars.len() {
            out.extend(first_chars[start.1..].iter());
        }
        out.push('\n');
        for row in (start.0 + 1)..end.0 {
            out.push_str(self.lines.get(row)?);
            out.push('\n');
        }
        let last = self.lines.get(end.0)?;
        let last_chars: Vec<char> = last.chars().collect();
        let end_idx = end.1.min(last_chars.len());
        out.extend(last_chars[..end_idx].iter());
        Some(out)
    }

    fn find_enclosing_quote_left(&self, quote: char) -> Option<(usize, usize)> {
        let mut r = self.cursor_row;
        let mut c = self.cursor_col;
        loop {
            if let Some(ch) = self.char_at(r, c) {
                if ch == quote && !self.is_escaped_at(r, c) {
                    return Some((r, c));
                }
            }
            if let Some((pr, pc)) = self.prev_pos(r, c) {
                r = pr;
                c = pc;
            } else {
                break;
            }
        }
        None
    }

    fn find_matching_quote_from(
        &self,
        quote: char,
        row: usize,
        col: usize,
    ) -> Option<(usize, usize)> {
        let mut r = row;
        let mut c = col;
        loop {
            if let Some((nr, nc)) = self.advance_pos(r, c) {
                r = nr;
                c = nc;
            } else {
                break;
            }
            if let Some(ch) = self.char_at(r, c) {
                if ch == quote && !self.is_escaped_at(r, c) {
                    return Some((r, c));
                }
            }
        }
        None
    }

    fn is_escaped_at(&self, row: usize, col: usize) -> bool {
        let line = match self.lines.get(row) {
            Some(l) => l,
            None => return false,
        };
        let chars: Vec<char> = line.chars().collect();
        if col >= chars.len() {
            return false;
        }
        is_escaped(&chars, col)
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
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
        self.change_tick = self.change_tick.wrapping_add(1);
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
            self.touch_edit();
        }
    }

    pub(super) fn redo(&mut self) {
        if let Some(state) = self.redo_stack.pop() {
            let current = self.snapshot();
            self.undo_stack.push(current);
            self.restore(state);
            self.insert_undo_snapshot = false;
            self.touch_edit();
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
        self.touch_edit();
        self.lines[lu.row] = lu.line;
        self.cursor_row = lu.row;
        let len = self.line_len(self.cursor_row);
        self.cursor_col = self.cursor_col.min(len);
        self.dirty = true;
    }

    pub(super) fn insert_char(&mut self, ch: char) {
        self.record_undo();
        self.touch_edit();
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
        self.touch_edit();
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
        self.touch_edit();
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
        self.touch_edit();
        if self.block_insert.is_some() {
            return;
        }
        let len = self.line_len(self.cursor_row);
        if self.cursor_col < len {
            self.yank_range((self.cursor_row, self.cursor_col), (self.cursor_row, self.cursor_col));
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
        self.touch_edit();
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
        self.touch_edit();
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
        self.touch_edit();
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
        self.touch_edit();
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
        self.touch_edit();
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
        self.touch_edit();
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
            Operator::Delete => {
                self.yank_range(start, end);
                self.delete_range(start, end);
            }
            Operator::Yank => self.yank_range(start, end),
            Operator::Change => {
                self.yank_range(start, end);
                self.delete_range(start, end);
            }
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
        self.touch_edit();
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
        self.touch_edit();
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
        self.touch_edit();
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

    pub(super) fn change_case_range(
        &mut self,
        start: (usize, usize),
        end: (usize, usize),
        to_upper: bool,
    ) {
        self.record_undo();
        self.touch_edit();
        self.clear_line_undo();
        let (start, end) = normalize_range(start, end);
        if start.0 == end.0 {
            if let Some(line) = self.lines.get(start.0).cloned() {
                self.lines[start.0] = change_case_in_line(&line, start.1, end.1, to_upper);
            }
        } else {
            for row in start.0..=end.0 {
                if row >= self.lines.len() {
                    break;
                }
                let line = self.lines[row].clone();
                let len = line.chars().count();
                if len == 0 {
                    continue;
                }
                let (s, e) = if row == start.0 {
                    (start.1, len.saturating_sub(1))
                } else if row == end.0 {
                    (0, end.1.min(len.saturating_sub(1)))
                } else {
                    (0, len.saturating_sub(1))
                };
                self.lines[row] = change_case_in_line(&line, s, e, to_upper);
            }
        }
        self.dirty = true;
    }

    pub(super) fn change_case_lines(&mut self, start_row: usize, end_row: usize, to_upper: bool) {
        self.record_undo();
        self.touch_edit();
        self.clear_line_undo();
        if self.lines.is_empty() {
            return;
        }
        let start = start_row.min(self.lines.len() - 1);
        let end = end_row.min(self.lines.len() - 1);
        for row in start..=end {
            let line = self.lines[row].clone();
            let len = line.chars().count();
            if len == 0 {
                continue;
            }
            self.lines[row] = change_case_in_line(&line, 0, len.saturating_sub(1), to_upper);
        }
        self.dirty = true;
    }

    pub(super) fn change_case_block(
        &mut self,
        start: (usize, usize),
        end: (usize, usize),
        to_upper: bool,
    ) {
        self.record_undo();
        self.touch_edit();
        self.clear_line_undo();
        let (start, end) = normalize_range(start, end);
        for row in start.0..=end.0 {
            if row >= self.lines.len() {
                break;
            }
            let line = self.lines[row].clone();
            let len = line.chars().count();
            if len == 0 || start.1 >= len {
                continue;
            }
            let end_col = end.1.min(len.saturating_sub(1));
            self.lines[row] = change_case_in_line(&line, start.1, end_col, to_upper);
        }
        self.dirty = true;
    }

    pub(super) fn toggle_case_range(&mut self, start: (usize, usize), end: (usize, usize)) {
        self.record_undo();
        self.touch_edit();
        self.clear_line_undo();
        let (start, end) = normalize_range(start, end);
        if start.0 == end.0 {
            if let Some(line) = self.lines.get(start.0).cloned() {
                self.lines[start.0] = toggle_case_in_line(&line, start.1, end.1);
            }
        } else {
            for row in start.0..=end.0 {
                if row >= self.lines.len() {
                    break;
                }
                let line = self.lines[row].clone();
                let len = line.chars().count();
                if len == 0 {
                    continue;
                }
                let (s, e) = if row == start.0 {
                    (start.1, len.saturating_sub(1))
                } else if row == end.0 {
                    (0, end.1.min(len.saturating_sub(1)))
                } else {
                    (0, len.saturating_sub(1))
                };
                self.lines[row] = toggle_case_in_line(&line, s, e);
            }
        }
        self.dirty = true;
    }

    pub(super) fn toggle_case_lines(&mut self, start_row: usize, end_row: usize) {
        self.record_undo();
        self.touch_edit();
        self.clear_line_undo();
        if self.lines.is_empty() {
            return;
        }
        let start = start_row.min(self.lines.len() - 1);
        let end = end_row.min(self.lines.len() - 1);
        for row in start..=end {
            let line = self.lines[row].clone();
            let len = line.chars().count();
            if len == 0 {
                continue;
            }
            self.lines[row] = toggle_case_in_line(&line, 0, len.saturating_sub(1));
        }
        self.dirty = true;
    }

    pub(super) fn toggle_case_block(&mut self, start: (usize, usize), end: (usize, usize)) {
        self.record_undo();
        self.touch_edit();
        self.clear_line_undo();
        let (start, end) = normalize_range(start, end);
        for row in start.0..=end.0 {
            if row >= self.lines.len() {
                break;
            }
            let line = self.lines[row].clone();
            let len = line.chars().count();
            if len == 0 || start.1 >= len {
                continue;
            }
            let end_col = end.1.min(len.saturating_sub(1));
            self.lines[row] = toggle_case_in_line(&line, start.1, end_col);
        }
        self.dirty = true;
    }

    #[allow(dead_code)]
    pub(super) fn delete_block_range(&mut self, start: (usize, usize), end: (usize, usize)) {
        self.touch_edit();
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

    #[allow(dead_code)]
    pub(super) fn delete_range_no_undo(&mut self, start: (usize, usize), end: (usize, usize)) {
        self.touch_edit();
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

    #[allow(dead_code)]
    pub(super) fn delete_lines_no_undo(&mut self, start_row: usize, end_row: usize) {
        self.touch_edit();
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

fn default_command_candidates() -> Vec<String> {
    vec![
        "w",
        "write",
        "q",
        "quit",
        "q!",
        "quit!",
        "wq",
        "x",
        "e",
        "edit",
        "set",
        "ls",
        "buffers",
        "b",
        "buffer",
        "bn",
        "bnext",
        "bp",
        "bprev",
        "bd",
        "bdelete",
        "bd!",
        "bdelete!",
    ]
    .into_iter()
    .map(|s| s.to_string())
    .collect()
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

fn is_escaped(chars: &[char], idx: usize) -> bool {
    if idx == 0 {
        return false;
    }
    let mut backslashes = 0usize;
    let mut i = idx;
    while i > 0 {
        i -= 1;
        if chars[i] == '\\' {
            backslashes += 1;
        } else {
            break;
        }
    }
    backslashes % 2 == 1
}

fn change_case_in_line(line: &str, start_col: usize, end_col: usize, to_upper: bool) -> String {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return line.to_string();
    }
    let end_col = end_col.min(chars.len().saturating_sub(1));
    let mut out = String::new();
    for (i, ch) in chars.iter().enumerate() {
        if i >= start_col && i <= end_col {
            if to_upper {
                out.extend(ch.to_uppercase());
            } else {
                out.extend(ch.to_lowercase());
            }
        } else {
            out.push(*ch);
        }
    }
    out
}

fn toggle_case_in_line(line: &str, start_col: usize, end_col: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return line.to_string();
    }
    let end_col = end_col.min(chars.len().saturating_sub(1));
    let mut out = String::new();
    for (i, ch) in chars.iter().enumerate() {
        if i >= start_col && i <= end_col {
            if ch.is_lowercase() {
                out.extend(ch.to_uppercase());
            } else if ch.is_uppercase() {
                out.extend(ch.to_lowercase());
            } else {
                out.push(*ch);
            }
        } else {
            out.push(*ch);
        }
    }
    out
}

#[derive(Debug, Clone)]
struct TagOpen {
    name: String,
    start: (usize, usize),
    end: (usize, usize),
    is_closing: bool,
    is_self_closing: bool,
}

#[derive(Debug, Clone)]
struct TagClose {
    start: (usize, usize),
    end: (usize, usize),
}

#[derive(Debug, Clone)]
struct TagMatch {
    open_start: (usize, usize),
    open_end: (usize, usize),
    close_start: (usize, usize),
    close_end: (usize, usize),
}

fn pos_le(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.0 || (a.0 == b.0 && a.1 <= b.1)
}
