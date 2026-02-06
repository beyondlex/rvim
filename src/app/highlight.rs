use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use tree_sitter::{Language, Parser, Query, QueryCursor, Tree, StreamingIterator};

use crate::logging::timestamp_prefix;
use super::App;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightKind {
    Keyword,
    String,
    Comment,
    Function,
    Type,
    Constant,
    Number,
    Operator,
    Property,
    Variable,
    Macro,
    Attribute,
    Punctuation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxSpan {
    pub(crate) start_col: usize,
    pub(crate) end_col: usize,
    pub(crate) kind: HighlightKind,
}

#[derive(Debug)]
struct QuerySource {
    text: String,
    key: String,
}

pub(crate) struct SyntaxState {
    language_name: &'static str,
    query_key: String,
    parser: Parser,
    tree: Option<Tree>,
    query: Query,
    inline_parser: Option<Parser>,
    inline_query: Option<Query>,
    source: String,
    line_offsets: Vec<usize>,
    cache_tick: u64,
    debug_last_log_tick: u64,
    cache: HashMap<usize, Vec<SyntaxSpan>>,
    line_cache: HashMap<usize, Vec<SyntaxSpan>>,
    line_cache_order: Vec<usize>,
    line_cache_limit: usize,
    viewport_tick: u64,
    viewport_start: usize,
    viewport_rows: usize,
    viewport_cache: HashMap<usize, Vec<SyntaxSpan>>,
}

#[derive(Debug, Clone)]
struct LanguageSpec {
    name: &'static str,
    extensions: &'static [&'static str],
    language: Language,
    default_query: Option<&'static str>,
}

fn detect_language(path: &Option<PathBuf>) -> Option<LanguageSpec> {
    let ext = path
        .as_ref()
        .and_then(|p| p.extension())
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())?;
    for spec in language_specs() {
        if spec.extensions.iter().any(|e| *e == ext) {
            return Some(spec);
        }
    }
    None
}

pub fn detect_language_name(path: &Option<PathBuf>) -> Option<&'static str> {
    detect_language(path).map(|spec| spec.name)
}

pub fn has_query_for_language(path: &Option<PathBuf>) -> bool {
    detect_language(path)
        .and_then(|spec| load_query_for_language(&spec))
        .is_some()
}

pub(crate) fn syntax_spans_for_state(
    state: &mut SyntaxState,
    lines: &[String],
    start_row: usize,
    rows: usize,
    edit_tick: u64,
) -> HashMap<usize, Vec<SyntaxSpan>> {
    if let Err(err) = state.sync(lines, edit_tick) {
        debug_log(&format!("syntax: sync failed: {}", err));
        return HashMap::new();
    }
    let line_count = lines.len();
    if line_count == 0 {
        return HashMap::new();
    }
    let end_row = (start_row + rows).min(line_count);
    if state.viewport_tick == edit_tick
        && state.viewport_start <= start_row
        && state.viewport_start + state.viewport_rows >= end_row
    {
        let mut out: HashMap<usize, Vec<SyntaxSpan>> = HashMap::new();
        let mut missing_start: Option<usize> = None;
        let mut missing_len = 0usize;
        for row in start_row..end_row {
            if let Some(spans) = take_line_cache(state, row) {
                if let Some(ms) = missing_start.take() {
                    let partial = compute_spans_for_range(state, lines, ms, missing_len, edit_tick);
                    out.extend(partial);
                    missing_len = 0;
                }
                out.insert(row, spans);
            } else {
                if missing_start.is_none() {
                    missing_start = Some(row);
                }
                missing_len += 1;
            }
        }
        if let Some(ms) = missing_start {
            let partial = compute_spans_for_range(state, lines, ms, missing_len, edit_tick);
            out.extend(partial);
        }
        for (row, spans) in &out {
            cache_line_spans(state, *row, spans.clone());
        }
        state.viewport_tick = edit_tick;
        state.viewport_start = start_row;
        state.viewport_rows = rows;
        state.viewport_cache = out.clone();
        return out;
    }
    let mut out: HashMap<usize, Vec<SyntaxSpan>> = HashMap::new();
    let mut missing_start: Option<usize> = None;
    let mut missing_len = 0usize;
    for row in start_row..end_row {
        if let Some(spans) = take_line_cache(state, row) {
            if let Some(ms) = missing_start.take() {
                let partial = compute_spans_for_range(state, lines, ms, missing_len, edit_tick);
                out.extend(partial);
                missing_len = 0;
            }
            out.insert(row, spans);
        } else {
            if missing_start.is_none() {
                missing_start = Some(row);
            }
            missing_len += 1;
        }
    }
    if let Some(ms) = missing_start {
        let partial = compute_spans_for_range(state, lines, ms, missing_len, edit_tick);
        out.extend(partial);
    }
    for (row, spans) in &out {
        cache_line_spans(state, *row, spans.clone());
    }
    state.viewport_tick = edit_tick;
    state.viewport_start = start_row;
    state.viewport_rows = rows;
    state.viewport_cache = out.clone();
    out
}

