use ratatui::style::{Color, Modifier, Style};

/// Application theme with all configurable colors and styles.
/// Default theme matches Discord dark mode colors.
#[derive(Debug, Clone)]
pub struct Theme {
    // Base colors
    pub bg: Color,
    pub fg: Color,
    pub fg_dim: Color,

    // Borders
    pub border_active: Color,
    pub border_inactive: Color,

    // Sidebar
    pub sidebar_bg: Color,
    pub sidebar_fg: Color,
    pub sidebar_selected_bg: Color,
    pub sidebar_selected_fg: Color,
    pub sidebar_category_fg: Color,
    pub sidebar_unread_fg: Color,
    pub sidebar_mention_fg: Color,

    // Messages
    pub message_author_fg: Color,
    pub message_timestamp_fg: Color,
    pub message_content_fg: Color,
    pub message_selected_bg: Color,
    pub message_edited_fg: Color,
    pub message_system_fg: Color,

    // Input
    pub input_bg: Color,
    pub input_fg: Color,
    pub input_cursor_fg: Color,
    pub input_placeholder_fg: Color,

    // Status bar
    pub status_bar_bg: Color,
    pub status_bar_fg: Color,
    pub status_connected_fg: Color,
    pub status_connecting_fg: Color,
    pub status_disconnected_fg: Color,
    pub status_mode_fg: Color,

    // Mentions / highlights
    pub mention_fg: Color,
    pub highlight_bg: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            // Discord dark mode base
            bg: Color::Rgb(54, 57, 63),
            fg: Color::Rgb(220, 221, 222),
            fg_dim: Color::Rgb(142, 146, 151),

            // Borders
            border_active: Color::Cyan,
            border_inactive: Color::DarkGray,

            // Sidebar (Discord channel list style)
            sidebar_bg: Color::Rgb(47, 49, 54),
            sidebar_fg: Color::Rgb(142, 146, 151),
            sidebar_selected_bg: Color::Rgb(66, 70, 77),
            sidebar_selected_fg: Color::White,
            sidebar_category_fg: Color::Rgb(142, 146, 151),
            sidebar_unread_fg: Color::White,
            sidebar_mention_fg: Color::Red,

            // Messages
            message_author_fg: Color::White,
            message_timestamp_fg: Color::Rgb(114, 118, 125),
            message_content_fg: Color::Rgb(220, 221, 222),
            message_selected_bg: Color::Rgb(66, 70, 77),
            message_edited_fg: Color::Rgb(114, 118, 125),
            message_system_fg: Color::Rgb(142, 146, 151),

            // Input
            input_bg: Color::Rgb(64, 68, 75),
            input_fg: Color::Rgb(220, 221, 222),
            input_cursor_fg: Color::White,
            input_placeholder_fg: Color::Rgb(114, 118, 125),

            // Status bar
            status_bar_bg: Color::Rgb(32, 34, 37),
            status_bar_fg: Color::Rgb(185, 187, 190),
            status_connected_fg: Color::Green,
            status_connecting_fg: Color::Yellow,
            status_disconnected_fg: Color::Red,
            status_mode_fg: Color::Cyan,

            // Mentions / highlights
            mention_fg: Color::Rgb(250, 166, 26),
            highlight_bg: Color::Rgb(68, 36, 16),
        }
    }
}

impl Theme {
    /// Apply custom border colors from config strings.
    pub fn with_border_colors(mut self, active: &str, inactive: &str) -> Self {
        self.border_active = parse_color(active).unwrap_or(self.border_active);
        self.border_inactive = parse_color(inactive).unwrap_or(self.border_inactive);
        self
    }

    // Style helpers for common use

    pub fn base_style(&self) -> Style {
        Style::default().fg(self.fg).bg(self.bg)
    }

    pub fn dim_style(&self) -> Style {
        Style::default().fg(self.fg_dim)
    }

    pub fn active_border_style(&self) -> Style {
        Style::default().fg(self.border_active)
    }

    pub fn inactive_border_style(&self) -> Style {
        Style::default().fg(self.border_inactive)
    }

    pub fn status_bar_style(&self) -> Style {
        Style::default().fg(self.status_bar_fg).bg(self.status_bar_bg)
    }

    pub fn sidebar_style(&self) -> Style {
        Style::default().fg(self.sidebar_fg).bg(self.sidebar_bg)
    }

