use ratatui::prelude::Color;

#[derive(Debug, Clone)]
pub struct Theme {
    pub(crate) status_fg: Color,
    pub(crate) status_bg: Color,
    pub(crate) line_number_fg: Color,
    pub(crate) line_number_fg_current: Color,
    pub(crate) current_line_bg: Color,
    pub(crate) selection_fg: Color,
    pub(crate) selection_bg: Color,
    pub(crate) search_fg: Color,
    pub(crate) search_bg: Color,
}

impl Theme {
    pub(crate) fn default_theme() -> Self {
        Self::light()
    }

    pub(crate) fn light() -> Self {
        Self {
            status_fg: Color::Black,
            status_bg: Color::White,
            line_number_fg: Color::DarkGray,
            line_number_fg_current: Color::Rgb(255, 165, 0),
            current_line_bg: Color::Rgb(64, 64, 64),
            selection_fg: Color::Black,
            selection_bg: Color::Cyan,
            search_fg: Color::Black,
            search_bg: Color::Yellow,
        }
    }

    pub(crate) fn dark() -> Self {
        Self {
            status_fg: Color::White,
            status_bg: Color::Rgb(32, 32, 32),
            line_number_fg: Color::DarkGray,
            line_number_fg_current: Color::Rgb(255, 165, 0),
            current_line_bg: Color::Rgb(48, 48, 48),
            selection_fg: Color::Black,
            selection_bg: Color::Rgb(102, 153, 204),
            search_fg: Color::Black,
            search_bg: Color::Rgb(255, 211, 105),
        }
    }

    pub(crate) fn solarized() -> Self {
        Self {
            status_fg: Color::Rgb(101, 123, 131),
            status_bg: Color::Rgb(253, 246, 227),
            line_number_fg: Color::Rgb(147, 161, 161),
            line_number_fg_current: Color::Rgb(203, 75, 22),
            current_line_bg: Color::Rgb(238, 232, 213),
            selection_fg: Color::Rgb(7, 54, 66),
            selection_bg: Color::Rgb(147, 161, 161),
            search_fg: Color::Rgb(7, 54, 66),
            search_bg: Color::Rgb(181, 137, 0),
        }
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "light" => Some(Self::light()),
            "dark" => Some(Self::dark()),
            "solarized" | "solarized-light" => Some(Self::solarized()),
            _ => None,
        }
    }
}
