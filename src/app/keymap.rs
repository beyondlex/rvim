use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::config::KeymapConfig;
use super::types::Mode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct KeySpec {
    pub(crate) code: KeyCode,
    pub(crate) mods: KeyModifiers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyAction {
    BufferNext,
    BufferPrev,
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    MoveWordLeft,
    MoveWordRight,
    MoveLineStart,
    MoveLineEnd,
    Backspace,
    DeleteWord,
    DeleteLineStart,
    Enter,
    Escape,
    Tab,
    BackTab,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct Keymaps {
    normal: HashMap<KeySpec, KeyAction>,
    insert: HashMap<KeySpec, KeyAction>,
    visual: HashMap<KeySpec, KeyAction>,
    command: HashMap<KeySpec, KeyAction>,
}

impl Keymaps {
    pub(crate) fn action_for(&self, mode: Mode, key: &KeyEvent) -> Option<KeyAction> {
        let spec = KeySpec {
            code: key.code,
            mods: key.modifiers,
        };
        match mode {
            Mode::Normal => self.normal.get(&spec).copied(),
            Mode::Insert => self.insert.get(&spec).copied(),
            Mode::VisualChar | Mode::VisualLine | Mode::VisualBlock => {
                self.visual.get(&spec).copied()
            }
            Mode::Command => self.command.get(&spec).copied(),
        }
    }

    pub(crate) fn from_config(cfg: Option<&KeymapConfig>) -> (Self, Vec<String>) {
        let mut keymaps = Keymaps::default();
        let mut errors = Vec::new();
        let Some(cfg) = cfg else {
            return (keymaps, errors);
        };

        if let Some(map) = cfg.normal.as_ref() {
            parse_map(map, &mut keymaps.normal, &mut errors);
        }
        if let Some(map) = cfg.insert.as_ref() {
            parse_map(map, &mut keymaps.insert, &mut errors);
        }
        if let Some(map) = cfg.visual.as_ref() {
            parse_map(map, &mut keymaps.visual, &mut errors);
        }
        if let Some(map) = cfg.command.as_ref() {
            parse_map(map, &mut keymaps.command, &mut errors);
        }

        (keymaps, errors)
    }
}

fn parse_map(
    map: &HashMap<String, String>,
    out: &mut HashMap<KeySpec, KeyAction>,
    errors: &mut Vec<String>,
) {
    for (lhs, rhs) in map {
        let Some(spec) = parse_key_spec(lhs) else {
            errors.push(format!("Invalid key: {}", lhs));
            continue;
        };
        let Some(action) = parse_key_action(rhs) else {
            errors.push(format!("Invalid action: {}", rhs));
            continue;
        };
        out.insert(spec, action);
    }
}

fn parse_key_action(s: &str) -> Option<KeyAction> {
    match s.trim().to_ascii_lowercase().as_str() {
        "buffer_next" | "bnext" | "bn" => Some(KeyAction::BufferNext),
        "buffer_prev" | "bprev" | "bp" => Some(KeyAction::BufferPrev),
        "left" | "move_left" => Some(KeyAction::MoveLeft),
        "right" | "move_right" => Some(KeyAction::MoveRight),
        "up" | "move_up" => Some(KeyAction::MoveUp),
        "down" | "move_down" => Some(KeyAction::MoveDown),
        "word_left" | "move_word_left" => Some(KeyAction::MoveWordLeft),
        "word_right" | "move_word_right" => Some(KeyAction::MoveWordRight),
        "line_start" | "move_line_start" => Some(KeyAction::MoveLineStart),
        "line_end" | "move_line_end" => Some(KeyAction::MoveLineEnd),
        "backspace" => Some(KeyAction::Backspace),
        "delete_word" => Some(KeyAction::DeleteWord),
        "delete_line_start" => Some(KeyAction::DeleteLineStart),
        "enter" => Some(KeyAction::Enter),
        "escape" => Some(KeyAction::Escape),
        "tab" => Some(KeyAction::Tab),
        "backtab" => Some(KeyAction::BackTab),
        _ => None,
    }
}

fn parse_key_spec(raw: &str) -> Option<KeySpec> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() == 1 {
        let ch = trimmed.chars().next()?;
        return Some(KeySpec {
            code: KeyCode::Char(ch),
            mods: KeyModifiers::NONE,
        });
    }
    if trimmed.starts_with('<') && trimmed.ends_with('>') {
        return parse_bracketed_key(&trimmed[1..trimmed.len() - 1]);
    }
    parse_named_key(trimmed)
}

fn parse_bracketed_key(raw: &str) -> Option<KeySpec> {
    let parts: Vec<&str> = raw.split('-').collect();
    if parts.is_empty() {
        return None;
    }
    let mut mods = KeyModifiers::NONE;
    for part in &parts[0..parts.len().saturating_sub(1)] {
        match part.to_ascii_lowercase().as_str() {
            "c" | "ctrl" | "control" => mods |= KeyModifiers::CONTROL,
            "m" | "alt" | "meta" => mods |= KeyModifiers::ALT,
            "d" | "cmd" | "super" => mods |= KeyModifiers::SUPER,
            "s" | "shift" => mods |= KeyModifiers::SHIFT,
            _ => return None,
        }
    }
    let key = *parts.last()?;
    let mut spec = parse_named_key(key)?;
    spec.mods |= mods;
    Some(spec)
}

fn parse_named_key(raw: &str) -> Option<KeySpec> {
    let lower = raw.to_ascii_lowercase();
    let code = match lower.as_str() {
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "backspace" | "bs" => KeyCode::Backspace,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "enter" | "cr" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "space" => KeyCode::Char(' '),
        "delete" | "del" => KeyCode::Delete,
        "insert" | "ins" => KeyCode::Insert,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        _ => {
            if raw.chars().count() == 1 {
                KeyCode::Char(raw.chars().next()?)
            } else {
                return None;
            }
        }
    };
    Some(KeySpec {
        code,
        mods: KeyModifiers::NONE,
    })
}
