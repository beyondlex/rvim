use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::fs;

use super::edit::selection_to_last_visual;
use super::keymap::KeyAction;
use super::types::{
    char_to_byte_idx, CommandPrompt, FindPending, FindSpec, Mode, Operator, OperatorPending,
    RepeatKey, TextObjectKind, TextObjectPending, TextObjectTarget, VisualSelectionKind,
};
use super::App;

pub fn handle_key(app: &mut App, key: KeyEvent) -> Result<bool> {
    app.log_key_event(&format!(
        "mode={:?} code={:?} mods={:?}",
        app.mode, key.code, key.modifiers
    ));
    let pre_tick = app.change_tick;
    if !app.repeat_replaying && !app.repeat_recording && should_start_repeat(app, &key) {
        app.repeat_recording = true;
        app.repeat_changed = false;
        app.repeat_buffer.clear();
    }
    let skip_record = matches!(app.mode, Mode::Normal)
        && key.code == KeyCode::Char('.')
        && key.modifiers == KeyModifiers::NONE;
    if app.repeat_recording && !app.repeat_replaying && !skip_record {
        app.repeat_buffer.push(RepeatKey {
            code: key.code,
            modifiers: key.modifiers,
        });
    }

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
    if app.pending_bracket.is_some()
        && !(key.code == KeyCode::Char('b') && key.modifiers == KeyModifiers::NONE)
    {
        app.pending_bracket = None;
    }

    if let Some(action) = app.keymaps.action_for(app.mode, &key) {
        if let Some(should_quit) = apply_keymap_action(app, action)? {
            return Ok(should_quit);
        }
    }

    if app.mode == Mode::Normal
        && key.modifiers == KeyModifiers::NONE
        && matches!(key.code, KeyCode::Char(ch) if ch.is_ascii_digit())
    {
        if let KeyCode::Char(ch) = key.code {
            let digit = ch.to_digit(10).unwrap_or(0) as usize;
            if app.pending_count.is_some() || digit != 0 {
                let next = app.pending_count.unwrap_or(0) * 10 + digit;
                app.pending_count = Some(next);
                return Ok(false);
            }
        }
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
            finalize_repeat(app, pre_tick);
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
        finalize_repeat(app, pre_tick);
        return Ok(false);
    }

    match app.mode {
        Mode::Normal => match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                app.operator_pending = None;
                app.pending_textobj = None;
                app.pending_find = None;
                app.pending_g = false;
                app.pending_bracket = None;
                app.last_search = None;
                app.pending_count = None;
            }
            (KeyCode::Char(']'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                app.pending_bracket = Some(']');
            }
            (KeyCode::Char('['), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                app.pending_bracket = Some('[');
            }
            (KeyCode::Char('.'), KeyModifiers::NONE) => {
                replay_last_change(app)?;
                return Ok(false);
            }
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
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                app.mode = Mode::Command;
                app.command_prompt = CommandPrompt::SearchForward;
                app.command_buffer.clear();
                app.command_cursor = 0;
                app.search_history_index = None;
                app.operator_pending = None;
            }
            (KeyCode::Char('?'), KeyModifiers::NONE) => {
                app.mode = Mode::Command;
                app.command_prompt = CommandPrompt::SearchBackward;
                app.command_buffer.clear();
                app.command_cursor = 0;
                app.search_history_index = None;
                app.operator_pending = None;
            }
            (KeyCode::Char(':'), KeyModifiers::NONE) => {
                app.mode = Mode::Command;
                app.command_prompt = CommandPrompt::Command;
                app.command_buffer.clear();
                app.command_cursor = 0;
                app.search_history_index = None;
                app.operator_pending = None;
            }
            (KeyCode::Char('d'), KeyModifiers::NONE) => {
                let mut handled = false;
                if let Some(op) = app.operator_pending.take() {
                    if op.op == Operator::Delete {
                        app.yank_line(app.cursor_row);
                        app.delete_line(app.cursor_row);
                        app.operator_pending = None;
                        handled = true;
                    }
                }
                if !handled {
                    app.operator_pending = Some(OperatorPending {
                        op: Operator::Delete,
                        start_row: app.cursor_row,
                        start_col: app.cursor_col,
                    });
                }
            }
            (KeyCode::Char('y'), KeyModifiers::NONE) => {
                let mut handled = false;
                if let Some(op) = app.operator_pending.take() {
                    if op.op == Operator::Yank {
                        app.yank_line(app.cursor_row);
                        app.operator_pending = None;
                        handled = true;
                    }
                }
                if !handled {
                    app.operator_pending = Some(OperatorPending {
                        op: Operator::Yank,
                        start_row: app.cursor_row,
                        start_col: app.cursor_col,
                    });
                }
            }
            (KeyCode::Char('c'), KeyModifiers::NONE) => {
                let mut handled = false;
                if let Some(op) = app.operator_pending.take() {
                    if op.op == Operator::Change {
                        app.yank_line(app.cursor_row);
                        app.delete_line(app.cursor_row);
                        app.mode = Mode::Insert;
                        app.insert_undo_snapshot = false;
                        app.set_status("-- INSERT --");
                        app.operator_pending = None;
                        handled = true;
                    }
                }
                if !handled {
                    app.operator_pending = Some(OperatorPending {
                        op: Operator::Change,
                        start_row: app.cursor_row,
                        start_col: app.cursor_col,
                    });
                }
            }
            (KeyCode::Char('p'), KeyModifiers::NONE) => app.paste_after(),
            (KeyCode::Char('P'), _) => app.paste_before(),
            (KeyCode::Char('h'), KeyModifiers::NONE) | (KeyCode::Left, _) => {
                let count = app.pending_count.take().unwrap_or(1);
                for _ in 0..count {
                    app.move_left();
                }
            }
            (KeyCode::Char('l'), KeyModifiers::NONE) | (KeyCode::Right, _) => {
                let count = app.pending_count.take().unwrap_or(1);
                for _ in 0..count {
                    app.move_right();
                }
            }
            (KeyCode::Char('w'), KeyModifiers::NONE) => {
                let count = app.pending_count.take().unwrap_or(1);
                if let Some(op) = app.operator_pending.take() {
                    for _ in 0..count {
                        app.move_word_forward();
                    }
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
                    for _ in 0..count {
                        app.move_word_forward();
                    }
                }
            }
            (KeyCode::Char('b'), KeyModifiers::NONE) => {
                if app.pending_bracket == Some(']') {
                    app.pending_bracket = None;
                    app.switch_next_buffer();
                } else if app.pending_bracket == Some('[') {
                    app.pending_bracket = None;
                    app.switch_prev_buffer();
                } else {
                    let count = app.pending_count.take().unwrap_or(1);
                    for _ in 0..count {
                        app.move_word_back();
                    }
                }
            }
            (KeyCode::Char('e'), KeyModifiers::NONE) => {
                let count = app.pending_count.take().unwrap_or(1);
                for _ in 0..count {
                    app.move_word_end();
                }
            }
            (KeyCode::Char('W'), _) => {
                let count = app.pending_count.take().unwrap_or(1);
                if let Some(op) = app.operator_pending.take() {
                    for _ in 0..count {
                        app.move_big_word_forward();
                    }
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
                    for _ in 0..count {
                        app.move_big_word_forward();
                    }
                }
            }
            (KeyCode::Char('B'), _) => {
                let count = app.pending_count.take().unwrap_or(1);
                for _ in 0..count {
                    app.move_big_word_back();
                }
            }
            (KeyCode::Char('E'), _) => {
                let count = app.pending_count.take().unwrap_or(1);
                for _ in 0..count {
                    app.move_big_word_end();
                }
            }
            (KeyCode::Char('0'), KeyModifiers::NONE) => app.move_line_start(),
            (KeyCode::Char('$'), _) => app.move_line_end(),
            (KeyCode::Char('%'), KeyModifiers::NONE) => {
                if !app.percent_jump() {
                    app.set_status("No matching bracket");
                }
            }
            (KeyCode::Char('g'), KeyModifiers::NONE) => {
                if app.pending_g {
                    if let Some(count) = app.pending_count.take() {
                        app.move_to_line(count);
                    } else {
                        app.move_to_top();
                    }
                    app.pending_g = false;
                } else {
                    app.pending_g = true;
                }
            }
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, _) => {
                let count = app.pending_count.take().unwrap_or(1);
                for _ in 0..count {
                    app.move_down();
                }
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, _) => {
                let count = app.pending_count.take().unwrap_or(1);
                for _ in 0..count {
                    app.move_up();
                }
            }
            (KeyCode::Char('G'), _) => {
                if let Some(count) = app.pending_count.take() {
                    app.move_to_line(count);
                } else {
                    app.move_to_bottom();
                }
            }
            (KeyCode::Char('n'), KeyModifiers::NONE) => {
                if let Some(spec) = app.last_search.clone() {
                    let found = if spec.reverse {
                        app.search_backward(&spec.pattern)
                    } else {
                        app.search_forward(&spec.pattern)
                    };
                    if !found {
                        app.set_status(format!(
                            "Pattern not found: {}{}",
                            if spec.reverse { "?" } else { "/" },
                            spec.pattern
                        ));
                    }
                } else {
                    app.set_status("No previous search");
                }
            }
            (KeyCode::Char('N'), _) => {
                if let Some(spec) = app.last_search.clone() {
                    let found = if spec.reverse {
                        app.search_forward(&spec.pattern)
                    } else {
                        app.search_backward(&spec.pattern)
                    };
                    if !found {
                        app.set_status(format!(
                            "Pattern not found: {}{}",
                            if spec.reverse { "/" } else { "?" },
                            spec.pattern
                        ));
                    }
                } else {
                    app.set_status("No previous search");
                }
            }
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
            (KeyCode::Char('~'), KeyModifiers::NONE) => {
                let row = app.cursor_row;
                let col = app.cursor_col;
                if app.line_len(row) > 0 && col < app.line_len(row) {
                    app.toggle_case_range((row, col), (row, col));
                    app.move_right();
                }
            }
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
                app.command_cursor = 0;
                app.command_prompt = CommandPrompt::Command;
                app.search_history_index = None;
                app.command_history_index = None;
                app.clear_completion();
            }
            (KeyCode::Enter, _) => {
                let should_quit = if matches!(app.command_prompt, CommandPrompt::Command) {
                    app.execute_command()?
                } else {
                    app.execute_search()?
                };
                app.command_buffer.clear();
                app.command_cursor = 0;
                app.mode = Mode::Normal;
                app.command_prompt = CommandPrompt::Command;
                app.search_history_index = None;
                app.command_history_index = None;
                app.clear_completion();
                if should_quit {
                    return Ok(true);
                }
            }
            (KeyCode::Backspace, mods) if mods.contains(KeyModifiers::SUPER) => {
                command_delete_to_line_start(app);
                app.search_history_index = None;
                app.command_history_index = None;
                app.clear_completion();
            }
            (KeyCode::Backspace, mods)
                if mods.contains(KeyModifiers::ALT) || mods.contains(KeyModifiers::CONTROL) =>
            {
                command_delete_prev_word(app);
                app.search_history_index = None;
                app.command_history_index = None;
                app.clear_completion();
            }
            (KeyCode::Backspace, _) => {
                command_backspace(app);
                app.search_history_index = None;
                app.command_history_index = None;
                app.clear_completion();
            }
            (KeyCode::Tab, _) => {
                if complete_path_in_command(app, false)
                    || complete_set_in_command(app, false)
                    || complete_command_in_command(app, false)
                {
                    app.search_history_index = None;
                }
            }
            (KeyCode::BackTab, _) => {
                if complete_path_in_command(app, true)
                    || complete_set_in_command(app, true)
                    || complete_command_in_command(app, true)
                {
                    app.search_history_index = None;
                }
            }
            (KeyCode::Up, _) => {
                if matches!(app.command_prompt, CommandPrompt::Command) {
                    if app.command_history.is_empty() {
                        app.set_status("No command history");
                    } else {
                        let next_idx = match app.command_history_index {
                            None => app.command_history.len() - 1,
                            Some(idx) => idx.saturating_sub(1),
                        };
                        app.command_history_index = Some(next_idx);
                        app.command_buffer = app.command_history[next_idx].clone();
                        command_cursor_to_end(app);
                        app.search_history_index = None;
                        app.clear_completion();
                    }
                } else if matches!(
                    app.command_prompt,
                    CommandPrompt::SearchForward | CommandPrompt::SearchBackward
                ) {
                    if app.search_history.is_empty() {
                        app.set_status("No search history");
                    } else {
                        let next_idx = match app.search_history_index {
                            None => app.search_history.len() - 1,
                            Some(idx) => idx.saturating_sub(1),
                        };
                        app.search_history_index = Some(next_idx);
                        app.command_buffer = app.search_history[next_idx].clone();
                        command_cursor_to_end(app);
                        app.command_history_index = None;
                        app.clear_completion();
                    }
                }
            }
            (KeyCode::Down, _) => {
                if matches!(app.command_prompt, CommandPrompt::Command) {
                    if let Some(idx) = app.command_history_index {
                        if idx + 1 < app.command_history.len() {
                            let next_idx = idx + 1;
                            app.command_history_index = Some(next_idx);
                            app.command_buffer = app.command_history[next_idx].clone();
                            command_cursor_to_end(app);
                            app.search_history_index = None;
                            app.clear_completion();
                        } else {
                            app.command_history_index = None;
                            app.command_buffer.clear();
                            app.command_cursor = 0;
                            app.clear_completion();
                        }
                    }
                } else if matches!(
                    app.command_prompt,
                    CommandPrompt::SearchForward | CommandPrompt::SearchBackward
                ) {
                    if let Some(idx) = app.search_history_index {
                        if idx + 1 < app.search_history.len() {
                            let next_idx = idx + 1;
                            app.search_history_index = Some(next_idx);
                            app.command_buffer = app.search_history[next_idx].clone();
                            command_cursor_to_end(app);
                            app.command_history_index = None;
                            app.clear_completion();
                        } else {
                            app.search_history_index = None;
                            app.command_buffer.clear();
                            app.command_cursor = 0;
                            app.clear_completion();
                        }
                    }
                }
            }
            (KeyCode::Left, mods) if mods.contains(KeyModifiers::SUPER) => {
                command_move_line_start(app);
            }
            (KeyCode::Right, mods) if mods.contains(KeyModifiers::SUPER) => {
                command_move_line_end(app);
            }
            (KeyCode::Left, mods)
                if mods.contains(KeyModifiers::ALT) || mods.contains(KeyModifiers::CONTROL) =>
            {
                command_move_word_left(app);
            }
            (KeyCode::Right, mods)
                if mods.contains(KeyModifiers::ALT) || mods.contains(KeyModifiers::CONTROL) =>
            {
                command_move_word_right(app);
            }
            (KeyCode::Left, _) => {
                command_move_left(app);
            }
            (KeyCode::Right, _) => {
                command_move_right(app);
            }
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                if enter_completion_directory(app) {
                    app.search_history_index = None;
                    app.command_history_index = None;
                } else {
                    command_insert_char(app, '/');
                    app.search_history_index = None;
                    app.command_history_index = None;
                    app.clear_completion();
                }
            }
            (KeyCode::Char(ch), KeyModifiers::NONE) => {
                command_insert_char(app, ch);
                app.search_history_index = None;
                app.command_history_index = None;
                app.clear_completion();
            }
            (KeyCode::Char(ch), KeyModifiers::SHIFT) => {
                command_insert_char(app, ch);
                app.search_history_index = None;
                app.command_history_index = None;
                app.clear_completion();
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
            (KeyCode::Char('u'), KeyModifiers::NONE) => {
                if let Some(selection) = app.visual_selection() {
                    match selection.kind {
                        VisualSelectionKind::Char(start, end) => {
                            app.change_case_range(start, end, false);
                        }
                        VisualSelectionKind::Line(start, end) => {
                            app.change_case_lines(start, end, false);
                        }
                        VisualSelectionKind::Block { start, end } => {
                            app.change_case_block(start, end, false);
                        }
                    }
                }
                app.mode = Mode::Normal;
                app.visual_start = None;
            }
            (KeyCode::Char('U'), _) => {
                if let Some(selection) = app.visual_selection() {
                    match selection.kind {
                        VisualSelectionKind::Char(start, end) => {
                            app.change_case_range(start, end, true);
                        }
                        VisualSelectionKind::Line(start, end) => {
                            app.change_case_lines(start, end, true);
                        }
                        VisualSelectionKind::Block { start, end } => {
                            app.change_case_block(start, end, true);
                        }
                    }
                }
                app.mode = Mode::Normal;
                app.visual_start = None;
            }
            (KeyCode::Char('~'), KeyModifiers::NONE) => {
                if let Some(selection) = app.visual_selection() {
                    match selection.kind {
                        VisualSelectionKind::Char(start, end) => {
                            app.toggle_case_range(start, end);
                        }
                        VisualSelectionKind::Line(start, end) => {
                            app.toggle_case_lines(start, end);
                        }
                        VisualSelectionKind::Block { start, end } => {
                            app.toggle_case_block(start, end);
                        }
                    }
                }
                app.mode = Mode::Normal;
                app.visual_start = None;
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
            (KeyCode::Char('b'), KeyModifiers::NONE) => {
                app.log_key_event(&format!("pending_bracket check {:?}", app.pending_bracket));
                if app.pending_bracket == Some(']') {
                    app.pending_bracket = None;
                    app.log_key_event("buffer_next");
                    app.switch_next_buffer();
                } else if app.pending_bracket == Some('[') {
                    app.pending_bracket = None;
                    app.log_key_event("buffer_prev");
                    app.switch_prev_buffer();
                } else {
                    app.move_word_back();
                }
            }
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
            (KeyCode::Char('G'), _) => {
                if let Some(count) = app.pending_count.take() {
                    app.move_to_line(count);
                } else {
                    app.move_to_bottom();
                }
            }
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
        if app.pending_count.is_some() && !app.pending_g {
            app.pending_count = None;
        }
    }

    finalize_repeat(app, pre_tick);

    Ok(false)
}