fn compute_spans_for_range(
    state: &mut SyntaxState,
    lines: &[String],
    start_row: usize,
    rows: usize,
    edit_tick: u64,
) -> HashMap<usize, Vec<SyntaxSpan>> {
    let line_count = lines.len();
    if line_count == 0 {
        return HashMap::new();
    }
    let end_row = (start_row + rows).min(line_count);
    let start_byte = state.line_offsets.get(start_row).copied().unwrap_or(0);
    let end_byte = state
        .line_offsets
        .get(end_row)
        .copied()
        .unwrap_or(state.source.len());
    let tree = match state.tree.as_ref() {
        Some(tree) => tree,
        None => return HashMap::new(),
    };
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(start_byte..end_byte);
    let mut out: HashMap<usize, Vec<SyntaxSpan>> = HashMap::new();
    let root = tree.root_node();
    let mut captures = cursor.captures(&state.query, root, state.source.as_bytes());
    loop {
        captures.advance();
        let Some((m, idx)) = captures.get() else {
            break;
        };
        let capture = &m.captures[*idx];
        let name = state.query.capture_names()[capture.index as usize];
        let Some(kind) = capture_to_kind(name) else {
            continue;
        };
        let node = capture.node;
        let start = node.start_byte();
        let end = node.end_byte();
        if start >= end {
            continue;
        }
        let start_row_cap = node.start_position().row as usize;
        let end_row_cap = node.end_position().row as usize;
        for row in start_row_cap..=end_row_cap {
            if row < start_row || row >= end_row {
                continue;
            }
            let line = match lines.get(row) {
                Some(line) => line,
                None => continue,
            };
            let line_start = state.line_offsets.get(row).copied().unwrap_or(0);
            let line_end = state
                .line_offsets
                .get(row + 1)
                .copied()
                .unwrap_or(state.source.len());
            let seg_start = if row == start_row_cap { start } else { line_start };
            let seg_end = if row == end_row_cap { end } else { line_end };
            if seg_end <= seg_start || seg_start < line_start || seg_end > line_end {
                continue;
            }
            if let Some((start_col, end_col)) =
                byte_range_to_col_range(line, line_start, seg_start, seg_end)
            {
                out.entry(row)
                    .or_default()
                    .push(SyntaxSpan { start_col, end_col, kind });
            }
        }
    }
    add_markdown_inline_spans(state, lines, start_row, end_row, &mut out);
    for spans in out.values_mut() {
        spans.sort_by(|a, b| (a.start_col, a.end_col).cmp(&(b.start_col, b.end_col)));
        normalize_spans(spans);
    }
    if std::env::var("RVIM_DEBUG_SYNTAX").ok().as_deref() == Some("1") {
        if state.debug_last_log_tick != edit_tick {
            let span_total: usize = out.values().map(|v| v.len()).sum();
            debug_log(&format!(
                "syntax: viewport rows {}..{} spans {}",
                start_row,
                end_row,
                span_total
            ));
            state.debug_last_log_tick = edit_tick;
        }
    } else if out.is_empty() {
        debug_log("syntax: no spans produced for viewport");
    }
    for row in start_row..end_row {
        out.entry(row).or_insert_with(Vec::new);
    }
    out
}

