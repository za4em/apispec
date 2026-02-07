use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::state::{FocusPanel, InputMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    TreeMoveUp,
    TreeMoveDown,
    TreeSelectFirst,
    TreeSelectLast,
    TreePageUp,
    TreePageDown,
    TreeToggleGroup,
    TreeActivate,
    DetailScrollUp,
    DetailScrollDown,
    DetailPageUp,
    DetailPageDown,
    DetailNextSection,
    DetailToggle,
    FocusTree,
    EnterSearch,
    ExitSearch,
    SubmitSearch,
    BackspaceSearch,
    ClearSearch,
    AppendSearchChar(char),
}

pub fn map_key(mode: InputMode, focus: FocusPanel, key: KeyEvent) -> Option<Action> {
    match mode {
        InputMode::Normal => map_normal_key(focus, key),
        InputMode::Search => map_search_key(key),
    }
}

fn map_normal_key(focus: FocusPanel, key: KeyEvent) -> Option<Action> {
    if let Some(action) = map_global_normal_key(key) {
        return Some(action);
    }

    match focus {
        FocusPanel::Tree => map_tree_focus_key(key),
        FocusPanel::Details => map_details_focus_key(key),
    }
}

fn map_global_normal_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('q') if key.modifiers.is_empty() => Some(Action::Quit),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Action::Quit),
        KeyCode::Char('/') if key.modifiers.is_empty() => Some(Action::EnterSearch),
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::EnterSearch)
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::ClearSearch)
        }
        _ => None,
    }
}

fn map_tree_focus_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('j') if key.modifiers.is_empty() => Some(Action::TreeMoveDown),
        KeyCode::Char('k') if key.modifiers.is_empty() => Some(Action::TreeMoveUp),
        KeyCode::Char('g') if key.modifiers.is_empty() => Some(Action::TreeSelectFirst),
        KeyCode::Char('G')
            if key.modifiers.contains(KeyModifiers::SHIFT) || key.modifiers.is_empty() =>
        {
            Some(Action::TreeSelectLast)
        }
        KeyCode::Down => Some(Action::TreeMoveDown),
        KeyCode::Up => Some(Action::TreeMoveUp),
        KeyCode::PageUp => Some(Action::TreePageUp),
        KeyCode::PageDown => Some(Action::TreePageDown),
        KeyCode::Right | KeyCode::Left => Some(Action::TreeToggleGroup),
        KeyCode::Enter => Some(Action::TreeActivate),
        _ => None,
    }
}

fn map_details_focus_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('j') if key.modifiers.is_empty() => Some(Action::DetailScrollDown),
        KeyCode::Char('k') if key.modifiers.is_empty() => Some(Action::DetailScrollUp),
        KeyCode::Down => Some(Action::DetailScrollDown),
        KeyCode::Up => Some(Action::DetailScrollUp),
        KeyCode::PageUp => Some(Action::DetailPageUp),
        KeyCode::PageDown => Some(Action::DetailPageDown),
        KeyCode::Char('h') if key.modifiers.is_empty() => Some(Action::DetailScrollUp),
        KeyCode::Char('l') if key.modifiers.is_empty() => Some(Action::DetailScrollDown),
        KeyCode::Left => Some(Action::DetailScrollUp),
        KeyCode::Right => Some(Action::DetailScrollDown),
        KeyCode::Tab => Some(Action::DetailNextSection),
        KeyCode::Enter => Some(Action::DetailToggle),
        KeyCode::Esc => Some(Action::FocusTree),
        _ => None,
    }
}

fn map_search_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Action::Quit),
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::ClearSearch)
        }
        KeyCode::Esc => Some(Action::ExitSearch),
        KeyCode::Enter => Some(Action::SubmitSearch),
        KeyCode::Backspace => Some(Action::BackspaceSearch),
        KeyCode::Char(c)
            if key.modifiers.is_empty()
                || key.modifiers == KeyModifiers::SHIFT
                || (c == ' ' && key.modifiers.is_empty()) =>
        {
            Some(Action::AppendSearchChar(c))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    #[test]
    fn maps_tree_focus_keys_to_tree_actions() {
        assert_eq!(
            map_key(InputMode::Normal, FocusPanel::Tree, key(KeyCode::Down)),
            Some(Action::TreeMoveDown)
        );
        assert_eq!(
            map_key(InputMode::Normal, FocusPanel::Tree, key(KeyCode::Right)),
            Some(Action::TreeToggleGroup)
        );
        assert_eq!(
            map_key(InputMode::Normal, FocusPanel::Tree, key(KeyCode::Enter)),
            Some(Action::TreeActivate)
        );
    }

    #[test]
    fn maps_details_focus_keys_to_detail_actions() {
        assert_eq!(
            map_key(InputMode::Normal, FocusPanel::Details, key(KeyCode::Down)),
            Some(Action::DetailScrollDown)
        );
        assert_eq!(
            map_key(InputMode::Normal, FocusPanel::Details, key(KeyCode::Tab)),
            Some(Action::DetailNextSection)
        );
        assert_eq!(
            map_key(InputMode::Normal, FocusPanel::Details, key(KeyCode::Esc)),
            Some(Action::FocusTree)
        );
    }

    #[test]
    fn search_mode_remains_focus_agnostic() {
        assert_eq!(
            map_key(InputMode::Search, FocusPanel::Tree, key(KeyCode::Esc)),
            Some(Action::ExitSearch)
        );
        assert_eq!(
            map_key(
                InputMode::Search,
                FocusPanel::Details,
                key(KeyCode::Backspace)
            ),
            Some(Action::BackspaceSearch)
        );
    }
}
