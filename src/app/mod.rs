mod command;
mod config;
mod edit;
mod highlight;
mod input;
mod motion;
mod theme;
mod types;

pub use input::handle_key;
pub use theme::Theme;
pub use highlight::{HighlightKind, SyntaxSpan, total_spans};
pub use types::{
    App, CommandPrompt, Mode, VisualSelection, VisualSelectionKind, char_display_width,
    char_to_screen_col, line_screen_width,
};
pub use config::load_config;