fn cache_line_spans(state: &mut SyntaxState, row: usize, spans: Vec<SyntaxSpan>) {
    if state.line_cache.contains_key(&row) {
        state.line_cache.insert(row, spans);
        return;
    }
    state.line_cache.insert(row, spans);
    state.line_cache_order.push(row);
    if state.line_cache_order.len() > state.line_cache_limit {
        if let Some(oldest) = state.line_cache_order.first().copied() {
            state.line_cache.remove(&oldest);
        }
        state.line_cache_order.remove(0);
    }
}

fn take_line_cache(state: &mut SyntaxState, row: usize) -> Option<Vec<SyntaxSpan>> {
    if let Some(spans) = state.line_cache.get(&row).cloned() {
        if let Some(pos) = state.line_cache_order.iter().position(|r| *r == row) {
            state.line_cache_order.remove(pos);
            state.line_cache_order.push(row);
        }
        return Some(spans);
    }
    None
}

fn load_markdown_inline_query() -> Option<QuerySource> {
    if let Some(q) = load_query_from_paths("markdown-inline") {
        return Some(q);
    }
    Some(QuerySource {
        text: tree_sitter_md::HIGHLIGHT_QUERY_INLINE.to_string(),
        key: "builtin:markdown-inline".to_string(),
    })
}

fn add_markdown_inline_spans(
    state: &mut SyntaxState,
    lines: &[String],
    start_row: usize,
    end_row: usize,
    out: &mut HashMap<usize, Vec<SyntaxSpan>>,
) {
    let (Some(inline_parser), Some(inline_query)) =
        (state.inline_parser.as_mut(), state.inline_query.as_ref())
    else {
        return;
    };
    let tree = match state.tree.as_ref() {
        Some(tree) => tree,
        None => return,
    };
    let root = tree.root_node();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "inline" {
            let inline_start = node.start_byte();
            let inline_end = node.end_byte();
            if inline_start >= inline_end {
                continue;
            }
            let inline_start_row = node.start_position().row as usize;
            let inline_end_row = node.end_position().row as usize;
            if inline_end_row < start_row || inline_start_row >= end_row {
                continue;
            }
            let Some(slice) = state.source.get(inline_start..inline_end) else {
                continue;
            };
            let inline_tree = match inline_parser.parse(slice, None) {
                Some(tree) => tree,
                None => continue,
            };
            let mut cursor = QueryCursor::new();
            let inline_root = inline_tree.root_node();
            let mut captures = cursor.captures(inline_query, inline_root, slice.as_bytes());
            loop {
                captures.advance();
                let Some((m, idx)) = captures.get() else {
                    break;
                };
                let capture = &m.captures[*idx];
                let name = inline_query.capture_names()[capture.index as usize];
                let Some(kind) = capture_to_kind(name) else {
                    continue;
                };
                let node = capture.node;
                let start = inline_start + node.start_byte();
                let end = inline_start + node.end_byte();
                if start >= end {
                    continue;
                }
                let start_row_cap = inline_start_row + node.start_position().row as usize;
                let end_row_cap = inline_start_row + node.end_position().row as usize;
                for row in start_row_cap..=end_row_cap {
                    if row < start_row || row >= end_row {
                        continue;
                    }
                    let line = match lines.get(row) {
                        Some(line) => line,
                        None => continue,
                    };
                    let line_start = state.line_offsets.get(row).copied().unwrap_or(0);
                    let line_end = state
                        .line_offsets
                        .get(row + 1)
                        .copied()
                        .unwrap_or(state.source.len());
                    let seg_start = if row == start_row_cap { start } else { line_start };
                    let seg_end = if row == end_row_cap { end } else { line_end };
                    if seg_end <= seg_start || seg_start < line_start || seg_end > line_end {
                        continue;
                    }
                    if let Some((start_col, end_col)) =
                        byte_range_to_col_range(line, line_start, seg_start, seg_end)
                    {
                        out.entry(row)
                            .or_default()
                            .push(SyntaxSpan { start_col, end_col, kind });
                    }
                }
            }
            continue;
        }
        for i in (0..node.child_count()).rev() {
            if let Some(child) = node.child(i) {
                stack.push(child);
            }
        }
    }
}