fn should_start_repeat(app: &App, key: &KeyEvent) -> bool {
    match app.mode {
        Mode::Normal => match (key.code, key.modifiers) {
            (KeyCode::Char('i'), KeyModifiers::NONE)
            | (KeyCode::Char('a'), KeyModifiers::NONE)
            | (KeyCode::Char('I'), _)
            | (KeyCode::Char('A'), _)
            | (KeyCode::Char('o'), KeyModifiers::NONE)
            | (KeyCode::Char('O'), _)
            | (KeyCode::Char('x'), KeyModifiers::NONE)
            | (KeyCode::Char('p'), KeyModifiers::NONE)
            | (KeyCode::Char('P'), _)
            | (KeyCode::Char('d'), KeyModifiers::NONE)
            | (KeyCode::Char('c'), KeyModifiers::NONE)
            | (KeyCode::Char('v'), KeyModifiers::NONE)
            | (KeyCode::Char('V'), _)
            | (KeyCode::Char('v'), KeyModifiers::CONTROL) => true,
            _ => false,
        },
        Mode::VisualChar | Mode::VisualLine | Mode::VisualBlock => match (key.code, key.modifiers) {
            (KeyCode::Char('d'), KeyModifiers::NONE)
            | (KeyCode::Char('c'), KeyModifiers::NONE)
            | (KeyCode::Char('p'), KeyModifiers::NONE)
            | (KeyCode::Char('P'), _)
            | (KeyCode::Char('I'), _)
            | (KeyCode::Char('A'), _) => true,
            _ => false,
        },
        _ => false,
    }
}

