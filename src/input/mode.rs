/// Input mode state machine.
/// Transitions: `i` → Insert, `Esc` → Normal, `:` → Command, `Ctrl+b` → `PanePrefix`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    /// Navigation mode (vim Normal). Keys are commands.
    #[default]
    Normal,
    /// Message composition mode (vim Insert). Keys are text input.
    Insert,
    /// Command line mode (`:` prefix). Keys go to command input.
    Command,
    /// Pane prefix mode (after Ctrl+b). Next key is a pane command.
    PanePrefix,
}

impl InputMode {
    /// Display name for the status bar.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Command => "COMMAND",
            Self::PanePrefix => "PANE",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mode_is_normal() {
        assert_eq!(InputMode::default(), InputMode::Normal);
    }

    #[test]
    fn display_names() {
        assert_eq!(InputMode::Normal.display_name(), "NORMAL");
        assert_eq!(InputMode::Insert.display_name(), "INSERT");
        assert_eq!(InputMode::Command.display_name(), "COMMAND");
        assert_eq!(InputMode::PanePrefix.display_name(), "PANE");
    }

    #[test]
    fn modes_are_copy() {
        let mode = InputMode::Normal;
        let copy = mode;
        assert_eq!(mode, copy);
    }

    #[test]
    fn all_variants_are_distinct() {
        let modes = [
            InputMode::Normal,
            InputMode::Insert,
            InputMode::Command,
            InputMode::PanePrefix,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
