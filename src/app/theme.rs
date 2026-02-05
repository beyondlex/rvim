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
    pub(crate) syntax_keyword: Color,
    pub(crate) syntax_string: Color,
    pub(crate) syntax_comment: Color,
    pub(crate) syntax_function: Color,
    pub(crate) syntax_type: Color,
    pub(crate) syntax_constant: Color,
    pub(crate) syntax_number: Color,
    pub(crate) syntax_operator: Color,
    pub(crate) syntax_property: Color,
    pub(crate) syntax_variable: Color,
    pub(crate) syntax_macro: Color,
    pub(crate) syntax_attribute: Color,
    pub(crate) syntax_punctuation: Color,
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
            syntax_keyword: Color::Blue,
            syntax_string: Color::Green,
            syntax_comment: Color::DarkGray,
            syntax_function: Color::Rgb(0, 102, 204),
            syntax_type: Color::Rgb(0, 128, 128),
            syntax_constant: Color::Rgb(153, 51, 102),
            syntax_number: Color::Rgb(204, 102, 0),
            syntax_operator: Color::Rgb(96, 96, 96),
            syntax_property: Color::Rgb(0, 102, 153),
            syntax_variable: Color::Rgb(0, 0, 0),
            syntax_macro: Color::Rgb(128, 0, 128),
            syntax_attribute: Color::Rgb(153, 76, 0),
            syntax_punctuation: Color::Rgb(80, 80, 80),
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
            syntax_keyword: Color::Rgb(86, 156, 214),
            syntax_string: Color::Rgb(106, 153, 85),
            syntax_comment: Color::Rgb(106, 153, 85),
            syntax_function: Color::Rgb(220, 220, 170),
            syntax_type: Color::Rgb(78, 201, 176),
            syntax_constant: Color::Rgb(86, 156, 214),
            syntax_number: Color::Rgb(181, 206, 168),
            syntax_operator: Color::Rgb(212, 212, 212),
            syntax_property: Color::Rgb(156, 220, 254),
            syntax_variable: Color::Rgb(212, 212, 212),
            syntax_macro: Color::Rgb(197, 134, 192),
            syntax_attribute: Color::Rgb(214, 157, 133),
            syntax_punctuation: Color::Rgb(212, 212, 212),
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
            syntax_keyword: Color::Rgb(38, 139, 210),
            syntax_string: Color::Rgb(42, 161, 152),
            syntax_comment: Color::Rgb(147, 161, 161),
            syntax_function: Color::Rgb(38, 139, 210),
            syntax_type: Color::Rgb(181, 137, 0),
            syntax_constant: Color::Rgb(211, 54, 130),
            syntax_number: Color::Rgb(203, 75, 22),
            syntax_operator: Color::Rgb(88, 110, 117),
            syntax_property: Color::Rgb(38, 139, 210),
            syntax_variable: Color::Rgb(101, 123, 131),
            syntax_macro: Color::Rgb(211, 54, 130),
            syntax_attribute: Color::Rgb(133, 153, 0),
            syntax_punctuation: Color::Rgb(88, 110, 117),
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