fn should_finish_repeat(app: &App) -> bool {
    app.repeat_changed
        && !matches!(app.mode, Mode::Insert | Mode::Command | Mode::VisualChar | Mode::VisualLine | Mode::VisualBlock)
        && app.operator_pending.is_none()
        && app.pending_textobj.is_none()
        && app.pending_find.is_none()
        && !app.pending_g
}

fn should_cancel_repeat(app: &App) -> bool {
    !app.repeat_changed
        && matches!(app.mode, Mode::Normal)
        && app.operator_pending.is_none()
        && app.pending_textobj.is_none()
        && app.pending_find.is_none()
        && !app.pending_g
}

fn finalize_repeat(app: &mut App, pre_tick: u64) {
    if app.repeat_recording && !app.repeat_replaying {
        if app.change_tick != pre_tick {
            app.repeat_changed = true;
        }
        if should_finish_repeat(app) {
            if app.repeat_changed {
                app.last_change = app.repeat_buffer.clone();
            }
            app.repeat_recording = false;
            app.repeat_changed = false;
            app.repeat_buffer.clear();
        } else if should_cancel_repeat(app) {
            app.repeat_recording = false;
            app.repeat_changed = false;
            app.repeat_buffer.clear();
        }
    }
}

fn replay_last_change(app: &mut App) -> Result<()> {
    if app.last_change.is_empty() {
        app.set_status("No previous change");
        return Ok(());
    }
    if app.last_change.len() == 2
        && matches!(app.last_change[0].code, KeyCode::Char('d'))
        && app.last_change[0].modifiers == KeyModifiers::NONE
        && matches!(app.last_change[1].code, KeyCode::Char('d'))
        && app.last_change[1].modifiers == KeyModifiers::NONE
    {
        app.repeat_replaying = true;
        app.repeat_recording = false;
        app.repeat_changed = false;
        app.repeat_buffer.clear();
        app.yank_line(app.cursor_row);
        app.delete_line(app.cursor_row);
        app.repeat_replaying = false;
        return Ok(());
    }
    let keys = app.last_change.clone();
    app.repeat_replaying = true;
    app.repeat_recording = false;
    app.repeat_changed = false;
    app.repeat_buffer.clear();
    for rk in keys {
        let event = KeyEvent::new(rk.code, rk.modifiers);
        let _ = handle_key(app, event)?;
    }
    app.repeat_replaying = false;
    Ok(())
}

