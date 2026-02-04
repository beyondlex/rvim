use std::fs;
use std::path::PathBuf;

use anyhow::Result;

use super::types::{BufferSlot, BufferState, CommandPrompt, SearchSpec};
use super::App;
use super::Theme;

impl App {
    fn find_buffer_id_by_path(&self, path: &PathBuf) -> Option<usize> {
        for slot in &self.buffers {
            if slot.state.file_path.as_ref() == Some(path) {
                return Some(slot.id);
            }
        }
        None
    }

    fn sorted_buffer_ids(&self) -> Vec<usize> {
        let mut ids = Vec::with_capacity(self.buffers.len() + 1);
        ids.push(self.current_buffer_id);
        for slot in &self.buffers {
            ids.push(slot.id);
        }
        ids.sort();
        ids
    }

    fn switch_to_buffer(&mut self, id: usize) -> bool {
        if id == self.current_buffer_id {
            return true;
        }
        let idx = match self.buffers.iter().position(|slot| slot.id == id) {
            Some(idx) => idx,
            None => {
                self.set_status("No such buffer");
                return false;
            }
        };
        let target = self.buffers.swap_remove(idx);
        let current_state = self.capture_buffer_state();
        let current_id = self.current_buffer_id;
        self.buffers.push(BufferSlot {
            id: current_id,
            state: current_state,
        });
        self.load_buffer_state(target.state);
        self.current_buffer_id = target.id;
        self.reset_transient_for_switch();
        true
    }

    fn switch_next_buffer(&mut self) {
        let ids = self.sorted_buffer_ids();
        if ids.len() <= 1 {
            self.set_status("No other buffers");
            return;
        }
        let idx = ids
            .iter()
            .position(|id| *id == self.current_buffer_id)
            .unwrap_or(0);
        let next_id = ids[(idx + 1) % ids.len()];
        if self.switch_to_buffer(next_id) {
            self.set_status(format!("Buffer {}", next_id));
        }
    }

    fn switch_prev_buffer(&mut self) {
        let ids = self.sorted_buffer_ids();
        if ids.len() <= 1 {
            self.set_status("No other buffers");
            return;
        }
        let idx = ids
            .iter()
            .position(|id| *id == self.current_buffer_id)
            .unwrap_or(0);
        let prev_id = if idx == 0 { ids[ids.len() - 1] } else { ids[idx - 1] };
        if self.switch_to_buffer(prev_id) {
            self.set_status(format!("Buffer {}", prev_id));
        }
    }

    fn open_or_switch_buffer(&mut self, path: PathBuf) -> Result<()> {
        if self.file_path.as_ref() == Some(&path) {
            self.reload(&path)?;
            self.set_status(format!("Opened {}", path.display()));
            return Ok(());
        }
        if let Some(id) = self.find_buffer_id_by_path(&path) {
            if self.switch_to_buffer(id) {
                self.set_status(format!("Buffer {}", id));
            }
            return Ok(());
        }
        let content = fs::read_to_string(&path).unwrap_or_default();
        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        let new_state = BufferState {
            lines,
            cursor_row: 0,
            cursor_col: 0,
            scroll_row: 0,
            scroll_col: 0,
            file_path: Some(path.clone()),
            dirty: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            line_undo: None,
            is_restoring: false,
            change_tick: 0,
        };
        let current_state = self.capture_buffer_state();
        let current_id = self.current_buffer_id;
        self.buffers.push(BufferSlot {
            id: current_id,
            state: current_state,
        });
        self.load_buffer_state(new_state);
        self.current_buffer_id = self.next_buffer_id;
        self.next_buffer_id += 1;
        self.reset_transient_for_switch();
        self.set_status(format!("Opened {}", path.display()));
        Ok(())
    }

