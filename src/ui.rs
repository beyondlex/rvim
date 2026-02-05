use std::io;

use anyhow::Result;
use crossterm::cursor::SetCursorStyle;
use crossterm::execute;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::{App, CommandPrompt, HighlightKind, Mode, SyntaxSpan, VisualSelection, VisualSelectionKind, total_spans, detect_language_name, has_query_for_language};
use crate::app::{char_display_width, char_to_screen_col, line_screen_width};

pub fn apply_cursor_style(app: &App) -> Result<()> {
    match app.mode {
        Mode::Insert => {
            execute!(io::stdout(), SetCursorStyle::SteadyBar)?;
        }
        _ => {
            execute!(io::stdout(), SetCursorStyle::SteadyBlock)?;
        }
    }
    Ok(())
}

pub fn ui(f: &mut Frame<'_>, app: &mut App) {
    let size = f.area();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)])
        .split(size);

    let main_area = rows[0];
    let status_area = rows[1];
    let message_area = rows[2];

    let viewport_rows = main_area.height as usize;
    let line_count = app.lines.len().max(1);
    let gutter_width = line_count.to_string().len() + 1;
    let viewport_cols = main_area
        .width
        .saturating_sub(gutter_width as u16)
        .max(1) as usize;
    app.ensure_cursor_visible(viewport_rows, viewport_cols);

    let mut text_lines: Vec<Line> = Vec::with_capacity(viewport_rows);
    let selection = app.visual_selection();
    let syntax = app.syntax_spans_for_viewport(app.scroll_row, viewport_rows);
    let debug_syntax = std::env::var("RVIM_DEBUG_SYNTAX").ok().as_deref() == Some("1");
    for i in 0..viewport_rows {
        let idx = app.scroll_row + i;
        if let Some(line) = app.lines.get(idx) {
            let scroll_screen = char_to_screen_col(line, app.scroll_col, app.shift_width);
            let syntax_spans = syntax.as_ref().and_then(|m| m.get(&idx)).map(|v| v.as_slice());
            text_lines.push(render_line_with_selection(
                line,
                idx,
                scroll_screen,
                viewport_cols,
                selection,
                syntax_spans,
                app.last_search.as_ref().map(|s| s.pattern.as_str()),
                gutter_width,
                idx == app.cursor_row,
                app.relative_number,
                app.cursor_row,
                app,
            ));
        } else {
            text_lines.push(render_empty_line(gutter_width));
        }
    }

    let paragraph = Paragraph::new(text_lines).block(Block::default().borders(Borders::NONE));
    f.render_widget(paragraph, main_area);

    let mode_label = match app.mode {
        Mode::Normal => "NORMAL",
        Mode::Insert => "INSERT",
        Mode::Command => match app.command_prompt {
            CommandPrompt::Command => "COMMAND",
            CommandPrompt::SearchForward | CommandPrompt::SearchBackward => "SEARCH",
        },
        Mode::VisualChar => "VISUAL",
        Mode::VisualLine => "VISUAL LINE",
        Mode::VisualBlock => "VISUAL BLOCK",
    };
    let file_label = app
        .file_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "[No Name]".to_string());
    let dirty = if app.dirty { " [+]" } else { "" };
    let mut status = format!(
        "{} | {}{} | buf:{}/{} | {}:{}",
        mode_label,
        file_label,
        dirty,
        app.current_buffer_id,
        app.buffer_count(),
        app.cursor_row + 1,
        app.cursor_col + 1
    );
    status.push_str(&format!(" | undo:{} redo:{}", app.undo_len(), app.redo_len()));
    status.push_str(&format!(" | theme:{}", app.theme_name));
    if app.mode == Mode::Command && app.command_buffer.starts_with("set theme=") {
        status.push_str(" | themes: light dark solarized");
    }
    if debug_syntax {
        status.push_str(&format!(" | {} spans:{}", app.syntax_debug_summary(), total_spans(&syntax)));
    }
    if app.mode == Mode::Command
        && !app.completion_candidates.is_empty()
        && app.command_prompt == CommandPrompt::Command
    {
        let total = app.completion_candidates.len();
        let idx = app.completion_index.unwrap_or(0) + 1;
        status.push_str(&format!(" | tab:{}/{}", idx, total));
    }
    if matches!(app.mode, Mode::VisualChar | Mode::VisualLine | Mode::VisualBlock) {
        if let Some(summary) = app.selection_summary() {
            status.push_str(" | ");
            status.push_str(&summary);
        }
    }

    let status_paragraph = Paragraph::new(status).style(
        Style::default()
            .fg(app.theme.status_fg)
            .bg(app.theme.status_bg),
    );
    f.render_widget(status_paragraph, status_area);

    let message = if app.mode == Mode::Command {
        let prefix = match app.command_prompt {
            CommandPrompt::Command => ':',
            CommandPrompt::SearchForward => '/',
            CommandPrompt::SearchBackward => '?',
        };
        Paragraph::new(format!("{}{}", prefix, app.command_buffer))
    } else {
        Paragraph::new(app.status_message.clone())
    };
    f.render_widget(message, message_area);

    if app.mode == Mode::Command
        && app.command_prompt == CommandPrompt::Command
        && !app.completion_candidates.is_empty()
    {
        render_completion_popover(f, app, main_area, message_area);
    }

    if app.mode == Mode::Command {
        let cursor_x = message_area.x + 1 + app.command_buffer.chars().count() as u16;
        let cursor_y = message_area.y;
        if cursor_x < message_area.right() && cursor_y < message_area.bottom() {
            f.set_cursor_position(Position::new(cursor_x, cursor_y));
        }
    } else {
        let line = app.lines.get(app.cursor_row).map(|s| s.as_str()).unwrap_or("");
        let cursor_screen = char_to_screen_col(line, app.cursor_col, app.shift_width);
        let scroll_screen = char_to_screen_col(line, app.scroll_col, app.shift_width);
        let cursor_x = cursor_screen.saturating_sub(scroll_screen) as u16
            + main_area.x
            + gutter_width as u16;
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

fn render_completion_popover(f: &mut Frame<'_>, app: &App, main_area: Rect, message_area: Rect) {
    let labels = completion_labels(app);
    if labels.is_empty() {
        return;
    }

    let max_len = completion_max_label_len(app);
    let width = (max_len + 2).min(main_area.width as usize).max(4) as u16;
    let height = 6
        .min(labels.len())
        .min(main_area.height as usize)
        .max(1) as u16;

    let anchor_x = completion_anchor_x(app, message_area);
    let x = anchor_x
        .saturating_add(1)
        .min(main_area.right().saturating_sub(width))
        .max(main_area.x);
    let y = message_area.y.saturating_sub(height).max(main_area.y);

    let area = Rect { x, y, width, height };
    let lines = completion_window(app, &labels, width as usize, height as usize);
    f.render_widget(Clear, area);
    let widget = Paragraph::new(lines).style(
        Style::default()
            .bg(app.theme.current_line_bg)
            .add_modifier(Modifier::DIM),
    );
    f.render_widget(widget, area);
}

fn completion_labels(app: &App) -> Vec<String> {
    let total = app.completion_candidates.len();
    if total == 0 {
        return Vec::new();
    }
    app.completion_candidates
        .iter()
        .map(|c| completion_item_label(c))
        .collect()
}

fn completion_max_label_len(app: &App) -> usize {
    let mut max_len = 0usize;
    for candidate in &app.completion_candidates {
        let label = completion_item_label(candidate);
        max_len = max_len.max(label.chars().count());
    }
    max_len
}

fn completion_window(
    app: &App,
    labels: &[String],
    width: usize,
    window_size: usize,
) -> Vec<Line<'static>> {
    if labels.is_empty() {
        return Vec::new();
    }
    let text_width = width.saturating_sub(1);
    let total = labels.len();
    let window = window_size.min(total).max(1);
    let selected = app.completion_index.unwrap_or(0).min(total.saturating_sub(1));
    let anchor = window / 2;
    let mut window_start = if selected <= anchor {
        0
    } else {
        selected - anchor
    };
    if window_start + window > total {
        window_start = total - window;
    }
    let selected_pos = selected.saturating_sub(window_start);
    let scroll = completion_scrollbar(total, window, selected);
    let mut out = Vec::with_capacity(window);
    for i in 0..window {
        let label = &labels[window_start + i];
        let mut text = format!(" {}", label);
        let text_len = text.chars().count();
        if text_len < text_width {
            text.push_str(&" ".repeat(text_width - text_len));
        } else if text_len > text_width {
            text = text.chars().take(text_width).collect();
        }
        let bar = if scroll.contains(&i) { '█' } else { ' ' };
        let line = if i == selected_pos {
            Line::from(vec![
                Span::styled(
                    text,
                    Style::default()
                        .fg(app.theme.selection_fg)
                        .bg(app.theme.selection_bg),
                ),
                Span::styled(bar.to_string(), Style::default().fg(app.theme.search_bg)),
            ])
        } else {
            Line::from(vec![
                Span::raw(text),
                Span::styled(bar.to_string(), Style::default().fg(app.theme.search_bg)),
            ])
        };
        out.push(line);
    }
    out
}