fn complete_set_in_command(app: &mut App, reverse: bool) -> bool {
    if !app.command_buffer.starts_with("set") {
        return false;
    }
    let rest = app.command_buffer.strip_prefix("set").unwrap_or("").trim_start();
    let prefix = if rest.starts_with("theme=") {
        "set theme="
    } else if rest.starts_with("shiftwidth=") {
        "set shiftwidth="
    } else {
        "set "
    };

    let options = if rest.starts_with("theme=") {
        let mut themes = vec!["light".to_string(), "dark".to_string(), "solarized".to_string()];
        if let Some(overrides) = app.theme_overrides.as_ref() {
            for name in overrides.keys() {
                if !themes.iter().any(|t| t == name) {
                    themes.push(name.clone());
                }
            }
        }
        themes.sort();
        themes
            .into_iter()
            .map(|name| format!("set theme={}", name))
            .collect()
    } else if rest.starts_with("shiftwidth=") {
        vec![
            "set shiftwidth=2".to_string(),
            "set shiftwidth=4".to_string(),
            "set shiftwidth=8".to_string(),
        ]
    } else if rest == "relativenumber"
        || rest == "norelativenumber"
        || rest == "rnu"
        || rest == "nornu"
    {
        vec![
            "set relativenumber".to_string(),
            "set norelativenumber".to_string(),
            "set rnu".to_string(),
            "set nornu".to_string(),
            "set relativenumber?".to_string(),
            "set rnu?".to_string(),
        ]
    } else if rest == "findcross" || rest == "nofindcross" {
        vec![
            "set findcross".to_string(),
            "set nofindcross".to_string(),
            "set findcross?".to_string(),
        ]
    } else if rest == "indentcolon" || rest == "noindentcolon" {
        vec![
            "set indentcolon".to_string(),
            "set noindentcolon".to_string(),
            "set indentcolon?".to_string(),
        ]
    } else {
        vec![
            "set findcross".to_string(),
            "set nofindcross".to_string(),
            "set findcross?".to_string(),
            "set shiftwidth=".to_string(),
            "set shiftwidth?".to_string(),
            "set indentcolon".to_string(),
            "set noindentcolon".to_string(),
            "set indentcolon?".to_string(),
            "set relativenumber".to_string(),
            "set norelativenumber".to_string(),
            "set relativenumber?".to_string(),
            "set rnu".to_string(),
            "set nornu".to_string(),
            "set rnu?".to_string(),
            "set theme=".to_string(),
            "set theme?".to_string(),
        ]
    };

    let mut matches: Vec<String> = options
        .into_iter()
        .filter(|opt| opt.starts_with(app.command_buffer.as_str()))
        .map(|opt| opt.to_string())
        .collect();
    if matches.is_empty() && rest.is_empty() {
        matches = vec![
            "set findcross".to_string(),
            "set nofindcross".to_string(),
            "set shiftwidth=".to_string(),
            "set indentcolon".to_string(),
            "set relativenumber".to_string(),
            "set theme=".to_string(),
        ];
    }
    if matches.is_empty() {
        app.clear_completion();
        return false;
    }
    matches.sort();

    let should_cycle = app
        .completion_cmd_prefix
        .as_deref()
        .is_some_and(|p| p == "<set>")
        && app
            .completion_candidates
            .iter()
            .any(|candidate| candidate == app.command_buffer.as_str());
    if should_cycle {
        let next_idx = match app.completion_index {
            Some(idx) => {
                if reverse {
                    (idx + app.completion_candidates.len() - 1) % app.completion_candidates.len()
                } else {
                    (idx + 1) % app.completion_candidates.len()
                }
            }
            None => 0,
        };
        app.completion_index = Some(next_idx);
        let next = app.completion_candidates[next_idx].clone();
        app.command_buffer = next;
        command_cursor_to_end(app);
        return true;
    }

    app.completion_candidates = matches;
    app.completion_index = Some(0);
    app.completion_cmd_prefix = Some("<set>".to_string());
    app.completion_anchor_fixed = true;
    app.completion_anchor_col = Some(prefix.chars().count() as u16);
    let first = app.completion_candidates[0].clone();
    app.command_buffer = first;
    command_cursor_to_end(app);
    true
}

