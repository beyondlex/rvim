use std::io;

use anyhow::Result;
use crossterm::cursor::SetCursorStyle;
use crossterm::execute;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, CommandPrompt, Mode, VisualSelection, VisualSelectionKind};

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
    for i in 0..viewport_rows {
        let idx = app.scroll_row + i;
        if let Some(line) = app.lines.get(idx) {
            text_lines.push(render_line_with_selection(
                line,
                idx,
                app.scroll_col,
                viewport_cols,
                selection,
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

    if app.mode == Mode::Command {
        let cursor_x = message_area.x + 1 + app.command_buffer.chars().count() as u16;
        let cursor_y = message_area.y;
        if cursor_x < message_area.right() && cursor_y < message_area.bottom() {
            f.set_cursor_position(Position::new(cursor_x, cursor_y));
        }
    } else {
        let cursor_x = (app.cursor_col.saturating_sub(app.scroll_col)) as u16
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

fn render_line_with_selection(
    line: &str,
    line_idx: usize,
    start_col: usize,
    max_cols: usize,
    selection: Option<VisualSelection>,
    search_pattern: Option<&str>,
    gutter_width: usize,
    is_current_line: bool,
    relative_number: bool,
    cursor_row: usize,
    app: &App,
) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();
    let mut col = 0;
    let mut buf = String::new();
    let mut buf_state = 0u8;

    let search_matches = search_pattern
        .and_then(|pat| build_search_mask(line, pat))
        .unwrap_or_default();

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

    let mut is_selected = |c: usize| -> bool {
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
                within_line && c >= start.1 && c <= end.1
            }
        }
    };

    for ch in line.chars() {
        if col >= start_col && (col - start_col) < max_cols {
            let selected = is_selected(col);
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
                buf.push(ch);
            } else if state == buf_state {
                buf.push(ch);
            } else {
                spans.push(Span::styled(buf.clone(), style_for_state(buf_state, app)));
                buf.clear();
                buf_state = state;
                buf.push(ch);
            }
        }
        col += 1;
        if (col.saturating_sub(start_col)) >= max_cols {
            break;
        }
    }

    if !buf.is_empty() {
        spans.push(Span::styled(buf, style_for_state(buf_state, app)));
    }

    if is_current_line {
        let line_len = line.chars().count();
        let rendered = line_len.saturating_sub(start_col).min(max_cols);
        let pad = max_cols.saturating_sub(rendered);
        if pad > 0 {
            spans.push(Span::styled(" ".repeat(pad), style_for_state(1, app)));
        }
    }

    Line::from(spans)
}

fn style_for_state(state: u8, app: &App) -> Style {
    match state {
        3 => Style::default()
            .fg(app.theme.selection_fg)
            .bg(app.theme.selection_bg),
        2 => Style::default()
            .fg(app.theme.search_fg)
            .bg(app.theme.search_bg),
        1 => Style::default().bg(app.theme.current_line_bg),
        _ => Style::default(),
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
