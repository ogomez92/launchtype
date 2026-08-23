/// UI modes, one per data domain. A mode is entered by typing its trigger
/// character into the empty input field ('.' returns to Commands).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UiMode {
    #[default]
    Commands,
    Snippets,
    Clipboard,
    Steam,
    /// Every application installed on this machine, as the OS itself lists
    /// them: Start Menu entries and Store apps on Windows, indexed application
    /// bundles on macOS.
    Apps,
    Screenshots,
    Timers,
    Alarms,
    Notebrook,
    Realtime,
    Stats,
    /// Remote shell over SSH: the input field holds the command, the results
    /// list holds the output lines.
    Ssh,
    /// Pick an emoji by the name a screen reader gives it, and copy it.
    Emoji,
    /// Convert a typed number between units: feet to centimeters, Celsius to
    /// Fahrenheit, shoe sizes between countries.
    Units,
    /// Passwords and other secrets, encrypted at rest behind a master
    /// password and only ever decrypted into memory.
    Vault,
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
            '@' => UiMode::Apps,
            '\'' => UiMode::Screenshots,
            '[' => UiMode::Timers,
            ']' => UiMode::Alarms,
            '#' => UiMode::Notebrook,
            '+' => UiMode::Realtime,
            '!' => UiMode::Stats,
            '$' => UiMode::Ssh,
            ':' => UiMode::Emoji,
            '=' => UiMode::Units,
            '*' => UiMode::Vault,
            _ => return None,
        })
    }

    /// The trigger character that enters this mode, or `None` for modes that
    /// are only reachable programmatically (Regions).
    pub fn trigger_char(self) -> Option<char> {
        Some(match self {
            UiMode::Snippets => '-',
            UiMode::Clipboard => '?',
            UiMode::Commands => '.',
            UiMode::Steam => ',',
            UiMode::Apps => '@',
            UiMode::Screenshots => '\'',
            UiMode::Timers => '[',
            UiMode::Alarms => ']',
            UiMode::Notebrook => '#',
            UiMode::Realtime => '+',
            UiMode::Stats => '!',
            UiMode::Ssh => '$',
            UiMode::Emoji => ':',
            UiMode::Units => '=',
            UiMode::Vault => '*',
            UiMode::Regions => return None,
        })
    }

    /// Every user-selectable mode, in the order shown by the modes menu. Kept
    /// in sync with [`from_trigger_char`]; Regions is excluded (no trigger).
    pub const MENU_MODES: [UiMode; 15] = [
        UiMode::Commands,
        UiMode::Snippets,
        UiMode::Clipboard,
        UiMode::Steam,
        UiMode::Apps,
        UiMode::Screenshots,
        UiMode::Timers,
        UiMode::Alarms,
        UiMode::Notebrook,
        UiMode::Realtime,
        UiMode::Stats,
        UiMode::Ssh,
        UiMode::Emoji,
        UiMode::Units,
        UiMode::Vault,
    ];
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
        assert_eq!(UiMode::from_trigger_char('@'), Some(UiMode::Apps));
        assert_eq!(UiMode::from_trigger_char('\''), Some(UiMode::Screenshots));
        assert_eq!(UiMode::from_trigger_char('['), Some(UiMode::Timers));
        assert_eq!(UiMode::from_trigger_char(']'), Some(UiMode::Alarms));
        assert_eq!(UiMode::from_trigger_char('#'), Some(UiMode::Notebrook));
        assert_eq!(UiMode::from_trigger_char('+'), Some(UiMode::Realtime));
        assert_eq!(UiMode::from_trigger_char('!'), Some(UiMode::Stats));
        assert_eq!(UiMode::from_trigger_char('$'), Some(UiMode::Ssh));
        assert_eq!(UiMode::from_trigger_char(':'), Some(UiMode::Emoji));
        assert_eq!(UiMode::from_trigger_char('='), Some(UiMode::Units));
        assert_eq!(UiMode::from_trigger_char('*'), Some(UiMode::Vault));
        assert_eq!(UiMode::from_trigger_char('a'), None);
        assert_eq!(UiMode::from_trigger_char(' '), None);
    }

    #[test]
    fn menu_modes_round_trip_through_their_trigger_char() {
        for mode in UiMode::MENU_MODES {
            let c = mode.trigger_char().expect("menu mode has a trigger char");
            assert_eq!(UiMode::from_trigger_char(c), Some(mode));
        }
        assert_eq!(UiMode::Regions.trigger_char(), None);
    }

    /// Two modes sharing a trigger would make one of them unreachable by
    /// typing, and only the round-trip above would notice — after the fact.
    #[test]
    fn every_trigger_char_is_unique() {
        let mut seen = Vec::new();
        for mode in UiMode::MENU_MODES {
            let c = mode.trigger_char().expect("menu mode has a trigger char");
            assert!(!seen.contains(&c), "{c:?} triggers more than one mode");
            seen.push(c);
        }
    }
}