    fn close_buffer(&mut self, id: Option<usize>, force: bool) {
        let target_id = id.unwrap_or(self.current_buffer_id);
        if target_id == self.current_buffer_id {
            if self.dirty && !force {
                self.set_status("No write since last change (add ! to override)");
                return;
            }
            if self.buffers.is_empty() {
                self.lines = vec![String::new()];
                self.cursor_row = 0;
                self.cursor_col = 0;
                self.scroll_row = 0;
                self.scroll_col = 0;
                self.file_path = None;
                self.dirty = false;
                self.undo_stack.clear();
                self.redo_stack.clear();
                self.line_undo = None;
                self.is_restoring = false;
                self.change_tick = 0;
                self.reset_transient_for_switch();
                self.set_status("Closed buffer (new empty)");
                return;
            }
            let ids = self.sorted_buffer_ids();
            let replacement_id = ids
                .into_iter()
                .find(|id| *id != self.current_buffer_id)
                .unwrap();
            let idx = self
                .buffers
                .iter()
                .position(|slot| slot.id == replacement_id)
                .unwrap();
            let replacement = self.buffers.swap_remove(idx);
            self.load_buffer_state(replacement.state);
            self.current_buffer_id = replacement.id;
            self.reset_transient_for_switch();
            self.set_status(format!("Closed buffer {}, now {}", target_id, replacement_id));
            return;
        }
        let idx = match self.buffers.iter().position(|slot| slot.id == target_id) {
            Some(idx) => idx,
            None => {
                self.set_status("No such buffer");
                return;
            }
        };
        if self.buffers[idx].state.dirty && !force {
            self.set_status("No write since last change (add ! to override)");
            return;
        }
        self.buffers.swap_remove(idx);
        self.set_status(format!("Closed buffer {}", target_id));
    }

    pub(super) fn save(&mut self) -> Result<()> {
        let Some(path) = self.file_path.clone() else {
            self.set_status("No file name (open with a path)");
            return Ok(());
        };
        let content = self.lines.join("\n");
        fs::write(&path, content)?;
        self.dirty = false;
        self.set_status(format!("Wrote {}", path.display()));
        Ok(())
    }

    pub(super) fn reload(&mut self, path: &PathBuf) -> Result<()> {
        let content = fs::read_to_string(path).unwrap_or_default();
        self.lines = content.lines().map(|s| s.to_string()).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.scroll_row = 0;
        self.scroll_col = 0;
        self.dirty = false;
        Ok(())
    }

