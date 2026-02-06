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

#[derive(Debug, Clone)]
pub(crate) struct Keymaps {
    normal: HashMap<Vec<KeySpec>, KeyAction>,
    insert: HashMap<Vec<KeySpec>, KeyAction>,
    visual: HashMap<Vec<KeySpec>, KeyAction>,
    command: HashMap<Vec<KeySpec>, KeyAction>,
}

impl Keymaps {
    pub(crate) fn action_for_seq(
        &self,
        mode: Mode,
        key: &KeyEvent,
        seq: &mut Vec<KeySpec>,
    ) -> KeymapResult {
        let spec = KeySpec {
            code: key.code,
            mods: key.modifiers,
        };
        let map = match mode {
            Mode::Normal => &self.normal,
            Mode::Insert => &self.insert,
            Mode::VisualChar | Mode::VisualLine | Mode::VisualBlock => &self.visual,
            Mode::Command => &self.command,
        };
        seq.push(spec);
        if let Some(action) = map.get(seq).copied() {
            seq.clear();
            return KeymapResult::Matched(action);
        }
        if has_prefix(map, seq) {
            return KeymapResult::Pending;
        }
        seq.clear();
        seq.push(spec);
        if let Some(action) = map.get(seq).copied() {
            seq.clear();
            return KeymapResult::Matched(action);
        }
        if has_prefix(map, seq) {
            return KeymapResult::Pending;
        }
        seq.clear();
        KeymapResult::NoMatch
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

impl Default for Keymaps {
    fn default() -> Self {
        let mut normal = HashMap::new();
        if let Some(seq) = parse_key_sequence("]b") {
            normal.insert(seq, KeyAction::BufferNext);
        }
        if let Some(seq) = parse_key_sequence("[b") {
            normal.insert(seq, KeyAction::BufferPrev);
        }
        Keymaps {
            normal,
            insert: HashMap::new(),
            visual: HashMap::new(),
            command: HashMap::new(),
        }
    }
}

fn parse_map(
    map: &HashMap<String, String>,
    out: &mut HashMap<Vec<KeySpec>, KeyAction>,
    errors: &mut Vec<String>,
) {
    for (lhs, rhs) in map {
        let Some(seq) = parse_key_sequence(lhs) else {
            errors.push(format!("Invalid key: {}", lhs));
            continue;
        };
        let Some(action) = parse_key_action(rhs) else {
            errors.push(format!("Invalid action: {}", rhs));
            continue;
        };
        out.insert(seq, action);
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

fn parse_key_sequence(raw: &str) -> Option<Vec<KeySpec>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    let mut iter = trimmed.chars().peekable();
    while let Some(ch) = iter.peek().copied() {
        if ch == '<' {
            let mut token = String::new();
            token.push(ch);
            iter.next();
            while let Some(next) = iter.next() {
                token.push(next);
                if next == '>' {
                    break;
                }
            }
            if !token.ends_with('>') {
                return None;
            }
            let inner = &token[1..token.len() - 1];
            let spec = parse_bracketed_key(inner)?;
            out.push(spec);
        } else {
            iter.next();
            out.push(KeySpec {
                code: KeyCode::Char(ch),
                mods: KeyModifiers::NONE,
            });
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
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

fn has_prefix(map: &HashMap<Vec<KeySpec>, KeyAction>, seq: &[KeySpec]) -> bool {
    map.keys().any(|k| k.len() >= seq.len() && k[..seq.len()] == *seq)
}

pub(crate) enum KeymapResult {
    Matched(KeyAction),
    Pending,
    NoMatch,
}
