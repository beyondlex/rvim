use std::path::PathBuf;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::prelude::Color;

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
pub(super) enum Operator {
    Delete,
    Yank,
    Change,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum YankType {
    Char,
    Line,
    Block,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct OperatorPending {
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
    pub(crate) yank_buffer: String,
    pub(crate) yank_type: YankType,
    pub(crate) visual_start: Option<(usize, usize)>,
    pub(crate) block_insert: Option<BlockInsert>,
    pub(crate) last_visual: Option<LastVisual>,
    pub(crate) insert_undo_snapshot: bool,
    pub(crate) pending_find: Option<FindPending>,
    pub(crate) pending_g: bool,
    pub(crate) operator_pending: Option<OperatorPending>,
    pub(crate) last_find: Option<FindSpec>,
    pub(crate) pending_textobj: Option<TextObjectPending>,
    pub(crate) quit_confirm: bool,
    pub(crate) status_time: Option<Instant>,
    pub(crate) undo_limit: usize,
    pub(crate) line_undo: Option<LineUndo>,
    pub(crate) is_restoring: bool,
    pub(crate) command_prompt: CommandPrompt,
    pub(crate) last_search: Option<SearchSpec>,
    pub(crate) search_history: Vec<String>,
    pub(crate) search_history_index: Option<usize>,
    pub(crate) repeat_recording: bool,
    pub(crate) repeat_replaying: bool,
    pub(crate) repeat_changed: bool,
    pub(crate) repeat_buffer: Vec<RepeatKey>,
    pub(crate) last_change: Vec<RepeatKey>,
    pub(crate) change_tick: u64,
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
pub(super) struct EditorState {
    pub(super) lines: Vec<String>,
    pub(super) cursor_row: usize,
    pub(super) cursor_col: usize,
    pub(super) scroll_row: usize,
    pub(super) scroll_col: usize,
    pub(super) dirty: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct BlockInsert {
    pub(super) start_row: usize,
    pub(super) end_row: usize,
    pub(super) col: usize,
    pub(super) append: bool,
}

#[derive(Debug, Clone)]
pub(super) struct LineUndo {
    pub(super) row: usize,
    pub(super) line: String,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LastVisual {
    pub(super) mode: Mode,
    pub(super) start: (usize, usize),
    pub(super) end: (usize, usize),
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FindPending {
    pub(super) until: bool,
    pub(super) reverse: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FindSpec {
    pub(super) ch: char,
    pub(super) until: bool,
    pub(super) reverse: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct SearchSpec {
    pub(crate) pattern: String,
    pub(crate) reverse: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Theme {
    pub(crate) status_fg: Color,
    pub(crate) status_bg: Color,
    pub(crate) line_number_fg: Color,
    pub(crate) line_number_fg_current: Color,
    pub(crate) current_line_bg: Color,
    pub(crate) selection_fg: Color,
    pub(crate) selection_bg: Color,
    pub(crate) search_fg: Color,
    pub(crate) search_bg: Color,
}

impl Theme {
    pub(crate) fn default_theme() -> Self {
        Self {
            status_fg: Color::Black,
            status_bg: Color::White,
            line_number_fg: Color::DarkGray,
            line_number_fg_current: Color::Rgb(255, 165, 0),
            current_line_bg: Color::Rgb(64, 64, 64),
            selection_fg: Color::Black,
            selection_bg: Color::Cyan,
            search_fg: Color::Black,
            search_bg: Color::Yellow,
        }
    }
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
pub(super) struct TextObjectPending {
    pub(super) kind: TextObjectKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CharClass {
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
