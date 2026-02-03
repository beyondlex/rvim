use std::io;

use anyhow::Result;
use crossterm::cursor::SetCursorStyle;
use crossterm::execute;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, Mode, VisualSelection};

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
    let viewport_cols = main_area.width as usize;
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
            ));
        } else {
            text_lines.push(Line::from("~"));
        }
    }

    let paragraph = Paragraph::new(text_lines).block(Block::default().borders(Borders::NONE));
    f.render_widget(paragraph, main_area);

    let mode_label = match app.mode {
        Mode::Normal => "NORMAL",
        Mode::Insert => "INSERT",
        Mode::Command => "COMMAND",
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
        "{} | {}{} | {}:{}",
        mode_label,
        file_label,
        dirty,
        app.cursor_row + 1,
        app.cursor_col + 1
    );
    status.push_str(&format!(
        " | undo:{} redo:{}",
        app.undo_stack.len(),
        app.redo_stack.len()
    ));
    if matches!(app.mode, Mode::VisualChar | Mode::VisualLine | Mode::VisualBlock) {
        if let Some(summary) = app.selection_summary() {
            status.push_str(" | ");
            status.push_str(&summary);
        }
    }

    let status_paragraph = Paragraph::new(status)
        .style(Style::default().fg(Color::Black).bg(Color::White));
    f.render_widget(status_paragraph, status_area);

    let message = if app.mode == Mode::Command {
        Paragraph::new(format!(":{}", app.command_buffer))
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
        let cursor_x = (app.cursor_col.saturating_sub(app.scroll_col)) as u16 + main_area.x;
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
) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();
    let mut col = 0;
    let mut buf = String::new();
    let mut buf_selected = false;

    let selection = match selection {
        Some(r) => r,
        None => return Line::from(slice_line(line, start_col, max_cols)),
    };

    let mut is_selected = |c: usize| -> bool {
        match selection {
            VisualSelection::Char((sel_start, sel_end)) => {
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
            VisualSelection::Line(start_row, end_row) => line_idx >= start_row && line_idx <= end_row,
            VisualSelection::Block { start, end } => {
                let within_line = line_idx >= start.0 && line_idx <= end.0;
                within_line && c >= start.1 && c <= end.1
            }
        }
    };

    for ch in line.chars() {
        if col >= start_col && (col - start_col) < max_cols {
            let selected = is_selected(col);
            if buf.is_empty() {
                buf_selected = selected;
                buf.push(ch);
            } else if selected == buf_selected {
                buf.push(ch);
            } else {
                spans.push(Span::styled(
                    buf.clone(),
                    if buf_selected {
                        Style::default().fg(Color::Black).bg(Color::Cyan)
                    } else {
                        Style::default()
                    },
                ));
                buf.clear();
                buf_selected = selected;
                buf.push(ch);
            }
        }
        col += 1;
        if (col.saturating_sub(start_col)) >= max_cols {
            break;
        }
    }

    if !buf.is_empty() {
        spans.push(Span::styled(
            buf,
            if buf_selected {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default()
            },
        ));
    }

    Line::from(spans)
}
