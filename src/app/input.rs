use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::edit::selection_to_last_visual;
use super::types::{
    FindPending, FindSpec, Mode, Operator, OperatorPending, TextObjectKind, TextObjectPending,
    TextObjectTarget, VisualSelectionKind,
};
use super::{App, VisualSelection};

pub fn handle_key(app: &mut App, key: KeyEvent) -> Result<bool> {
    let is_quit = matches!(key.code, KeyCode::Char('q')) && key.modifiers == KeyModifiers::CONTROL;
    if !is_quit {
        app.quit_confirm = false;
    }
    if app.pending_g
        && !matches!(key.code, KeyCode::Char('g') | KeyCode::Char('v'))
        && key.modifiers == KeyModifiers::NONE
    {
        app.pending_g = false;
    }
    if matches!(
        app.mode,
        Mode::Normal | Mode::VisualChar | Mode::VisualLine | Mode::VisualBlock
    ) {
        if let Some(pending) = app.pending_find.take() {
            if let KeyCode::Char(ch) = key.code {
                let found = if pending.reverse {
                    app.find_backward(ch, pending.until)
                } else {
                    app.find_forward(ch, pending.until)
                };
                if !found {
                    app.set_status(format!(
                        "Pattern not found: {}{}",
                        if pending.reverse { "F" } else { "f" },
                        ch
                    ));
                } else {
                    app.last_find = Some(FindSpec {
                        ch,
                        until: pending.until,
                        reverse: pending.reverse,
                    });
                }
            }
            if app.mode == Mode::Normal {
                if let Some(op) = app.operator_pending.take() {
                    app.apply_operator(
                        op.op,
                        (op.start_row, op.start_col),
                        (app.cursor_row, app.cursor_col),
                    );
                    if op.op == Operator::Change {
                        app.mode = Mode::Insert;
                        app.set_status("-- INSERT --");
                    }
                }
            }
            return Ok(false);
        }
    }

    if let Some(pending) = app.pending_textobj.take() {
        if let KeyCode::Char(ch) = key.code {
            let target = match ch {
                'w' => Some(TextObjectTarget::Word),
                '{' | '}' => Some(TextObjectTarget::Brace),
                '(' | ')' => Some(TextObjectTarget::Paren),
                '[' | ']' => Some(TextObjectTarget::Bracket),
                '<' | '>' => Some(TextObjectTarget::Angle),
                't' => Some(TextObjectTarget::Tag),
                '"' => Some(TextObjectTarget::QuoteDouble),
                '\'' => Some(TextObjectTarget::QuoteSingle),
                _ => None,
            };
            if let Some(target) = target {
                let range = match target {
                    TextObjectTarget::Word => {
                        if matches!(pending.kind, TextObjectKind::Around) {
                            app.textobj_word_range_around()
                        } else {
                            app.textobj_word_range()
                        }
                    }
                    TextObjectTarget::Brace => app.textobj_pair_range('{', '}', pending.kind),
                    TextObjectTarget::Paren => app.textobj_pair_range('(', ')', pending.kind),
                    TextObjectTarget::Bracket => app.textobj_pair_range('[', ']', pending.kind),
                    TextObjectTarget::Angle => app.textobj_pair_range('<', '>', pending.kind),
                    TextObjectTarget::Tag => app.textobj_tag_range(pending.kind),
                    TextObjectTarget::QuoteSingle => app.textobj_quote_range('\'', pending.kind),
                    TextObjectTarget::QuoteDouble => app.textobj_quote_range('"', pending.kind),
                };
                if let Some(((sr, sc), (er, ec))) = range {
                    if matches!(app.mode, Mode::VisualChar | Mode::VisualLine | Mode::VisualBlock) {
                        app.visual_start = Some((sr, sc));
                        app.cursor_row = er;
                        app.cursor_col = ec;
                    } else if let Some(op) = app.operator_pending.take() {
                        app.apply_operator(op.op, (sr, sc), (er, ec));
                        if op.op == Operator::Change {
                            app.mode = Mode::Insert;
                            app.insert_undo_snapshot = false;
                            app.set_status("-- INSERT --");
                        }
                    }
                } else {
                    app.set_status("No text object");
                }
            }
        }
        return Ok(false);
    }

    match app.mode {
        Mode::Normal => match (key.code, key.modifiers) {
            (KeyCode::Char('r'), KeyModifiers::CONTROL) => app.redo(),
            (KeyCode::Char('z'), KeyModifiers::CONTROL) => app.undo(),
            (KeyCode::Char('u'), KeyModifiers::NONE) => app.undo(),
            (KeyCode::Char('U'), _) => app.undo_line(),
            (KeyCode::Char('q'), KeyModifiers::CONTROL) => {
                if app.dirty && !app.quit_confirm {
                    app.quit_confirm = true;
                    app.set_status("Unsaved changes. Press Ctrl+Q again to quit.");
                    return Ok(false);
                }
                return Ok(true);
            }
            (KeyCode::Char('s'), KeyModifiers::CONTROL) => app.save()?,
            (KeyCode::Char('i'), KeyModifiers::NONE) => {
                if app.operator_pending.is_some() {
                    app.pending_textobj = Some(TextObjectPending {
                        kind: TextObjectKind::Inner,
                    });
                } else {
                    app.mode = Mode::Insert;
                    app.operator_pending = None;
                    app.insert_undo_snapshot = false;
                    app.set_status("-- INSERT --");
                }
            }
            (KeyCode::Char('a'), KeyModifiers::NONE) => {
                if app.operator_pending.is_some() {
                    app.pending_textobj = Some(TextObjectPending {
                        kind: TextObjectKind::Around,
                    });
                } else {
                    let len = app.line_len(app.cursor_row);
                    if app.cursor_col < len {
                        app.cursor_col += 1;
                    }
                    app.mode = Mode::Insert;
                    app.operator_pending = None;
                    app.insert_undo_snapshot = false;
                    app.set_status("-- INSERT --");
                }
            }
            (KeyCode::Char('I'), _) => {
                app.move_line_first_non_blank();
                app.mode = Mode::Insert;
                app.operator_pending = None;
                app.insert_undo_snapshot = false;
                app.set_status("-- INSERT --");
            }
            (KeyCode::Char('A'), _) => {
                app.move_line_end_insert();
                app.mode = Mode::Insert;
                app.operator_pending = None;
                app.insert_undo_snapshot = false;
                app.set_status("-- INSERT --");
            }
            (KeyCode::Char('v'), KeyModifiers::NONE) => {
                if app.pending_g {
                    app.pending_g = false;
                    if let Some(last) = app.last_visual {
                        app.mode = last.mode;
                        app.visual_start = Some(last.start);
                        app.cursor_row = last.end.0;
                        app.cursor_col = last.end.1;
                        let label = match app.mode {
                            Mode::VisualChar => "-- VISUAL --",
                            Mode::VisualLine => "-- VISUAL LINE --",
                            Mode::VisualBlock => "-- VISUAL BLOCK --",
                            _ => "-- VISUAL --",
                        };
                        app.set_status(label);
                    } else {
                        app.set_status("No previous visual selection");
                    }
                } else {
                    app.mode = Mode::VisualChar;
                    app.visual_start = Some((app.cursor_row, app.cursor_col));
                    app.operator_pending = None;
                    app.set_status("-- VISUAL --");
                }
            }
            (KeyCode::Char('V'), _) => {
                app.mode = Mode::VisualLine;
                app.visual_start = Some((app.cursor_row, app.cursor_col));
                app.operator_pending = None;
                app.set_status("-- VISUAL LINE --");
            }
            (KeyCode::Char('v'), KeyModifiers::CONTROL) => {
                app.mode = Mode::VisualBlock;
                app.visual_start = Some((app.cursor_row, app.cursor_col));
                app.operator_pending = None;
                app.set_status("-- VISUAL BLOCK --");
            }
            (KeyCode::Char(':'), KeyModifiers::NONE) => {
                app.mode = Mode::Command;
                app.command_buffer.clear();
                app.operator_pending = None;
            }
            (KeyCode::Char('d'), KeyModifiers::NONE) => {
                if let Some(op) = app.operator_pending.take() {
                    if op.op == Operator::Delete {
                        app.yank_line(app.cursor_row);
                        app.delete_line(app.cursor_row);
                        return Ok(false);
                    }
                }
                app.operator_pending = Some(OperatorPending {
                    op: Operator::Delete,
                    start_row: app.cursor_row,
                    start_col: app.cursor_col,
                });
            }
            (KeyCode::Char('y'), KeyModifiers::NONE) => {
                if let Some(op) = app.operator_pending.take() {
                    if op.op == Operator::Yank {
                        app.yank_line(app.cursor_row);
                        return Ok(false);
                    }
                }
                app.operator_pending = Some(OperatorPending {
                    op: Operator::Yank,
                    start_row: app.cursor_row,
                    start_col: app.cursor_col,
                });
            }
            (KeyCode::Char('c'), KeyModifiers::NONE) => {
                if let Some(op) = app.operator_pending.take() {
                    if op.op == Operator::Change {
                        app.yank_line(app.cursor_row);
                        app.delete_line(app.cursor_row);
                        app.mode = Mode::Insert;
                        app.insert_undo_snapshot = false;
                        app.set_status("-- INSERT --");
                        return Ok(false);
                    }
                }
                app.operator_pending = Some(OperatorPending {
                    op: Operator::Change,
                    start_row: app.cursor_row,
                    start_col: app.cursor_col,
                });
            }
            (KeyCode::Char('p'), KeyModifiers::NONE) => app.paste_after(),
            (KeyCode::Char('P'), _) => app.paste_before(),
            (KeyCode::Char('h'), KeyModifiers::NONE) | (KeyCode::Left, _) => app.move_left(),
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, _) => app.move_down(),
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, _) => app.move_up(),
            (KeyCode::Char('l'), KeyModifiers::NONE) | (KeyCode::Right, _) => app.move_right(),
            (KeyCode::Char('w'), KeyModifiers::NONE) => {
                if let Some(op) = app.operator_pending.take() {
                    app.move_word_forward();
                    let mut end = (app.cursor_row, app.cursor_col);
                    if let Some(prev) = app.prev_pos(end.0, end.1) {
                        end = prev;
                    }
                    app.apply_operator(op.op, (op.start_row, op.start_col), end);
                    if op.op == Operator::Change {
                        app.mode = Mode::Insert;
                        app.insert_undo_snapshot = false;
                        app.set_status("-- INSERT --");
                    }
                } else {
                    app.move_word_forward();
                }
            }
            (KeyCode::Char('b'), KeyModifiers::NONE) => app.move_word_back(),
            (KeyCode::Char('e'), KeyModifiers::NONE) => app.move_word_end(),
            (KeyCode::Char('W'), _) => {
                if let Some(op) = app.operator_pending.take() {
                    app.move_big_word_forward();
                    let mut end = (app.cursor_row, app.cursor_col);
                    if let Some(prev) = app.prev_pos(end.0, end.1) {
                        end = prev;
                    }
                    app.apply_operator(op.op, (op.start_row, op.start_col), end);
                    if op.op == Operator::Change {
                        app.mode = Mode::Insert;
                        app.insert_undo_snapshot = false;
                        app.set_status("-- INSERT --");
                    }
                } else {
                    app.move_big_word_forward();
                }
            }
            (KeyCode::Char('B'), _) => app.move_big_word_back(),
            (KeyCode::Char('E'), _) => app.move_big_word_end(),
            (KeyCode::Char('0'), KeyModifiers::NONE) => app.move_line_start(),
            (KeyCode::Char('$'), _) => app.move_line_end(),
            (KeyCode::Char('g'), KeyModifiers::NONE) => {
                if app.pending_g {
                    app.move_to_top();
                    app.pending_g = false;
                } else {
                    app.pending_g = true;
                }
            }
            (KeyCode::Char('G'), _) => app.move_to_bottom(),
            (KeyCode::Char('f'), KeyModifiers::NONE) => {
                app.pending_find = Some(FindPending {
                    until: false,
                    reverse: false,
                });
            }
            (KeyCode::Char('t'), KeyModifiers::NONE) => {
                app.pending_find = Some(FindPending {
                    until: true,
                    reverse: false,
                });
            }
            (KeyCode::Char('F'), _) => {
                app.pending_find = Some(FindPending {
                    until: false,
                    reverse: true,
                });
            }
            (KeyCode::Char('T'), _) => {
                app.pending_find = Some(FindPending {
                    until: true,
                    reverse: true,
                });
            }
            (KeyCode::Char(';'), KeyModifiers::NONE) => {
                if let Some(spec) = app.last_find {
                    let found = if spec.reverse {
                        app.find_backward(spec.ch, spec.until)
                    } else {
                        app.find_forward(spec.ch, spec.until)
                    };
                    if !found {
                        app.set_status(format!(
                            "Pattern not found: {}{}",
                            if spec.reverse { "F" } else { "f" },
                            spec.ch
                        ));
                    }
                } else {
                    app.set_status("No previous find");
                }
            }
            (KeyCode::Char(','), KeyModifiers::NONE) => {
                if let Some(spec) = app.last_find {
                    let found = if spec.reverse {
                        app.find_forward(spec.ch, spec.until)
                    } else {
                        app.find_backward(spec.ch, spec.until)
                    };
                    if !found {
                        app.set_status(format!(
                            "Pattern not found: {}{}",
                            if spec.reverse { "f" } else { "F" },
                            spec.ch
                        ));
                    }
                } else {
                    app.set_status("No previous find");
                }
            }
            (KeyCode::Char('x'), KeyModifiers::NONE) => app.delete_at_cursor(),
            (KeyCode::Char('o'), KeyModifiers::NONE) => {
                app.open_line_below();
                app.mode = Mode::Insert;
                app.insert_undo_snapshot = false;
            }
            (KeyCode::Char('O'), _) => {
                app.open_line_above();
                app.mode = Mode::Insert;
                app.insert_undo_snapshot = false;
            }
            _ => {}
        },
        Mode::Insert => match (key.code, key.modifiers) {
            (KeyCode::Char('q'), KeyModifiers::CONTROL) => {
                if app.dirty && !app.quit_confirm {
                    app.quit_confirm = true;
                    app.set_status("Unsaved changes. Press Ctrl+Q again to quit.");
                    return Ok(false);
                }
                return Ok(true);
            }
            (KeyCode::Char('r'), KeyModifiers::CONTROL) => app.redo(),
            (KeyCode::Char('z'), KeyModifiers::CONTROL) => app.undo(),
            (KeyCode::Esc, _) => {
                app.mode = Mode::Normal;
                app.block_insert = None;
                app.insert_undo_snapshot = false;
                app.set_status("-- NORMAL --");
            }
            (KeyCode::Char('s'), KeyModifiers::CONTROL) => app.save()?,
            (KeyCode::Enter, _) => {
                app.insert_undo_snapshot = false;
                app.insert_newline()
            }
            (KeyCode::Backspace, _) => {
                app.insert_undo_snapshot = false;
                app.backspace()
            }
            (KeyCode::Delete, _) => {
                app.insert_undo_snapshot = false;
                app.delete_at_cursor()
            }
            (KeyCode::Tab, _) => {
                app.insert_undo_snapshot = false;
                for _ in 0..4 {
                    app.insert_char(' ');
                }
            }
            (KeyCode::Char(ch), KeyModifiers::NONE) => {
                if super::types::is_undo_break_char(ch) {
                    app.insert_undo_snapshot = false;
                }
                app.insert_char(ch)
            }
            (KeyCode::Char(ch), KeyModifiers::SHIFT) => {
                if super::types::is_undo_break_char(ch) {
                    app.insert_undo_snapshot = false;
                }
                app.insert_char(ch)
            }
            (KeyCode::Left, _) => {
                app.insert_undo_snapshot = false;
                app.move_left()
            }
            (KeyCode::Right, _) => {
                app.insert_undo_snapshot = false;
                app.move_right()
            }
            (KeyCode::Up, _) => {
                app.insert_undo_snapshot = false;
                app.move_up()
            }
            (KeyCode::Down, _) => {
                app.insert_undo_snapshot = false;
                app.move_down()
            }
            _ => {}
        },
        Mode::Command => match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                app.mode = Mode::Normal;
                app.command_buffer.clear();
            }
            (KeyCode::Enter, _) => {
                let should_quit = app.execute_command()?;
                app.command_buffer.clear();
                app.mode = Mode::Normal;
                if should_quit {
                    return Ok(true);
                }
            }
            (KeyCode::Backspace, _) => {
                app.command_buffer.pop();
            }
            (KeyCode::Char(ch), KeyModifiers::NONE) => {
                app.command_buffer.push(ch);
            }
            (KeyCode::Char(ch), KeyModifiers::SHIFT) => {
                app.command_buffer.push(ch);
            }
            _ => {}
        },
        Mode::VisualChar | Mode::VisualLine | Mode::VisualBlock => match (key.code, key.modifiers) {
            (KeyCode::Esc, _)
            | (KeyCode::Char('v'), KeyModifiers::NONE)
            | (KeyCode::Char('V'), _) => {
                if let Some(selection) = app.visual_selection() {
                    app.last_visual = Some(selection_to_last_visual(selection, app.mode));
                }
                app.mode = Mode::Normal;
                app.visual_start = None;
                app.set_status("-- NORMAL --");
            }
            (KeyCode::Char('y'), KeyModifiers::NONE) => {
                if let Some(selection) = app.visual_selection() {
                    match selection.kind {
                        VisualSelectionKind::Char(start, end) => app.yank_range(start, end),
                        VisualSelectionKind::Line(start, end) => app.yank_lines(start, end),
                        VisualSelectionKind::Block { start, end } => app.yank_block(start, end),
                    }
                    app.last_visual = Some(selection_to_last_visual(selection, app.mode));
                }
                app.mode = Mode::Normal;
                app.visual_start = None;
            }
            (KeyCode::Char('d'), KeyModifiers::NONE) => {
                if let Some(selection) = app.visual_selection() {
                    match selection.kind {
                        VisualSelectionKind::Char(start, end) => {
                            app.yank_range(start, end);
                            app.delete_range(start, end);
                        }
                        VisualSelectionKind::Line(start, end) => {
                            app.yank_lines(start, end);
                            app.delete_lines(start, end);
                        }
                        VisualSelectionKind::Block { start, end } => {
                            app.yank_block(start, end);
                            app.delete_block(start, end);
                        }
                    }
                    app.last_visual = Some(selection_to_last_visual(selection, app.mode));
                }
                app.mode = Mode::Normal;
                app.visual_start = None;
            }
            (KeyCode::Char('c'), KeyModifiers::NONE) => {
                if let Some(selection) = app.visual_selection() {
                    match selection.kind {
                        VisualSelectionKind::Char(start, end) => {
                            app.yank_range(start, end);
                            app.delete_range(start, end);
                        }
                        VisualSelectionKind::Line(start, end) => {
                            app.yank_lines(start, end);
                            app.delete_lines(start, end);
                        }
                        VisualSelectionKind::Block { start, end } => {
                            app.yank_block(start, end);
                            app.delete_block(start, end);
                        }
                    }
                    app.last_visual = Some(selection_to_last_visual(selection, app.mode));
                }
                app.mode = Mode::Insert;
                app.insert_undo_snapshot = false;
                app.visual_start = None;
                app.set_status("-- INSERT --");
            }
            (KeyCode::Char('i'), KeyModifiers::NONE) => {
                app.pending_textobj = Some(TextObjectPending {
                    kind: TextObjectKind::Inner,
                });
            }
            (KeyCode::Char('a'), KeyModifiers::NONE) => {
                app.pending_textobj = Some(TextObjectPending {
                    kind: TextObjectKind::Around,
                });
            }
            (KeyCode::Char('p'), KeyModifiers::NONE) | (KeyCode::Char('P'), _) => {
                let selection = app.visual_selection();
                let start = if let Some(selection) = selection {
                    match selection.kind {
                        VisualSelectionKind::Char(start, end) => {
                            app.delete_range(start, end);
                            start
                        }
                        VisualSelectionKind::Line(start, end) => {
                            app.delete_lines(start, end);
                            (start, 0)
                        }
                        VisualSelectionKind::Block { start, end } => {
                            app.delete_block(start, end);
                            start
                        }
                    }
                } else {
                    (app.cursor_row, app.cursor_col)
                };
                app.cursor_row = start.0;
                app.cursor_col = start.1;
                app.paste_before();
                if let Some(selection) = selection {
                    app.last_visual = Some(selection_to_last_visual(selection, app.mode));
                }
                app.mode = Mode::Normal;
                app.visual_start = None;
            }
            (KeyCode::Char('I'), _) => {
                if matches!(app.mode, Mode::VisualBlock) {
                    if let Some(selection) = app.visual_selection() {
                        if let VisualSelectionKind::Block { start, end } = selection.kind {
                            app.block_insert = Some(super::types::BlockInsert {
                                start_row: start.0,
                                end_row: end.0,
                                col: start.1,
                                append: false,
                            });
                            app.cursor_row = start.0;
                            app.cursor_col = start.1;
                            app.mode = Mode::Insert;
                            app.insert_undo_snapshot = false;
                            app.visual_start = None;
                            app.set_status("-- INSERT --");
                        }
                    }
                }
            }
            (KeyCode::Char('A'), _) => {
                if matches!(app.mode, Mode::VisualBlock) {
                    if let Some(selection) = app.visual_selection() {
                        if let VisualSelectionKind::Block { start, end } = selection.kind {
                            app.block_insert = Some(super::types::BlockInsert {
                                start_row: start.0,
                                end_row: end.0,
                                col: end.1 + 1,
                                append: false,
                            });
                            app.cursor_row = start.0;
                            app.cursor_col = end.1 + 1;
                            app.mode = Mode::Insert;
                            app.insert_undo_snapshot = false;
                            app.visual_start = None;
                            app.set_status("-- INSERT --");
                        }
                    }
                }
            }
            (KeyCode::Char('h'), KeyModifiers::NONE) | (KeyCode::Left, _) => app.move_left(),
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, _) => app.move_down(),
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, _) => app.move_up(),
            (KeyCode::Char('l'), KeyModifiers::NONE) | (KeyCode::Right, _) => app.move_right(),
            (KeyCode::Char('w'), KeyModifiers::NONE) => app.move_word_forward(),
            (KeyCode::Char('b'), KeyModifiers::NONE) => app.move_word_back(),
            (KeyCode::Char('e'), KeyModifiers::NONE) => app.move_word_end(),
            (KeyCode::Char('W'), _) => app.move_big_word_forward(),
            (KeyCode::Char('B'), _) => app.move_big_word_back(),
            (KeyCode::Char('E'), _) => app.move_big_word_end(),
            (KeyCode::Char('0'), KeyModifiers::NONE) => app.move_line_start(),
            (KeyCode::Char('$'), _) => app.move_line_end(),
            (KeyCode::Char('g'), KeyModifiers::NONE) => {
                if app.pending_g {
                    app.move_to_top();
                    app.pending_g = false;
                } else {
                    app.pending_g = true;
                }
            }
            (KeyCode::Char('G'), _) => app.move_to_bottom(),
            (KeyCode::Char('f'), KeyModifiers::NONE) => {
                app.pending_find = Some(FindPending {
                    until: false,
                    reverse: false,
                });
            }
            (KeyCode::Char('t'), KeyModifiers::NONE) => {
                app.pending_find = Some(FindPending {
                    until: true,
                    reverse: false,
                });
            }
            (KeyCode::Char('F'), _) => {
                app.pending_find = Some(FindPending {
                    until: false,
                    reverse: true,
                });
            }
            (KeyCode::Char('T'), _) => {
                app.pending_find = Some(FindPending {
                    until: true,
                    reverse: true,
                });
            }
            (KeyCode::Char(';'), KeyModifiers::NONE) => {
                if let Some(spec) = app.last_find {
                    let found = if spec.reverse {
                        app.find_backward(spec.ch, spec.until)
                    } else {
                        app.find_forward(spec.ch, spec.until)
                    };
                    if !found {
                        app.set_status(format!(
                            "Pattern not found: {}{}",
                            if spec.reverse { "F" } else { "f" },
                            spec.ch
                        ));
                    }
                } else {
                    app.set_status("No previous find");
                }
            }
            (KeyCode::Char(','), KeyModifiers::NONE) => {
                if let Some(spec) = app.last_find {
                    let found = if spec.reverse {
                        app.find_forward(spec.ch, spec.until)
                    } else {
                        app.find_backward(spec.ch, spec.until)
                    };
                    if !found {
                        app.set_status(format!(
                            "Pattern not found: {}{}",
                            if spec.reverse { "f" } else { "F" },
                            spec.ch
                        ));
                    }
                } else {
                    app.set_status("No previous find");
                }
            }
            _ => {}
        },
    }

    if app.mode == Mode::Normal {
        if let Some(op) = app.operator_pending.take() {
            let end = (app.cursor_row, app.cursor_col);
            if (op.start_row, op.start_col) != end {
                app.apply_operator(op.op, (op.start_row, op.start_col), end);
                if op.op == Operator::Change {
                    app.mode = Mode::Insert;
                    app.insert_undo_snapshot = false;
                    app.set_status("-- INSERT --");
                }
            } else {
                app.operator_pending = Some(op);
            }
        }
    }

    Ok(false)
}