fn completion_scrollbar(total: usize, window: usize, selected: usize) -> std::ops::Range<usize> {
    if total == 0 || window == 0 || total <= window {
        return 0..0;
    }
    let thumb_size = (window * window / total).max(1);
    let max_start = window.saturating_sub(thumb_size);
    let thumb_start = ((selected * window) / total).min(max_start);
    thumb_start..(thumb_start + thumb_size)
}

fn completion_item_label(candidate: &str) -> String {
    let mut s = candidate.to_string();
    if let Some(quote) = s.chars().next() {
        if quote == '"' || quote == '\'' {
            s = s[1..].to_string();
        }
    }
    let unescaped = unescape_display(&s);
    let is_dir = s.ends_with('/');
    let mut base = match unescaped.rfind('/') {
        Some(idx) => unescaped[idx + 1..].to_string(),
        None => unescaped.clone(),
    };
    if base.is_empty() && is_dir {
        let trimmed = unescaped.trim_end_matches('/');
        if let Some(idx) = trimmed.rfind('/') {
            base = trimmed[idx + 1..].to_string();
        } else {
            base = trimmed.to_string();
        }
    }
    if is_dir && !base.ends_with('/') {
        base.push('/');
    }
    base
}

fn unescape_display(input: &str) -> String {
    let mut out = String::new();
    let mut iter = input.chars();
    while let Some(ch) = iter.next() {
        if ch == '\\' {
            if let Some(next) = iter.next() {
                out.push(next);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn completion_anchor_x(app: &App, message_area: Rect) -> u16 {
    let cmd = app.command_buffer.as_str();
    if app.completion_anchor_fixed {
        if let Some(offset) = app.completion_anchor_col {
            return message_area.x + 1 + offset;
        }
    }
    let path_part = if cmd == "e" {
        ""
    } else if let Some(rest) = cmd.strip_prefix("e ") {
        rest
    } else if cmd == "edit" {
        ""
    } else if let Some(rest) = cmd.strip_prefix("edit ") {
        rest
    } else if cmd == "w" {
        ""
    } else if let Some(rest) = cmd.strip_prefix("w ") {
        rest
    } else if cmd == "write" {
        ""
    } else if let Some(rest) = cmd.strip_prefix("write ") {
        rest
    } else {
        cmd
    };
    let trimmed = path_part.trim_end_matches('/');
    let slash_col = trimmed.rfind('/').map(|idx| trimmed[..idx].chars().count());
    let prefix_len = cmd.chars().count().saturating_sub(path_part.chars().count());
    let offset = slash_col
        .map(|col| prefix_len + col + 1)
        .unwrap_or_else(|| prefix_len + trimmed.chars().count());
    message_area.x + 1 + offset as u16
}

fn render_line_with_selection(
    line: &str,
    line_idx: usize,
    start_col: usize,
    max_cols: usize,
    selection: Option<VisualSelection>,
    syntax_spans: Option<&[SyntaxSpan]>,
    search_pattern: Option<&str>,
    gutter_width: usize,
    is_current_line: bool,
    relative_number: bool,
    cursor_row: usize,
    app: &App,
) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();
    let mut col = 0usize;
    let mut screen_col = 0usize;
    let mut buf = String::new();
    let mut buf_state = 0u8;
    let mut buf_kind: Option<HighlightKind> = None;
    let mut syntax_idx = 0usize;
    let syntax = syntax_spans.unwrap_or(&[]);

    let search_matches = search_pattern
        .and_then(|pat| build_search_mask(line, pat))
        .unwrap_or_default();

    let block_range = match selection {
        Some(sel) => match sel.kind {
            VisualSelectionKind::Block { start, end } => {
                let base_line = app.lines.get(start.0).map(|s| s.as_str()).unwrap_or("");
                let start_sc = char_to_screen_col(base_line, start.1, app.shift_width);
                let end_sc = char_to_screen_col(base_line, end.1, app.shift_width);
                let end_char = base_line.chars().nth(end.1).unwrap_or(' ');
                let end_w = char_display_width(end_char, end_sc, app.shift_width);
                let (a, b) = if start_sc <= end_sc {
                    (start_sc, end_sc.saturating_add(end_w))
                } else {
                    (end_sc, start_sc.saturating_add(end_w))
                };
                Some((a, b))
            }
            _ => None,
        },
        None => None,
    };

    let number = if relative_number && line_idx != cursor_row {
        line_idx.abs_diff(cursor_row)
    } else {
        line_idx + 1
    };
    let line_label = format!("{:>width$} ", number, width = gutter_width - 1);
    spans.push(Span::styled(
        line_label,
        if is_current_line {
        Style::default().fg(app.theme.line_number_fg_current)
    } else {
        Style::default().fg(app.theme.line_number_fg)
    },
));

    let mut is_selected = |c: usize, sc: usize, w: usize| -> bool {
        let selection = match selection {
            Some(r) => r,
            None => return false,
        };
        match selection.kind {
            VisualSelectionKind::Char(sel_start, sel_end) => {
                let within_line = line_idx >= sel_start.0 && line_idx <= sel_end.0;
                if !within_line {
                    return false;
                }
                if sel_start.0 == sel_end.0 {
                    c >= sel_start.1 && c <= sel_end.1
                } else if line_idx == sel_start.0 {
                    c >= sel_start.1
                } else if line_idx == sel_end.0 {
                    c <= sel_end.1
                } else {
                    true
                }
            }
            VisualSelectionKind::Line(start_row, end_row) => {
                line_idx >= start_row && line_idx <= end_row
            }
            VisualSelectionKind::Block { start, end } => {
                let within_line = line_idx >= start.0 && line_idx <= end.0;
                if !within_line {
                    return false;
                }
                if let Some((block_start, block_end)) = block_range {
                    sc < block_end && sc.saturating_add(w) > block_start
                } else {
                    false
                }
            }
        }
    };

    for ch in line.chars() {
        while syntax_idx < syntax.len() && col >= syntax[syntax_idx].end_col {
            syntax_idx += 1;
        }
        let kind = if syntax_idx < syntax.len()
            && col >= syntax[syntax_idx].start_col
            && col < syntax[syntax_idx].end_col
        {
            Some(syntax[syntax_idx].kind)
        } else {
            None
        };
        let width = char_display_width(ch, screen_col, app.shift_width);
        if screen_col + width > start_col && screen_col < start_col + max_cols {
            let selected = is_selected(col, screen_col, width);
            let matched = search_matches.get(col).copied().unwrap_or(false);
            let state = if selected {
                3
            } else if matched {
                2
            } else if is_current_line {
                1
            } else {
                0
            };
            if buf.is_empty() {
                buf_state = state;
                buf_kind = kind;
                buf.push(ch);
            } else if state == buf_state && kind == buf_kind {
                buf.push(ch);
            } else {
                spans.push(Span::styled(
                    buf.clone(),
                    style_for_state(buf_state, buf_kind, app),
                ));
                buf.clear();
                buf_state = state;
                buf_kind = kind;
                buf.push(ch);
            }
        }
        col += 1;
        screen_col += width;
        if screen_col >= start_col + max_cols {
            break;
        }
    }

    if !buf.is_empty() {
        spans.push(Span::styled(buf, style_for_state(buf_state, buf_kind, app)));
    }

    if is_current_line {
        let line_len = line_screen_width(line, app.shift_width);
        let rendered = line_len.saturating_sub(start_col).min(max_cols);
        let pad = max_cols.saturating_sub(rendered);
        if pad > 0 {
            spans.push(Span::styled(" ".repeat(pad), style_for_state(1, None, app)));
        }
    }

    Line::from(spans)
}

fn style_for_state(state: u8, kind: Option<HighlightKind>, app: &App) -> Style {
    let syntax_fg = kind.map(|k| syntax_color(k, app));
    match state {
        3 => Style::default()
            .fg(app.theme.selection_fg)
            .bg(app.theme.selection_bg),
        2 => Style::default()
            .fg(app.theme.search_fg)
            .bg(app.theme.search_bg),
        1 => {
            let mut style = Style::default().bg(app.theme.current_line_bg);
            if let Some(fg) = syntax_fg {
                style = style.fg(fg);
            }
            style
        }
        _ => {
            let mut style = Style::default();
            if let Some(fg) = syntax_fg {
                style = style.fg(fg);
            }
            style
        }
    }
}

fn syntax_color(kind: HighlightKind, app: &App) -> Color {
    match kind {
        HighlightKind::Keyword => app.theme.syntax_keyword,
        HighlightKind::String => app.theme.syntax_string,
        HighlightKind::Comment => app.theme.syntax_comment,
        HighlightKind::Function => app.theme.syntax_function,
        HighlightKind::Type => app.theme.syntax_type,
        HighlightKind::Constant => app.theme.syntax_constant,
        HighlightKind::Number => app.theme.syntax_number,
        HighlightKind::Operator => app.theme.syntax_operator,
        HighlightKind::Property => app.theme.syntax_property,
        HighlightKind::Variable => app.theme.syntax_variable,
        HighlightKind::Macro => app.theme.syntax_macro,
        HighlightKind::Attribute => app.theme.syntax_attribute,
        HighlightKind::Punctuation => app.theme.syntax_punctuation,
    }
}

fn build_search_mask(line: &str, pattern: &str) -> Option<Vec<bool>> {
    if pattern.is_empty() {
        return None;
    }
    let chars: Vec<char> = line.chars().collect();
    let needle: Vec<char> = pattern.chars().collect();
    if needle.is_empty() || needle.len() > chars.len() {
        return None;
    }
    let mut mask = vec![false; chars.len()];
    let max_start = chars.len().saturating_sub(needle.len());
    for i in 0..=max_start {
        if chars[i..i + needle.len()] == *needle {
            for j in 0..needle.len() {
                mask[i + j] = true;
            }
        }
    }
    Some(mask)
}

fn render_empty_line(gutter_width: usize) -> Line<'static> {
    let gutter = " ".repeat(gutter_width);
    Line::from(format!("{}~", gutter))
}