pub fn total_spans(spans: &Option<HashMap<usize, Vec<SyntaxSpan>>>) -> usize {
    spans
        .as_ref()
        .map(|m| m.values().map(|v| v.len()).sum())
        .unwrap_or(0)
}

impl App {
    pub(crate) fn syntax_debug_summary(&self) -> String {
        let lang = detect_language_name(&self.file_path).unwrap_or("none");
        let query = if has_query_for_language(&self.file_path) { "ok" } else { "none" };
        let state = self.syntax_by_buffer.get(&self.current_buffer_id);
        match state {
            None => format!("syntax:{} query:{} state:none", lang, query),
            Some(state) => format!(
                "syntax:{} query:{} tree:{} src:{} lines:{} cache:{} edit:{}",
                lang,
                query,
                state.tree.is_some(),
                state.source.len(),
                state.line_offsets.len(),
                state.cache_tick,
                self.edit_tick
            ),
        }
    }
}

impl SyntaxState {
    fn new(spec: LanguageSpec, query: QuerySource) -> Result<Self> {
        let mut parser = Parser::new();
        parser
            .set_language(&spec.language)
            .context("set language")?;
        let query_key = query.key.clone();
        let query = Query::new(&spec.language, &query.text)
            .map_err(|err| anyhow::anyhow!("compile query: {}", err))?;
        let (inline_parser, inline_query) = if spec.name == "markdown" {
            match load_markdown_inline_query() {
                Some(inline) => {
                    let mut parser = Parser::new();
                    match parser.set_language(&tree_sitter_md::INLINE_LANGUAGE.into()) {
                        Ok(_) => match Query::new(&tree_sitter_md::INLINE_LANGUAGE.into(), &inline.text)
                        {
                            Ok(query) => (Some(parser), Some(query)),
                            Err(err) => {
                                debug_log(&format!("syntax: markdown inline query failed: {}", err));
                                (None, None)
                            }
                        },
                        Err(err) => {
                            debug_log(&format!("syntax: markdown inline language failed: {}", err));
                            (None, None)
                        }
                    }
                }
                None => (None, None),
            }
        } else {
            (None, None)
        };
        Ok(Self {
            language_name: spec.name,
            query_key,
            parser,
            tree: None,
            query,
            inline_parser,
            inline_query,
            source: String::new(),
            line_offsets: Vec::new(),
            cache_tick: u64::MAX,
            debug_last_log_tick: u64::MAX,
            cache: HashMap::new(),
            line_cache: HashMap::new(),
            line_cache_order: Vec::new(),
            line_cache_limit: 2000,
            viewport_tick: u64::MAX,
            viewport_start: 0,
            viewport_rows: 0,
            viewport_cache: HashMap::new(),
        })
    }

    fn sync(&mut self, lines: &[String], edit_tick: u64) -> Result<()> {
        if self.cache_tick == edit_tick && self.tree.is_some() {
            return Ok(());
        }
        let (source, offsets) = build_source(lines);
        let tree = self.parser.parse(&source, None).context("parse")?;
        self.source = source;
        self.line_offsets = offsets;
        self.tree = Some(tree);
        self.cache_tick = edit_tick;
        self.cache.clear();
        self.line_cache.clear();
        self.line_cache_order.clear();
        self.viewport_tick = u64::MAX;
        self.viewport_cache.clear();
        Ok(())
    }
}