fn command_buffer_len(app: &App) -> usize {
    app.command_buffer.chars().count()
}

fn command_cursor_to_end(app: &mut App) {
    app.command_cursor = command_buffer_len(app);
}

fn command_insert_char(app: &mut App, ch: char) {
    let idx = app.command_cursor.min(command_buffer_len(app));
    let byte_idx = char_to_byte_idx(&app.command_buffer, idx);
    app.command_buffer.insert(byte_idx, ch);
    app.command_cursor = idx + 1;
}

fn command_backspace(app: &mut App) {
    if app.command_cursor == 0 {
        if app.command_buffer.is_empty() {
            app.mode = Mode::Normal;
            app.command_prompt = CommandPrompt::Command;
            app.search_history_index = None;
            app.command_history_index = None;
            app.clear_completion();
        }
        return;
    }
    let start = app.command_cursor - 1;
    let start_byte = char_to_byte_idx(&app.command_buffer, start);
    let end_byte = char_to_byte_idx(&app.command_buffer, app.command_cursor);
    app.command_buffer.replace_range(start_byte..end_byte, "");
    app.command_cursor = start;
    if app.command_buffer.is_empty() {
        app.mode = Mode::Normal;
        app.command_prompt = CommandPrompt::Command;
        app.search_history_index = None;
        app.command_history_index = None;
        app.clear_completion();
    }
}

