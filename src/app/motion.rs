use super::types::{char_class, CharClass, VisualSelection, VisualSelectionKind};
use super::App;

impl App {
    pub(super) fn char_at(&self, row: usize, col: usize) -> Option<char> {
        self.lines.get(row).and_then(|l| l.chars().nth(col))
    }

    pub(super) fn class_at(&self, row: usize, col: usize) -> Option<CharClass> {
        let len = self.line_len(row);
        if col == len {
            return Some(CharClass::Space);
        }
        if col > len {
            return None;
        }
        self.char_at(row, col).map(char_class)
    }

    pub(super) fn advance_pos(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        let len = self.line_len(row);
        if col < len {
            Some((row, col + 1))
        } else if row + 1 < self.lines.len() {
            Some((row + 1, 0))
        } else {
            None
        }
    }

    pub(super) fn prev_pos(&self, row: usize, col: usize) -> Option<(usize, usize)> {
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

    pub(super) fn move_left(&mut self) {
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

    pub(super) fn move_right(&mut self) {
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

    pub(super) fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            let len = self.line_len(self.cursor_row);
            self.cursor_col = self.cursor_col.min(len);
            self.clear_line_undo();
        }
    }

    pub(super) fn move_down(&mut self) {
        if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            let len = self.line_len(self.cursor_row);
            self.cursor_col = self.cursor_col.min(len);
            self.clear_line_undo();
        }
    }

    pub(super) fn move_line_start(&mut self) {
        self.cursor_col = 0;
    }