fn language_specs() -> Vec<LanguageSpec> {
    vec![
        LanguageSpec {
            name: "rust",
            extensions: &["rs"],
            language: tree_sitter_rust::LANGUAGE.into(),
            default_query: Some(RUST_QUERY),
        },
        LanguageSpec {
            name: "javascript",
            extensions: &["js", "mjs", "cjs", "jsx"],
            language: tree_sitter_javascript::LANGUAGE.into(),
            default_query: None,
        },
        LanguageSpec {
            name: "typescript",
            extensions: &["ts"],
            language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            default_query: None,
        },
        LanguageSpec {
            name: "tsx",
            extensions: &["tsx"],
            language: tree_sitter_typescript::LANGUAGE_TSX.into(),
            default_query: None,
        },
        LanguageSpec {
            name: "json",
            extensions: &["json"],
            language: tree_sitter_json::LANGUAGE.into(),
            default_query: None,
        },
        LanguageSpec {
            name: "toml",
            extensions: &["toml"],
            language: tree_sitter_toml::LANGUAGE.into(),
            default_query: None,
        },
        LanguageSpec {
            name: "markdown",
            extensions: &["md", "markdown", "mdx"],
            language: tree_sitter_md::LANGUAGE.into(),
            default_query: None,
        },
        LanguageSpec {
            name: "html",
            extensions: &["html", "htm"],
            language: tree_sitter_html::LANGUAGE.into(),
            default_query: None,
        },
        LanguageSpec {
            name: "css",
            extensions: &["css", "scss"],
            language: tree_sitter_css::LANGUAGE.into(),
            default_query: None,
        },
        LanguageSpec {
            name: "python",
            extensions: &["py"],
            language: tree_sitter_python::LANGUAGE.into(),
            default_query: None,
        },
        LanguageSpec {
            name: "lua",
            extensions: &["lua"],
            language: tree_sitter_lua::LANGUAGE.into(),
            default_query: None,
        },
        LanguageSpec {
            name: "go",
            extensions: &["go"],
            language: tree_sitter_go::LANGUAGE.into(),
            default_query: None,
        },
    ]
}

fn load_query_for_language(spec: &LanguageSpec) -> Option<QuerySource> {
    if let Some(q) = load_query_from_paths(spec.name) {
        return Some(q);
    }
    spec.default_query.map(|text| QuerySource {
        text: text.to_string(),
        key: format!("builtin:{}", spec.name),
    })
}

fn load_query_from_paths(lang: &str) -> Option<QuerySource> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    candidates.push(PathBuf::from("queries").join(lang).join("highlights.scm"));
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(
            PathBuf::from(home)
                .join(".config/rvim/queries")
                .join(lang)
                .join("highlights.scm"),
        );
    }
    for path in candidates {
        if !path.exists() {
            continue;
        }
        if let Ok(text) = fs::read_to_string(&path) {
            return Some(QuerySource {
                text,
                key: path.to_string_lossy().to_string(),
            });
        }
    }
    None
}

fn ensure_syntax_state(
    map: &mut HashMap<usize, SyntaxState>,
    buffer_id: usize,
    spec: LanguageSpec,
) -> Option<&mut SyntaxState> {
    let query = match load_query_for_language(&spec) {
        Some(query) => query,
        None => {
            debug_log(&format!("syntax: no query for language {}", spec.name));
            return None;
        }
    };
    let needs_reset = match map.get(&buffer_id) {
        Some(state) => state.language_name != spec.name || state.query_key != query.key,
        None => true,
    };
    if needs_reset {
        match SyntaxState::new(spec, query) {
            Ok(state) => {
                map.insert(buffer_id, state);
            }
            Err(err) => {
                debug_log(&format!("syntax: init failed: {}", err));
                return None;
            }
        }
    }
    map.get_mut(&buffer_id)
}

