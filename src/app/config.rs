use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use ratatui::prelude::Color;
use serde::Deserialize;

use super::theme::Theme;

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    pub(crate) theme: Option<String>,
    pub(crate) themes: Option<HashMap<String, ThemeOverride>>,
}

pub fn load_config() -> Result<Config> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    candidates.push(PathBuf::from("rvim.toml"));
    candidates.push(PathBuf::from(".rvim.toml"));
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(PathBuf::from(home).join(".config/rvim/config.toml"));
    }

    for path in candidates {
        if !path.exists() {
            continue;
        }
        let content = fs::read_to_string(&path)?;
        let cfg: Config = toml::from_str(&content)?;
        return Ok(cfg);
    }
    Ok(Config::default())
}

#[derive(Debug, Deserialize, Default, Clone)]
pub(crate) struct ThemeOverride {
    pub(crate) status_fg: Option<String>,
    pub(crate) status_bg: Option<String>,
    pub(crate) line_number_fg: Option<String>,
    pub(crate) line_number_fg_current: Option<String>,
    pub(crate) current_line_bg: Option<String>,
    pub(crate) selection_fg: Option<String>,
    pub(crate) selection_bg: Option<String>,
    pub(crate) search_fg: Option<String>,
    pub(crate) search_bg: Option<String>,
    pub(crate) syntax_keyword: Option<String>,
    pub(crate) syntax_string: Option<String>,
    pub(crate) syntax_comment: Option<String>,
    pub(crate) syntax_function: Option<String>,
    pub(crate) syntax_type: Option<String>,
    pub(crate) syntax_constant: Option<String>,
    pub(crate) syntax_number: Option<String>,
    pub(crate) syntax_operator: Option<String>,
    pub(crate) syntax_property: Option<String>,
    pub(crate) syntax_variable: Option<String>,
    pub(crate) syntax_macro: Option<String>,
    pub(crate) syntax_attribute: Option<String>,
    pub(crate) syntax_punctuation: Option<String>,
}

pub(crate) fn apply_theme_overrides(theme: &mut Theme, overrides: &ThemeOverride) {
    if let Some(color) = overrides.status_fg.as_deref().and_then(parse_color) {
        theme.status_fg = color;
    }
    if let Some(color) = overrides.status_bg.as_deref().and_then(parse_color) {
        theme.status_bg = color;
    }
    if let Some(color) = overrides.line_number_fg.as_deref().and_then(parse_color) {
        theme.line_number_fg = color;
    }
    if let Some(color) = overrides
        .line_number_fg_current
        .as_deref()
        .and_then(parse_color)
    {
        theme.line_number_fg_current = color;
    }
    if let Some(color) = overrides
        .current_line_bg
        .as_deref()
        .and_then(parse_color)
    {
        theme.current_line_bg = color;
    }
    if let Some(color) = overrides.selection_fg.as_deref().and_then(parse_color) {
        theme.selection_fg = color;
    }
    if let Some(color) = overrides.selection_bg.as_deref().and_then(parse_color) {
        theme.selection_bg = color;
    }
    if let Some(color) = overrides.search_fg.as_deref().and_then(parse_color) {
        theme.search_fg = color;
    }
    if let Some(color) = overrides.search_bg.as_deref().and_then(parse_color) {
        theme.search_bg = color;
    }
    if let Some(color) = overrides.syntax_keyword.as_deref().and_then(parse_color) {
        theme.syntax_keyword = color;
    }
    if let Some(color) = overrides.syntax_string.as_deref().and_then(parse_color) {
        theme.syntax_string = color;
    }
    if let Some(color) = overrides.syntax_comment.as_deref().and_then(parse_color) {
        theme.syntax_comment = color;
    }
    if let Some(color) = overrides.syntax_function.as_deref().and_then(parse_color) {
        theme.syntax_function = color;
    }
    if let Some(color) = overrides.syntax_type.as_deref().and_then(parse_color) {
        theme.syntax_type = color;
    }
    if let Some(color) = overrides.syntax_constant.as_deref().and_then(parse_color) {
        theme.syntax_constant = color;
    }
    if let Some(color) = overrides.syntax_number.as_deref().and_then(parse_color) {
        theme.syntax_number = color;
    }
    if let Some(color) = overrides.syntax_operator.as_deref().and_then(parse_color) {
        theme.syntax_operator = color;
    }
    if let Some(color) = overrides.syntax_property.as_deref().and_then(parse_color) {
        theme.syntax_property = color;
    }
    if let Some(color) = overrides.syntax_variable.as_deref().and_then(parse_color) {
        theme.syntax_variable = color;
    }
    if let Some(color) = overrides.syntax_macro.as_deref().and_then(parse_color) {
        theme.syntax_macro = color;
    }
    if let Some(color) = overrides.syntax_attribute.as_deref().and_then(parse_color) {
        theme.syntax_attribute = color;
    }
    if let Some(color) = overrides.syntax_punctuation.as_deref().and_then(parse_color) {
        theme.syntax_punctuation = color;
    }
}

fn parse_color(value: &str) -> Option<Color> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}