    pub(super) fn move_line_first_non_blank(&mut self) {
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

    pub(super) fn move_line_end(&mut self) {
        let len = self.line_len(self.cursor_row);
        self.cursor_col = if len == 0 { 0 } else { len - 1 };
    }

    pub(super) fn move_line_end_insert(&mut self) {
        let len = self.line_len(self.cursor_row);
        self.cursor_col = len;
    }

    pub(super) fn move_to_top(&mut self) {
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    pub(super) fn move_to_bottom(&mut self) {
        if self.lines.is_empty() {
            self.cursor_row = 0;
            self.cursor_col = 0;
            return;
        }
        self.cursor_row = self.lines.len() - 1;
        self.cursor_col = 0;
    }

    pub(super) fn move_to_line(&mut self, line: usize) {
        if self.lines.is_empty() {
            self.cursor_row = 0;
            self.cursor_col = 0;
            return;
        }
        let target = line.saturating_sub(1).min(self.lines.len() - 1);
        self.cursor_row = target;
        self.cursor_col = 0;
    }

    pub(super) fn move_word_forward(&mut self) {
        if let Some((row, col)) = self.next_word_start(self.cursor_row, self.cursor_col) {
            self.cursor_row = row;
            self.cursor_col = col;
        }
    }

    pub(super) fn move_word_end(&mut self) {
        if let Some((row, col)) = self.next_word_end(self.cursor_row, self.cursor_col) {
            self.cursor_row = row;
            self.cursor_col = col;
        }
    }

    pub(super) fn move_word_back(&mut self) {
        if let Some((row, col)) = self.prev_word_start(self.cursor_row, self.cursor_col) {
            self.cursor_row = row;
            self.cursor_col = col;
        }
    }

    pub(super) fn move_big_word_forward(&mut self) {
        if let Some((row, col)) = self.next_big_word_start(self.cursor_row, self.cursor_col) {
            self.cursor_row = row;
            self.cursor_col = col;
        }
    }

    pub(super) fn move_big_word_end(&mut self) {
        if let Some((row, col)) = self.next_big_word_end(self.cursor_row, self.cursor_col) {
            self.cursor_row = row;
            self.cursor_col = col;
        }
    }

    pub(super) fn move_big_word_back(&mut self) {
        if let Some((row, col)) = self.prev_big_word_start(self.cursor_row, self.cursor_col) {
            self.cursor_row = row;
            self.cursor_col = col;
        }
    }

    pub(super) fn next_word_start(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        let cur = self.class_at(row, col)?;
        if cur == CharClass::Space {
            return self.skip_spaces_forward(row, col);
        }
        let after = self.advance_to_next_class(row, col, cur)?;
        self.skip_spaces_forward(after.0, after.1)
    }

    pub(super) fn next_word_end(&self, row: usize, col: usize) -> Option<(usize, usize)> {
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

    pub(super) fn prev_word_start(&self, row: usize, col: usize) -> Option<(usize, usize)> {
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

    pub(super) fn next_big_word_start(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        let cur = self.class_at(row, col)?;
        if cur == CharClass::Space {
            return self.skip_spaces_forward(row, col);
        }
        let after = self.advance_to_next_non_space_change(row, col)?;
        self.skip_spaces_forward(after.0, after.1)
    }

    pub(super) fn next_big_word_end(&self, row: usize, col: usize) -> Option<(usize, usize)> {
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

    pub(super) fn prev_big_word_start(&self, row: usize, col: usize) -> Option<(usize, usize)> {
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

    pub(super) fn find_forward(&mut self, target: char, until: bool) -> bool {
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

    pub(super) fn find_backward(&mut self, target: char, until: bool) -> bool {
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

    pub(super) fn search_forward(&mut self, pattern: &str) -> bool {
        if pattern.is_empty() || self.lines.is_empty() {
            return false;
        }
        let prev_row = self.cursor_row;
        let needle: Vec<char> = pattern.chars().collect();
        let mut row = self.cursor_row;
        let mut col = self.cursor_col + 1;
        while row < self.lines.len() {
            let line = &self.lines[row];
            if let Some(idx) = find_in_line(line, &needle, col) {
                self.cursor_row = row;
                self.cursor_col = idx;
                if self.cursor_row != prev_row {
                    self.clear_line_undo();
                }
                return true;
            }
            if !self.find_cross_line {
                break;
            }
            row += 1;
            col = 0;
        }
        false
    }

    pub(super) fn search_backward(&mut self, pattern: &str) -> bool {
        if pattern.is_empty() || self.lines.is_empty() {
            return false;
        }
        let prev_row = self.cursor_row;
        let needle: Vec<char> = pattern.chars().collect();
        let mut row = self.cursor_row;
        let mut col = self.cursor_col.saturating_sub(1);
        loop {
            let line = &self.lines[row];
            if let Some(idx) = find_in_line_rev(line, &needle, col) {
                self.cursor_row = row;
                self.cursor_col = idx;
                if self.cursor_row != prev_row {
                    self.clear_line_undo();
                }
                return true;
            }
            if row == 0 || !self.find_cross_line {
                break;
            }
            row -= 1;
            let len = self.line_len(row);
            col = len.saturating_sub(1);
        }
        false
    }

    pub(super) fn percent_jump(&mut self) -> bool {
        let prev_row = self.cursor_row;
        let (open, close, forward) = if let Some(ch) = self.char_at(self.cursor_row, self.cursor_col)
        {
            match ch {
                '(' => ('(', ')', true),
                '[' => ('[', ']', true),
                '{' => ('{', '}', true),
                '<' => ('<', '>', true),
                ')' => ('(', ')', false),
                ']' => ('[', ']', false),
                '}' => ('{', '}', false),
                '>' => ('<', '>', false),
                _ => {
                    if let Some((r, c, o, cl, fwd)) = self.find_next_bracket() {
                        self.cursor_row = r;
                        self.cursor_col = c;
                        (o, cl, fwd)
                    } else {
                        return false;
                    }
                }
            }
        } else if let Some((r, c, o, cl, fwd)) = self.find_next_bracket() {
            self.cursor_row = r;
            self.cursor_col = c;
            (o, cl, fwd)
        } else {
            return false;
        };
        let target = if forward {
            self.find_match_forward(open, close)
        } else {
            self.find_match_backward(open, close)
        };
        if let Some((row, col)) = target {
            self.cursor_row = row;
            self.cursor_col = col;
            if self.cursor_row != prev_row {
                self.clear_line_undo();
            }
            return true;
        }
        false
    }

    fn find_match_forward(&self, open: char, close: char) -> Option<(usize, usize)> {
        let mut depth = 0i32;
        let mut r = self.cursor_row;
        let mut c = self.cursor_col;
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

    fn find_match_backward(&self, open: char, close: char) -> Option<(usize, usize)> {
        let mut depth = 0i32;
        let mut r = self.cursor_row;
        let mut c = self.cursor_col;
        loop {
            if let Some((pr, pc)) = self.prev_pos(r, c) {
                r = pr;
                c = pc;
            } else {
                break;
            }
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
        }
        None
    }

    fn find_next_bracket(&self) -> Option<(usize, usize, char, char, bool)> {
        let mut r = self.cursor_row;
        let mut c = self.cursor_col;
        loop {
            if let Some((nr, nc)) = self.advance_pos(r, c) {
                r = nr;
                c = nc;
            } else {
                break;
            }
            if let Some(ch) = self.char_at(r, c) {
                match ch {
                    '(' => return Some((r, c, '(', ')', true)),
                    '[' => return Some((r, c, '[', ']', true)),
                    '{' => return Some((r, c, '{', '}', true)),
                    '<' => return Some((r, c, '<', '>', true)),
                    ')' => return Some((r, c, '(', ')', false)),
                    ']' => return Some((r, c, '[', ']', false)),
                    '}' => return Some((r, c, '{', '}', false)),
                    '>' => return Some((r, c, '<', '>', false)),
                    _ => {}
                }
            }
        }
        None
    }
}

fn find_in_line(line: &str, needle: &[char], start_col: usize) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let chars: Vec<char> = line.chars().collect();
    if needle.len() > chars.len() || start_col >= chars.len() {
        return None;
    }
    let max_start = chars.len().saturating_sub(needle.len());
    for i in start_col..=max_start {
        if chars[i..i + needle.len()] == *needle {
            return Some(i);
        }
    }
    None
}

fn find_in_line_rev(line: &str, needle: &[char], end_col: usize) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() || needle.len() > chars.len() {
        return None;
    }
    let max_start = chars.len().saturating_sub(needle.len());
    let mut i = end_col.min(max_start);
    loop {
        if chars[i..i + needle.len()] == *needle {
            return Some(i);
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    None
}

pub(super) fn char_count_in_range(app: &App, start: (usize, usize), end: (usize, usize)) -> usize {
    let (start, end) = super::types::normalize_range(start, end);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_does_not_panic_when_pattern_longer_than_line() {
        let mut app = App::new(None, "a\nbb\nccc".to_string());
        app.cursor_row = 0;
        app.cursor_col = 0;
        assert!(!app.search_forward("abcdef"));
    }
}
