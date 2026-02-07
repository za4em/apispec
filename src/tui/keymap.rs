use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::state::InputMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    MoveSelectionUp,
    MoveSelectionDown,
    SelectFirst,
    SelectLast,
    PageUp,
    PageDown,
    ScrollDetailUp,
    ScrollDetailDown,
    EnterSearch,
    ExitSearch,
    SubmitSearch,
    BackspaceSearch,
    ClearSearch,
    AppendSearchChar(char),
}

pub fn map_key(mode: InputMode, key: KeyEvent) -> Option<Action> {
    match mode {
        InputMode::Normal => map_normal_key(key),
        InputMode::Search => map_search_key(key),
    }
}

fn map_normal_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('q') if key.modifiers.is_empty() => Some(Action::Quit),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Action::Quit),
        KeyCode::Char('j') if key.modifiers.is_empty() => Some(Action::MoveSelectionDown),
        KeyCode::Char('k') if key.modifiers.is_empty() => Some(Action::MoveSelectionUp),
        KeyCode::Char('g') if key.modifiers.is_empty() => Some(Action::SelectFirst),
        KeyCode::Char('G')
            if key.modifiers.contains(KeyModifiers::SHIFT) || key.modifiers.is_empty() =>
        {
            Some(Action::SelectLast)
        }
        KeyCode::Down => Some(Action::MoveSelectionDown),
        KeyCode::Up => Some(Action::MoveSelectionUp),
        KeyCode::PageUp => Some(Action::PageUp),
        KeyCode::PageDown => Some(Action::PageDown),
        KeyCode::Char('h') if key.modifiers.is_empty() => Some(Action::ScrollDetailUp),
        KeyCode::Char('l') if key.modifiers.is_empty() => Some(Action::ScrollDetailDown),
        KeyCode::Left => Some(Action::ScrollDetailUp),
        KeyCode::Right => Some(Action::ScrollDetailDown),
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