fn is_command_word_sep(ch: char) -> bool {
    ch.is_whitespace() || ch == '/'
}

fn command_delete_prev_word(app: &mut App) {
    let len = command_buffer_len(app);
    let mut idx = app.command_cursor.min(len);
    if idx == 0 {
        return;
    }
    let chars: Vec<char> = app.command_buffer.chars().collect();
    while idx > 0 && is_command_word_sep(chars[idx - 1]) {
        idx -= 1;
    }
    while idx > 0 && !is_command_word_sep(chars[idx - 1]) {
        idx -= 1;
    }
    let start_byte = char_to_byte_idx(&app.command_buffer, idx);
    let end_byte = char_to_byte_idx(&app.command_buffer, app.command_cursor);
    app.command_buffer.replace_range(start_byte..end_byte, "");
    app.command_cursor = idx;
}

fn command_delete_to_line_start(app: &mut App) {
    if app.command_cursor == 0 {
        return;
    }
    let end_byte = char_to_byte_idx(&app.command_buffer, app.command_cursor);
    app.command_buffer.replace_range(0..end_byte, "");
    app.command_cursor = 0;
}

fn command_move_left(app: &mut App) {
    if app.command_cursor > 0 {
        app.command_cursor -= 1;
    }
}

fn command_move_right(app: &mut App) {
    let len = command_buffer_len(app);
    if app.command_cursor < len {
        app.command_cursor += 1;
    }
}

fn command_move_word_left(app: &mut App) {
    let len = command_buffer_len(app);
    let mut idx = app.command_cursor.min(len);
    if idx == 0 {
        return;
    }
    let chars: Vec<char> = app.command_buffer.chars().collect();
    while idx > 0 && is_command_word_sep(chars[idx - 1]) {
        idx -= 1;
    }
    while idx > 0 && !is_command_word_sep(chars[idx - 1]) {
        idx -= 1;
    }
    app.command_cursor = idx;
}

fn command_move_word_right(app: &mut App) {
    let len = command_buffer_len(app);
    let mut idx = app.command_cursor.min(len);
    let chars: Vec<char> = app.command_buffer.chars().collect();
    while idx < len && is_command_word_sep(chars[idx]) {
        idx += 1;
    }
    while idx < len && !is_command_word_sep(chars[idx]) {
        idx += 1;
    }
    app.command_cursor = idx;
}

fn command_move_line_start(app: &mut App) {
    app.command_cursor = 0;
}

fn command_move_line_end(app: &mut App) {
    app.command_cursor = command_buffer_len(app);
}

fn complete_path_in_command(app: &mut App, reverse: bool) -> bool {
    if !matches!(app.command_prompt, CommandPrompt::Command) {
        return false;
    }

    let (cmd_prefix, path_part) = if app.command_buffer == "e" {
        ("e ", "")
    } else if let Some(rest) = app.command_buffer.strip_prefix("e ") {
        ("e ", rest)
    } else if app.command_buffer == "edit" {
        ("edit ", "")
    } else if let Some(rest) = app.command_buffer.strip_prefix("edit ") {
        ("edit ", rest)
    } else if app.command_buffer == "w" {
        ("w ", "")
    } else if let Some(rest) = app.command_buffer.strip_prefix("w ") {
        ("w ", rest)
    } else if app.command_buffer == "write" {
        ("write ", "")
    } else if let Some(rest) = app.command_buffer.strip_prefix("write ") {
        ("write ", rest)
    } else {
        return false;
    };

    let should_cycle = app
        .completion_cmd_prefix
        .as_deref()
        .is_some_and(|prefix| prefix == cmd_prefix)
        && !app.completion_candidates.is_empty()
        && app
            .completion_candidates
            .iter()
            .any(|candidate| candidate == path_part);

    if should_cycle {
        let next_idx = match app.completion_index {
            Some(idx) => {
                if reverse {
                    (idx + app.completion_candidates.len() - 1) % app.completion_candidates.len()
                } else {
                    (idx + 1) % app.completion_candidates.len()
                }
            }
            None => 0,
        };
        app.completion_index = Some(next_idx);
        let next = app.completion_candidates[next_idx].clone();
        app.command_buffer = format!("{}{}", cmd_prefix, next);
        command_cursor_to_end(app);
        return true;
    }

    let (quote_char, raw_path) = strip_leading_quote(path_part);
    let unescaped = if quote_char.is_some() {
        raw_path.to_string()
    } else {
        unescape_path(raw_path)
    };
    let (expanded_path_part, _had_tilde) = expand_tilde(&unescaped);
    let trimmed = expanded_path_part.trim_end_matches('/');
    let path_is_dir =
        !trimmed.is_empty() && fs::metadata(trimmed).map(|m| m.is_dir()).unwrap_or(false);
    let (dir_part, base, dir_for_fs) = if path_is_dir {
        let display_dir = format!("{}/", unescaped.trim_end_matches('/'));
        (display_dir, "", trimmed.to_string())
    } else {
        let (dir_display, file_base) = match unescaped.rfind('/') {
            Some(idx) => (&unescaped[..=idx], &unescaped[idx + 1..]),
            None => ("", unescaped.as_str()),
        };
        let (dir_fs, _) = match expanded_path_part.rfind('/') {
            Some(idx) => (&expanded_path_part[..=idx], &expanded_path_part[idx + 1..]),
            None => ("", expanded_path_part.as_str()),
        };
        let dir_for_fs = if dir_fs.is_empty() {
            ".".to_string()
        } else if dir_fs == "/" {
            "/".to_string()
        } else {
            dir_fs.trim_end_matches('/').to_string()
        };
        (dir_display.to_string(), file_base, dir_for_fs)
    };

    let mut matches: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir_for_fs) {
        for entry in entries.flatten() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if !name.starts_with(base) {
                continue;
            }
            let mut candidate = if dir_part.is_empty() {
                name
            } else {
                format!("{}{}", dir_part, name)
            };
            if entry.path().is_dir() {
                candidate.push('/');
            }
            matches.push(format_candidate(&candidate, quote_char));
        }
    }

    if base == "." || base == ".." {
        let mut candidate = if dir_part.is_empty() {
            format!("{}/", base)
        } else {
            format!("{}{}", dir_part, base)
        };
        if !candidate.ends_with('/') {
            candidate.push('/');
        }
        matches.push(format_candidate(&candidate, quote_char));
    }

    if matches.is_empty() {
        app.clear_completion();
        return false;
    }

    matches.sort();

    if !reverse && matches.len() == 1 {
        let only = matches[0].clone();
        app.command_buffer = format!("{}{}", cmd_prefix, only);
        command_cursor_to_end(app);
        app.clear_completion();
        return true;
    }

    app.completion_candidates = matches;
    app.completion_index = Some(0);
    app.completion_cmd_prefix = Some(cmd_prefix.to_string());
    app.completion_anchor_fixed = path_part.is_empty();
    app.completion_anchor_col = Some(cmd_prefix.chars().count() as u16);
    let first = app.completion_candidates[0].clone();
    app.command_buffer = format!("{}{}", cmd_prefix, first);
    command_cursor_to_end(app);
    true
}

