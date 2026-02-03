use std::path::PathBuf;
use std::time::Instant;

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
