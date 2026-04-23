use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Theme {
    pub mode: ThemeMode,
    pub base: Color,
    pub surface: Color,
    pub overlay: Color,
    pub muted: Color,
    pub text: Color,
    pub iris: Color,
    pub rose: Color,
    pub foam: Color,
    pub pine: Color,
    pub gold: Color,
    pub love: Color,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            base: Color::Rgb(25, 23, 36),     // #191724
            surface: Color::Rgb(31, 29, 46),  // #1f1d2e
            overlay: Color::Rgb(38, 35, 58),  // #26233a
            muted: Color::Rgb(110, 106, 134), // #6e6a86
            text: Color::Rgb(224, 222, 244),  // #e0def4
            iris: Color::Rgb(196, 167, 231),  // #c4a7e7
            rose: Color::Rgb(235, 188, 186),  // #ebbcba
            foam: Color::Rgb(156, 207, 216),  // #9ccfd8
            pine: Color::Rgb(49, 116, 143),   // #31748f
            gold: Color::Rgb(246, 193, 119),  // #f6c177
            love: Color::Rgb(235, 111, 146),  // #eb6f92
        }
    }

    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            base: Color::Rgb(250, 244, 237),    // #faf4ed
            surface: Color::Rgb(255, 250, 243), // #fffaf3
            overlay: Color::Rgb(242, 233, 225), // #f2e9e1
            muted: Color::Rgb(152, 147, 165),   // #9893a5
            text: Color::Rgb(87, 82, 121),      // #575279
            iris: Color::Rgb(144, 122, 169),    // #907aa9
            rose: Color::Rgb(215, 130, 126),    // #d7827e
            foam: Color::Rgb(86, 148, 159),     // #56949f
            pine: Color::Rgb(40, 105, 131),     // #286983
            gold: Color::Rgb(234, 157, 52),     // #ea9d34
            love: Color::Rgb(180, 99, 122),     // #b4637a
        }
    }

    pub fn from_option(theme_arg: Option<&str>) -> Self {
        match theme_arg {
            Some("light") => Self::light(),
            Some("dark") => Self::dark(),
            _ => Self::detect(),
        }
    }

    fn detect() -> Self {
        // OSC 10/11 query covers modern terminals (Alacritty, Kitty, WezTerm,
        // Ghostty, foot, GNOME/VTE, Terminal.app) where COLORFGBG is unset.
        // Falls back to Dark on detection failure (no TTY, query timeout,
        // unsupported terminal) — matches the prior behavior.
        use terminal_colorsaurus::{QueryOptions, ThemeMode as Detected, theme_mode};
        match theme_mode(QueryOptions::default()) {
            Ok(Detected::Light) => Self::light(),
            Ok(Detected::Dark) | Err(_) => Self::dark(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_theme_has_correct_base_color() {
        let theme = Theme::dark();
        assert_eq!(theme.mode, ThemeMode::Dark);
        assert_eq!(theme.base, Color::Rgb(25, 23, 36));
    }

    #[test]
    fn light_theme_has_correct_base_color() {
        let theme = Theme::light();
        assert_eq!(theme.mode, ThemeMode::Light);
        assert_eq!(theme.base, Color::Rgb(250, 244, 237));
    }

    #[test]
    fn from_option_respects_explicit_choice() {
        let dark = Theme::from_option(Some("dark"));
        assert_eq!(dark.mode, ThemeMode::Dark);

        let light = Theme::from_option(Some("light"));
        assert_eq!(light.mode, ThemeMode::Light);
    }
}