fn enter_completion_directory(app: &mut App) -> bool {
    if !matches!(app.command_prompt, CommandPrompt::Command) {
        return false;
    }

    let Some(prefix) = app.completion_cmd_prefix.as_deref() else {
        return false;
    };
    if !matches!(prefix, "e " | "edit " | "w " | "write ") {
        return false;
    }

    let idx = app.completion_index.unwrap_or(0);
    let Some(candidate) = app.completion_candidates.get(idx) else {
        return false;
    };
    if !candidate.ends_with('/') {
        return false;
    }

    app.command_buffer = format!("{}{}", prefix, candidate);
    app.clear_completion();
    complete_path_in_command(app, false)
}

fn complete_command_in_command(app: &mut App, reverse: bool) -> bool {
    if !matches!(app.command_prompt, CommandPrompt::Command) {
        return false;
    }
    if app.command_buffer.contains(' ') {
        return false;
    }

    let current = app.command_buffer.as_str();
    let mut matches: Vec<String> = app
        .command_candidates
        .iter()
        .filter(|opt| opt.starts_with(current))
        .map(|opt| opt.to_string())
        .collect();
    if matches.is_empty() {
        app.clear_completion();
        return false;
    }
    matches.sort();

    let should_cycle = app
        .completion_cmd_prefix
        .as_deref()
        .is_some_and(|prefix| prefix == "<cmd>")
        && app
            .completion_candidates
            .iter()
            .any(|candidate| candidate == current);
    if should_cycle {
        let next_idx = match app.completion_index {
            Some(idx) => {
                if reverse {
                    (idx + app.completion_candidates.len() - 1) % app.completion_candidates.len()
                } else {
                    (idx + 1) % app.completion_candidates.len()
                }
            }
            None => 0,
        };
        app.completion_index = Some(next_idx);
        let next = app.completion_candidates[next_idx].clone();
        app.command_buffer = next;
        command_cursor_to_end(app);
        return true;
    }

    app.completion_candidates = matches;
    app.completion_index = Some(0);
    app.completion_cmd_prefix = Some("<cmd>".to_string());
    app.completion_anchor_fixed = true;
    app.completion_anchor_col = Some(current.chars().count() as u16);
    let first = app.completion_candidates[0].clone();
    app.command_buffer = first;
    command_cursor_to_end(app);
    true
}