    pub fn sidebar_selected_style(&self) -> Style {
        Style::default()
            .fg(self.sidebar_selected_fg)
            .bg(self.sidebar_selected_bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn message_author_style(&self) -> Style {
        Style::default()
            .fg(self.message_author_fg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn message_timestamp_style(&self) -> Style {
        Style::default().fg(self.message_timestamp_fg)
    }

    pub fn input_style(&self) -> Style {
        Style::default().fg(self.input_fg).bg(self.input_bg)
    }
}

/// Parse a color name string into a ratatui Color.
fn parse_color(name: &str) -> Option<Color> {
    match name.to_lowercase().as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "white" => Some(Color::White),
        "gray" | "grey" => Some(Color::Gray),
        "dark_gray" | "dark_grey" | "darkgray" | "darkgrey" => Some(Color::DarkGray),
        "light_red" | "lightred" => Some(Color::LightRed),
        "light_green" | "lightgreen" => Some(Color::LightGreen),
        "light_yellow" | "lightyellow" => Some(Color::LightYellow),
        "light_blue" | "lightblue" => Some(Color::LightBlue),
        "light_magenta" | "lightmagenta" => Some(Color::LightMagenta),
        "light_cyan" | "lightcyan" => Some(Color::LightCyan),
        _ => {
            // Try hex: #RRGGBB
            if name.starts_with('#') && name.len() == 7 {
                let r = u8::from_str_radix(&name[1..3], 16).ok()?;
                let g = u8::from_str_radix(&name[3..5], 16).ok()?;
                let b = u8::from_str_radix(&name[5..7], 16).ok()?;
                Some(Color::Rgb(r, g, b))
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_has_discord_colors() {
        let theme = Theme::default();
        // Discord dark mode background is approximately RGB(54, 57, 63)
        assert_eq!(theme.bg, Color::Rgb(54, 57, 63));
        assert_eq!(theme.fg, Color::Rgb(220, 221, 222));
        assert_eq!(theme.border_active, Color::Cyan);
        assert_eq!(theme.border_inactive, Color::DarkGray);
    }

    #[test]
    fn custom_border_colors_override() {
        let theme = Theme::default().with_border_colors("green", "dark_gray");
        assert_eq!(theme.border_active, Color::Green);
        assert_eq!(theme.border_inactive, Color::DarkGray);
    }

    #[test]
    fn parse_color_named() {
        assert_eq!(parse_color("red"), Some(Color::Red));
        assert_eq!(parse_color("cyan"), Some(Color::Cyan));
        assert_eq!(parse_color("gray"), Some(Color::Gray));
        assert_eq!(parse_color("dark_gray"), Some(Color::DarkGray));
        assert_eq!(parse_color("White"), Some(Color::White));
    }

    #[test]
    fn parse_color_hex() {
        assert_eq!(parse_color("#FF0000"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_color("#00FF00"), Some(Color::Rgb(0, 255, 0)));
        assert_eq!(parse_color("#363945"), Some(Color::Rgb(54, 57, 69)));
    }

    #[test]
    fn parse_color_invalid_returns_none() {
        assert_eq!(parse_color("notacolor"), None);
        assert_eq!(parse_color("#GGGGGG"), None);
        assert_eq!(parse_color("#12"), None);
        assert_eq!(parse_color(""), None);
    }

    #[test]
    fn invalid_border_color_keeps_default() {
        let theme = Theme::default().with_border_colors("notacolor", "alsonotacolor");
        assert_eq!(theme.border_active, Color::Cyan);
        assert_eq!(theme.border_inactive, Color::DarkGray);
    }

    #[test]
    fn style_helpers_return_correct_styles() {
        let theme = Theme::default();
        let base = theme.base_style();
        assert_eq!(base.fg, Some(theme.fg));
        assert_eq!(base.bg, Some(theme.bg));

        let status = theme.status_bar_style();
        assert_eq!(status.fg, Some(theme.status_bar_fg));
        assert_eq!(status.bg, Some(theme.status_bar_bg));

        let sidebar = theme.sidebar_style();
        assert_eq!(sidebar.fg, Some(theme.sidebar_fg));
        assert_eq!(sidebar.bg, Some(theme.sidebar_bg));

        let author = theme.message_author_style();
        assert_eq!(author.fg, Some(theme.message_author_fg));
        assert!(author.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn sidebar_selected_style_is_bold() {
        let theme = Theme::default();
        let selected = theme.sidebar_selected_style();
        assert!(selected.add_modifier.contains(Modifier::BOLD));
        assert_eq!(selected.fg, Some(theme.sidebar_selected_fg));
        assert_eq!(selected.bg, Some(theme.sidebar_selected_bg));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    // --- P6.1: valid hex #RRGGBB always parses ---
    proptest! {
        #[test]
        fn valid_hex_always_parses(r in 0u8..=255, g in 0u8..=255, b in 0u8..=255) {
            let hex = format!("#{:02X}{:02X}{:02X}", r, g, b);
            let result = parse_color(&hex);
            prop_assert_eq!(result, Some(Color::Rgb(r, g, b)), "Failed to parse {}", hex);
        }
    }

    // --- P6.2: case insensitive named colors ---
    proptest! {
        #[test]
        fn named_color_case_insensitive(
            name in prop_oneof![
                Just("red"), Just("green"), Just("blue"), Just("yellow"),
                Just("cyan"), Just("magenta"), Just("white"), Just("black"),
                Just("gray"), Just("grey"),
            ]
        ) {
            let lower = parse_color(name);
            let upper = parse_color(&name.to_uppercase());
            let mixed = parse_color(&{
                let mut s = String::new();
                for (i, c) in name.chars().enumerate() {
                    if i % 2 == 0 { s.extend(c.to_uppercase()); }
                    else { s.push(c); }
                }
                s
            });
            prop_assert_eq!(lower, upper, "Case mismatch: {:?} vs {:?}", name, name.to_uppercase());
            prop_assert_eq!(lower, mixed);
        }
    }
}