    pub(super) fn execute_command(&mut self) -> Result<bool> {
        let input = self.command_buffer.trim().to_string();
        if input.is_empty() {
            return Ok(false);
        }

        let mut parts = input.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        let arg = parts.next().map(|s| s.to_string());

        match cmd {
            "w" | "write" => {
                if let Some(path) = arg.as_deref().map(PathBuf::from) {
                    self.file_path = Some(path.clone());
                    self.save()?;
                } else if self.file_path.is_none() {
                    self.set_status("Usage: :w <path>");
                } else {
                    self.save()?;
                }
            }
            "q" | "quit" => {
                if self.dirty {
                    self.set_status("No write since last change (add ! to override)");
                    return Ok(false);
                }
                return Ok(true);
            }
            "q!" | "quit!" => {
                return Ok(true);
            }
            "wq" | "x" => {
                if let Some(path) = arg.as_deref().map(PathBuf::from) {
                    self.file_path = Some(path.clone());
                    self.save()?;
                    return Ok(true);
                }
                if self.file_path.is_none() {
                    self.set_status("Usage: :wq <path>");
                    return Ok(false);
                }
                self.save()?;
                return Ok(true);
            }
            "e" | "edit" => {
                if let Some(path) = arg.map(PathBuf::from) {
                    self.open_or_switch_buffer(path)?;
                } else {
                    self.set_status("Usage: :e <path>");
                }
            }
            "ls" | "buffers" => {
                self.set_status(self.list_buffers());
            }
            "b" | "buffer" => {
                if let Some(arg) = arg.as_deref() {
                    if let Ok(id) = arg.parse::<usize>() {
                        if self.switch_to_buffer(id) {
                            self.set_status(format!("Buffer {}", id));
                        }
                    } else {
                        self.set_status("Usage: :b <id>");
                    }
                } else {
                    self.set_status(self.list_buffers());
                }
            }
            "bn" | "bnext" => {
                self.switch_next_buffer();
            }
            "bp" | "bprev" => {
                self.switch_prev_buffer();
            }
            "bd" | "bdelete" => {
                if let Some(arg) = arg.as_deref() {
                    if let Ok(id) = arg.parse::<usize>() {
                        self.close_buffer(Some(id), false);
                    } else {
                        self.set_status("Usage: :bd <id>");
                    }
                } else {
                    self.close_buffer(None, false);
                }
            }
            "bd!" | "bdelete!" => {
                if let Some(arg) = arg.as_deref() {
                    if let Ok(id) = arg.parse::<usize>() {
                        self.close_buffer(Some(id), true);
                    } else {
                        self.set_status("Usage: :bd! <id>");
                    }
                } else {
                    self.close_buffer(None, true);
                }
            }
            "set" => {
                if let Some(setting) = arg.as_deref() {
                    if let Some(value) = setting.strip_prefix("shiftwidth=") {
                        if let Ok(width) = value.parse::<usize>() {
                            if width > 0 {
                                self.shift_width = width;
                                self.set_status(format!("shiftwidth={}", width));
                            } else {
                                self.set_status("shiftwidth must be > 0");
                            }
                        } else {
                            self.set_status("shiftwidth expects a number");
                        }
                        return Ok(false);
                    }
                    if let Some(value) = setting.strip_prefix("theme=") {
                        if let Some(theme) = Theme::from_name(value) {
                            self.set_theme_named(value, theme);
                            self.set_status(format!("theme={}", self.theme_name));
                        } else {
                            self.set_status("Unknown theme (use light|dark|solarized)");
                        }
                        return Ok(false);
                    }
                    match setting {
                        "findcross" => {
                            self.find_cross_line = true;
                            self.set_status("findcross");
                        }
                        "nofindcross" => {
                            self.find_cross_line = false;
                            self.set_status("nofindcross");
                        }
                        "findcross?" => {
                            let value = if self.find_cross_line {
                                "findcross"
                            } else {
                                "nofindcross"
                            };
                            self.set_status(value);
                        }
                        "relativenumber" | "rnu" => {
                            self.relative_number = true;
                            self.set_status("relativenumber");
                        }
                        "norelativenumber" | "nornu" => {
                            self.relative_number = false;
                            self.set_status("norelativenumber");
                        }
                        "relativenumber?" | "rnu?" => {
                            let value = if self.relative_number {
                                "relativenumber"
                            } else {
                                "norelativenumber"
                            };
                            self.set_status(value);
                        }
                        "theme?" => {
                            self.set_status(format!(
                                "theme={} (light|dark|solarized)",
                                self.theme_name
                            ));
                        }
                        "shiftwidth?" => {
                            self.set_status(format!("shiftwidth={}", self.shift_width));
                        }
                        "indentcolon" => {
                            self.indent_colon = true;
                            self.set_status("indentcolon");
                        }
                        "noindentcolon" => {
                            self.indent_colon = false;
                            self.set_status("noindentcolon");
                        }
                        "indentcolon?" => {
                            let value = if self.indent_colon {
                                "indentcolon"
                            } else {
                                "noindentcolon"
                            };
                            self.set_status(value);
                        }
                        _ => self.set_status("Unknown option"),
                    }
                } else {
                    self.set_status(
                        "Usage: :set findcross|nofindcross|shiftwidth=4|indentcolon|relativenumber|theme=dark",
                    );
                }
            }
            _ => {
                self.set_status(format!("Not an editor command: {}", cmd));
            }
        }

        Ok(false)
    }

    pub(super) fn execute_search(&mut self) -> Result<bool> {
        let pattern = self.command_buffer.clone();
        if pattern.is_empty() {
            return Ok(false);
        }
        if self
            .search_history
            .last()
            .map(|last| last != &pattern)
            .unwrap_or(true)
        {
            self.search_history.push(pattern.clone());
        }
        self.search_history_index = None;
        let reverse = matches!(self.command_prompt, CommandPrompt::SearchBackward);
        let found = if reverse {
            self.search_backward(&pattern)
        } else {
            self.search_forward(&pattern)
        };
        if !found {
            self.set_status(format!(
                "Pattern not found: {}{}",
                if reverse { "?" } else { "/" },
                pattern
            ));
        } else {
            self.last_search = Some(SearchSpec { pattern, reverse });
        }
        Ok(false)
    }
}