fn apply_keymap_action(app: &mut App, action: KeyAction) -> Result<Option<bool>> {
    match app.mode {
        Mode::Command => {
            match action {
                KeyAction::MoveLeft => command_move_left(app),
                KeyAction::MoveRight => command_move_right(app),
                KeyAction::MoveWordLeft => command_move_word_left(app),
                KeyAction::MoveWordRight => command_move_word_right(app),
                KeyAction::MoveLineStart => command_move_line_start(app),
                KeyAction::MoveLineEnd => command_move_line_end(app),
                KeyAction::Backspace => command_backspace(app),
                KeyAction::DeleteWord => command_delete_prev_word(app),
                KeyAction::DeleteLineStart => command_delete_to_line_start(app),
                KeyAction::Enter => {
                    let should_quit = if matches!(app.command_prompt, CommandPrompt::Command) {
                        app.execute_command()?
                    } else {
                        app.execute_search()?
                    };
                    app.command_buffer.clear();
                    app.command_cursor = 0;
                    app.mode = Mode::Normal;
                    app.command_prompt = CommandPrompt::Command;
                    app.search_history_index = None;
                    app.command_history_index = None;
                    app.clear_completion();
                    if should_quit {
                        return Ok(Some(true));
                    }
                    return Ok(Some(false));
                }
                KeyAction::Escape => {
                    app.mode = Mode::Normal;
                    app.command_buffer.clear();
                    app.command_cursor = 0;
                    app.command_prompt = CommandPrompt::Command;
                    app.search_history_index = None;
                    app.command_history_index = None;
                    app.clear_completion();
                    return Ok(Some(false));
                }
                KeyAction::Tab => {
                    if complete_path_in_command(app, false)
                        || complete_set_in_command(app, false)
                        || complete_command_in_command(app, false)
                    {
                        app.search_history_index = None;
                    }
                    return Ok(Some(false));
                }
                KeyAction::BackTab => {
                    if complete_path_in_command(app, true)
                        || complete_set_in_command(app, true)
                        || complete_command_in_command(app, true)
                    {
                        app.search_history_index = None;
                    }
                    return Ok(Some(false));
                }
                _ => {}
            }
            app.search_history_index = None;
            app.command_history_index = None;
            app.clear_completion();
            Ok(Some(false))
        }
        Mode::Insert => {
            match action {
                KeyAction::MoveLeft => {
                    app.insert_undo_snapshot = false;
                    app.move_left();
                }
                KeyAction::MoveRight => {
                    app.insert_undo_snapshot = false;
                    app.move_right();
                }
                KeyAction::MoveUp => {
                    app.insert_undo_snapshot = false;
                    app.move_up();
                }
                KeyAction::MoveDown => {
                    app.insert_undo_snapshot = false;
                    app.move_down();
                }
                KeyAction::MoveWordLeft => {
                    app.insert_undo_snapshot = false;
                    app.move_word_back();
                }
                KeyAction::MoveWordRight => {
                    app.insert_undo_snapshot = false;
                    app.move_word_forward();
                }
                KeyAction::MoveLineStart => {
                    app.insert_undo_snapshot = false;
                    app.move_line_start();
                }
                KeyAction::MoveLineEnd => {
                    app.insert_undo_snapshot = false;
                    app.move_line_end();
                }
                KeyAction::Backspace => {
                    app.insert_undo_snapshot = false;
                    app.backspace();
                }
                KeyAction::Enter => {
                    app.insert_undo_snapshot = false;
                    app.insert_newline();
                }
                KeyAction::Escape => {
                    app.mode = Mode::Normal;
                    app.block_insert = None;
                    app.insert_undo_snapshot = false;
                    app.set_status("-- NORMAL --");
                }
                _ => return Ok(None),
            }
            Ok(Some(false))
        }
        Mode::Normal | Mode::VisualChar | Mode::VisualLine | Mode::VisualBlock => {
            match action {
                KeyAction::BufferNext => app.switch_next_buffer(),
                KeyAction::BufferPrev => app.switch_prev_buffer(),
                KeyAction::MoveLeft => app.move_left(),
                KeyAction::MoveRight => app.move_right(),
                KeyAction::MoveUp => app.move_up(),
                KeyAction::MoveDown => app.move_down(),
                KeyAction::MoveWordLeft => app.move_word_back(),
                KeyAction::MoveWordRight => app.move_word_forward(),
                KeyAction::MoveLineStart => app.move_line_start(),
                KeyAction::MoveLineEnd => app.move_line_end(),
                KeyAction::Escape => {
                    if !matches!(app.mode, Mode::Normal) {
                        exit_visual(app);
                    }
                }
                _ => return Ok(None),
            }
            Ok(Some(false))
        }
    }
}

fn exit_visual(app: &mut App) {
    if let Some(selection) = app.visual_selection() {
        app.last_visual = Some(selection_to_last_visual(selection, app.mode));
    }
    app.mode = Mode::Normal;
    app.visual_start = None;
    app.set_status("-- NORMAL --");
}

pub(crate) fn expand_tilde(input: &str) -> (String, bool) {
    if !input.starts_with('~') {
        return (input.to_string(), false);
    }
    let Ok(home) = std::env::var("HOME") else {
        return (input.to_string(), false);
    };
    if input == "~" {
        return (home, true);
    }
    if let Some(rest) = input.strip_prefix("~/") {
        return (format!("{}/{}", home, rest), true);
    }
    (input.to_string(), false)
}

pub(crate) fn expand_tilde_path(input: impl AsRef<str>) -> String {
    let input = input.as_ref();
    let trimmed = input.trim();
    let (quote, rest) = strip_leading_quote(trimmed);
    let (expanded, _had_tilde) = expand_tilde(rest);
    if quote.is_some() {
        format!("{}{}", quote.unwrap(), expanded)
    } else {
        expanded
    }
}

fn strip_leading_quote(input: &str) -> (Option<char>, &str) {
    let mut chars = input.chars();
    match chars.next() {
        Some('"') => (Some('"'), &input[1..]),
        Some('\'') => (Some('\''), &input[1..]),
        _ => (None, input),
    }
}

fn unescape_path(input: &str) -> String {
    let mut out = String::new();
    let mut iter = input.chars();
    while let Some(ch) = iter.next() {
        if ch == '\\' {
            if let Some(next) = iter.next() {
                out.push(next);
            } else {
                out.push('\\');
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn escape_unquoted_path(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        if ch == ' ' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn format_candidate(candidate: &str, quote_char: Option<char>) -> String {
    if let Some(quote) = quote_char {
        let mut out = String::new();
        out.push(quote);
        for ch in candidate.chars() {
            if ch == quote || ch == '\\' {
                out.push('\\');
            }
            out.push(ch);
        }
        return out;
    }
    escape_unquoted_path(candidate)
}
