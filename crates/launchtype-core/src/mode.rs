/// UI modes, one per data domain. A mode is entered by typing its trigger
/// character into the empty input field ('.' returns to Commands).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UiMode {
    #[default]
    Commands,
    Snippets,
    Clipboard,
    Steam,
    Screenshots,
    Timers,
    Alarms,
    Notebrook,
    Realtime,
    Stats,
    /// Remote shell over SSH: the input field holds the command, the results
    /// list holds the output lines.
    Ssh,
    /// Entered programmatically after "explore regions" analysis, not by a
    /// trigger character: lists the AI-detected regions of the last screenshot.
    Regions,
}

impl UiMode {
    /// The mode selected by typing `c` as the first character of the input
    /// field, or `None` if `c` is not a trigger character.
    pub fn from_trigger_char(c: char) -> Option<UiMode> {
        Some(match c {
            '-' => UiMode::Snippets,
            '?' => UiMode::Clipboard,
            '.' => UiMode::Commands,
            ',' => UiMode::Steam,
            '\'' => UiMode::Screenshots,
            '[' => UiMode::Timers,
            ']' => UiMode::Alarms,
            '#' => UiMode::Notebrook,
            '+' => UiMode::Realtime,
            '!' => UiMode::Stats,
            '$' => UiMode::Ssh,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_chars_map_to_modes() {
        assert_eq!(UiMode::from_trigger_char('-'), Some(UiMode::Snippets));
        assert_eq!(UiMode::from_trigger_char('?'), Some(UiMode::Clipboard));
        assert_eq!(UiMode::from_trigger_char('.'), Some(UiMode::Commands));
        assert_eq!(UiMode::from_trigger_char(','), Some(UiMode::Steam));
        assert_eq!(UiMode::from_trigger_char('\''), Some(UiMode::Screenshots));
        assert_eq!(UiMode::from_trigger_char('['), Some(UiMode::Timers));
        assert_eq!(UiMode::from_trigger_char(']'), Some(UiMode::Alarms));
        assert_eq!(UiMode::from_trigger_char('#'), Some(UiMode::Notebrook));
        assert_eq!(UiMode::from_trigger_char('+'), Some(UiMode::Realtime));
        assert_eq!(UiMode::from_trigger_char('!'), Some(UiMode::Stats));
        assert_eq!(UiMode::from_trigger_char('$'), Some(UiMode::Ssh));
        assert_eq!(UiMode::from_trigger_char('a'), None);
        assert_eq!(UiMode::from_trigger_char(' '), None);
    }
}
