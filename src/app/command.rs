use std::fs;
use std::path::PathBuf;

use anyhow::Result;

use super::types::{CommandPrompt, SearchSpec};
use super::App;

impl App {
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
        let input = self.command_buffer.trim();
        if input.is_empty() {
            return Ok(false);
        }

        let mut parts = input.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        let arg = parts.next();

        match cmd {
            "w" | "write" => {
                self.save()?;
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
                self.save()?;
                return Ok(true);
            }
            "e" | "edit" => {
                if let Some(path) = arg.map(PathBuf::from) {
                    self.file_path = Some(path.clone());
                    self.reload(&path)?;
                    self.set_status(format!("Opened {}", path.display()));
                } else {
                    self.set_status("Usage: :e <path>");
                }
            }
            "set" => {
                if let Some(setting) = arg {
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
                        "Usage: :set findcross|nofindcross|shiftwidth=4|indentcolon|relativenumber",
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