fn capture_to_kind(name: &str) -> Option<HighlightKind> {
    let base = name.split('.').next().unwrap_or(name);
    match base {
        "keyword" => Some(HighlightKind::Keyword),
        "string" => Some(HighlightKind::String),
        "text" => Some(HighlightKind::String),
        "comment" => Some(HighlightKind::Comment),
        "function" => Some(HighlightKind::Function),
        "type" => Some(HighlightKind::Type),
        "constant" => Some(HighlightKind::Constant),
        "number" => Some(HighlightKind::Number),
        "operator" => Some(HighlightKind::Operator),
        "property" => Some(HighlightKind::Property),
        "variable" => Some(HighlightKind::Variable),
        "macro" => Some(HighlightKind::Macro),
        "attribute" => Some(HighlightKind::Attribute),
        "punctuation" => Some(HighlightKind::Punctuation),
        _ => None,
    }
}

fn build_source(lines: &[String]) -> (String, Vec<usize>) {
    let mut source = String::new();
    let mut offsets = Vec::with_capacity(lines.len() + 1);
    let mut offset = 0usize;
    for (idx, line) in lines.iter().enumerate() {
        offsets.push(offset);
        source.push_str(line);
        offset += line.len();
        if idx + 1 < lines.len() {
            source.push('\n');
            offset += 1;
        }
    }
    offsets.push(offset);
    (source, offsets)
}

fn byte_range_to_col_range(
    line: &str,
    line_start: usize,
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    let local_start = start.saturating_sub(line_start);
    let local_end = end.saturating_sub(line_start);
    if local_start >= local_end || local_start >= line.len() {
        return None;
    }
    let mut start_col = None;
    let mut end_col = None;
    for (idx, (byte_idx, ch)) in line.char_indices().enumerate() {
        let ch_end = byte_idx + ch.len_utf8();
        if ch_end <= local_start {
            continue;
        }
        if byte_idx >= local_end {
            break;
        }
        if start_col.is_none() {
            start_col = Some(idx);
        }
        end_col = Some(idx + 1);
    }
    match (start_col, end_col) {
        (Some(s), Some(e)) if s < e => Some((s, e)),
        _ => None,
    }
}

fn normalize_spans(spans: &mut Vec<SyntaxSpan>) {
    let mut out: Vec<SyntaxSpan> = Vec::with_capacity(spans.len());
    for span in spans.drain(..) {
        if let Some(last) = out.last_mut() {
            if span.start_col < last.end_col {
                if span.end_col > last.end_col {
                    *last = span;
                }
                continue;
            }
        }
        out.push(span);
    }
    spans.extend(out);
}

impl App {
    pub(crate) fn syntax_spans_for_viewport(
        &mut self,
        start_row: usize,
        rows: usize,
    ) -> Option<HashMap<usize, Vec<SyntaxSpan>>> {
        let spec = match detect_language(&self.file_path) {
            Some(spec) => spec,
            None => {
                debug_log("syntax: no language detected for current file");
                return None;
            }
        };
        let state = match ensure_syntax_state(&mut self.syntax_by_buffer, self.current_buffer_id, spec) {
            Some(state) => state,
            None => {
                if std::env::var("RVIM_DEBUG_SYNTAX").ok().as_deref() == Some("1") {
                    self.set_status("syntax: init failed (see log)");
                }
                return None;
            }
        };
        Some(syntax_spans_for_state(
            state,
            &self.lines,
            start_row,
            rows,
            self.edit_tick,
        ))
    }
}

fn debug_log(message: &str) {
    if std::env::var("RVIM_DEBUG_SYNTAX").ok().as_deref() != Some("1") {
        return;
    }
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let mut path = PathBuf::from(home);
    path.push(".config/rvim");
    let _ = fs::create_dir_all(&path);
    path.push("rvim.log");
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} {}", timestamp_prefix(), message);
    }
}

const RUST_QUERY: &str = r#"
; Minimal query to validate parser + rendering.
; Expand as needed once verified.

(identifier) @variable
(field_identifier) @property
(type_identifier) @type
(primitive_type) @type
"#;
