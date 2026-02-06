use std::path::PathBuf;
use std::time::Instant;
use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyModifiers};

use super::theme::Theme;
use super::highlight::SyntaxState;
use super::keymap::Keymaps;

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
pub enum CommandPrompt {
    Command,
    SearchForward,
    SearchBackward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Operator {
    Delete,
    Yank,
    Change,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum YankType {
    Char,
    Line,
    Block,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct OperatorPending {
    pub(super) op: Operator,
    pub(super) start_row: usize,
    pub(super) start_col: usize,
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
    pub(crate) undo_stack: Vec<EditorState>,
    pub(crate) redo_stack: Vec<EditorState>,
    pub(crate) find_cross_line: bool,
    pub(crate) shift_width: usize,
    pub(crate) indent_colon: bool,
    pub(crate) relative_number: bool,
    pub(crate) pending_count: Option<usize>,
    pub(crate) theme: Theme,
    pub(crate) theme_name: String,
    pub(crate) theme_overrides: Option<HashMap<String, super::config::ThemeOverride>>,
    pub(crate) yank_buffer: String,
    pub(crate) yank_type: YankType,
    pub(crate) visual_start: Option<(usize, usize)>,
    pub(crate) block_insert: Option<BlockInsert>,
    pub(crate) last_visual: Option<LastVisual>,
    pub(crate) insert_undo_snapshot: bool,
    pub(crate) pending_find: Option<FindPending>,
    pub(crate) pending_g: bool,
    pub(crate) pending_bracket: Option<char>,
    pub(crate) operator_pending: Option<OperatorPending>,
    pub(crate) last_find: Option<FindSpec>,
    pub(crate) pending_textobj: Option<TextObjectPending>,
    pub(crate) quit_confirm: bool,
    pub(crate) status_time: Option<Instant>,
    pub(crate) undo_limit: usize,
    pub(crate) line_undo: Option<LineUndo>,
    pub(crate) is_restoring: bool,
    pub(crate) command_prompt: CommandPrompt,
    pub(crate) command_history: Vec<String>,
    pub(crate) command_history_index: Option<usize>,
    pub(crate) command_candidates: Vec<String>,
    pub(crate) command_cursor: usize,
    pub(crate) keymaps: Keymaps,
    pub(crate) keymap_debug: bool,
    pub(crate) last_search: Option<SearchSpec>,
    pub(crate) search_history: Vec<String>,
    pub(crate) search_history_index: Option<usize>,
    pub(crate) repeat_recording: bool,
    pub(crate) repeat_replaying: bool,
    pub(crate) repeat_changed: bool,
    pub(crate) repeat_buffer: Vec<RepeatKey>,
    pub(crate) last_change: Vec<RepeatKey>,
    pub(crate) change_tick: u64,
    pub(crate) buffers: Vec<BufferSlot>,
    pub(crate) current_buffer_id: usize,
    pub(crate) next_buffer_id: usize,
    pub(crate) completion_candidates: Vec<String>,
    pub(crate) completion_index: Option<usize>,
    pub(crate) completion_cmd_prefix: Option<String>,
    pub(crate) completion_anchor_fixed: bool,
    pub(crate) completion_anchor_col: Option<u16>,
    pub(crate) edit_tick: u64,
    pub(crate) syntax_by_buffer: HashMap<usize, SyntaxState>,
}

#[derive(Debug, Clone, Copy)]
pub struct VisualSelection {
    pub(crate) kind: VisualSelectionKind,
}

#[derive(Debug, Clone, Copy)]
pub enum VisualSelectionKind {
    Char((usize, usize), (usize, usize)),
    Line(usize, usize),
    Block { start: (usize, usize), end: (usize, usize) },
}

#[derive(Debug, Clone)]
pub(crate) struct EditorState {
    pub(super) lines: Vec<String>,
    pub(super) cursor_row: usize,
    pub(super) cursor_col: usize,
    pub(super) scroll_row: usize,
    pub(super) scroll_col: usize,
    pub(super) dirty: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct BufferState {
    pub(crate) lines: Vec<String>,
    pub(crate) cursor_row: usize,
    pub(crate) cursor_col: usize,
    pub(crate) scroll_row: usize,
    pub(crate) scroll_col: usize,
    pub(crate) file_path: Option<PathBuf>,
    pub(crate) dirty: bool,
    pub(crate) undo_stack: Vec<EditorState>,
    pub(crate) redo_stack: Vec<EditorState>,
    pub(crate) line_undo: Option<LineUndo>,
    pub(crate) is_restoring: bool,
    pub(crate) change_tick: u64,
    pub(crate) edit_tick: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct BufferSlot {
    pub(crate) id: usize,
    pub(crate) state: BufferState,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BlockInsert {
    pub(super) start_row: usize,
    pub(super) end_row: usize,
    pub(super) col: usize,
    pub(super) append: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct LineUndo {
    pub(super) row: usize,
    pub(super) line: String,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LastVisual {
    pub(super) mode: Mode,
    pub(super) start: (usize, usize),
    pub(super) end: (usize, usize),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FindPending {
    pub(super) until: bool,
    pub(super) reverse: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FindSpec {
    pub(super) ch: char,
    pub(super) until: bool,
    pub(super) reverse: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct SearchSpec {
    pub(crate) pattern: String,
    pub(crate) reverse: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum TextObjectKind {
    Inner,
    Around,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum TextObjectTarget {
    Word,
    Brace,
    Paren,
    Bracket,
    Angle,
    Tag,
    QuoteSingle,
    QuoteDouble,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TextObjectPending {
    pub(super) kind: TextObjectKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CharClass {
    Space,
    Word,
    Punct,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RepeatKey {
    pub(crate) code: KeyCode,
    pub(crate) modifiers: KeyModifiers,
}

pub(super) fn char_class(ch: char) -> CharClass {
    if ch.is_whitespace() {
        CharClass::Space
    } else if ch.is_alphanumeric() || ch == '_' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

pub(super) fn is_undo_break_char(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | '(' | ')' | '[' | ']' | '{' | '}')
}

pub(super) fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or_else(|| s.len())
}

pub fn line_screen_width(s: &str, tab_width: usize) -> usize {
    let mut col = 0usize;
    for ch in s.chars() {
        col += char_display_width(ch, col, tab_width);
    }
    col
}

pub fn char_to_screen_col(s: &str, char_idx: usize, tab_width: usize) -> usize {
    let mut col = 0usize;
    for (i, ch) in s.chars().enumerate() {
        if i >= char_idx {
            break;
        }
        col += char_display_width(ch, col, tab_width);
    }
    col
}

pub(crate) fn screen_col_to_char_idx(s: &str, screen_col: usize, tab_width: usize) -> usize {
    let mut col = 0usize;
    for (i, ch) in s.chars().enumerate() {
        let w = char_display_width(ch, col, tab_width);
        if col + w > screen_col {
            return i;
        }
        col += w;
    }
    s.chars().count()
}

pub fn char_display_width(ch: char, col: usize, tab_width: usize) -> usize {
    if ch == '\t' {
        let width = tab_width.max(1);
        return width - (col % width);
    }
    unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0).max(1)
}

pub(super) fn normalize_range(
    a: (usize, usize),
    b: (usize, usize),
) -> ((usize, usize), (usize, usize)) {
    if pos_le(a, b) {
        (a, b)
    } else {
        (b, a)
    }
}

fn pos_le(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.0 || (a.0 == b.0 && a.1 <= b.1)
}
