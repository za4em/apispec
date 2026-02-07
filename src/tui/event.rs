use crossterm::event::{self, Event, KeyEvent};

use crate::error::AppError;
use crate::tui::keymap::{Action, map_key};
use crate::tui::state::AppState;

#[derive(Debug, Clone, Copy, Default)]
pub struct FrameMetrics {
    pub detail_width: u16,
    pub detail_height: u16,
}

pub fn process_next_event(state: &mut AppState, metrics: FrameMetrics) -> Result<(), AppError> {
    match event::read().map_err(tui_error)? {
        Event::Key(key_event) => {
            handle_key_event(state, key_event, metrics);
        }
        Event::Resize(_, _) => {
            state.clamp_detail_scroll(metrics.detail_height, metrics.detail_width);
        }
        _ => {}
    }

    Ok(())
}

fn handle_key_event(state: &mut AppState, key: KeyEvent, metrics: FrameMetrics) {
    let Some(action) = map_key(state.input_mode(), key) else {
        return;
    };

    match action {
        Action::Quit => state.request_quit(),
        Action::MoveSelectionUp => state.move_selection_up(1),
        Action::MoveSelectionDown => state.move_selection_down(1),
        Action::SelectFirst => state.select_first(),
        Action::SelectLast => state.select_last(),
        Action::PageUp => state.page_up(),
        Action::PageDown => state.page_down(),
        Action::ScrollDetailUp => state.scroll_detail_up(1),
        Action::ScrollDetailDown => {
            state.scroll_detail_down(1, metrics.detail_height, metrics.detail_width)
        }
        Action::EnterSearch => state.enter_search_mode(),
        Action::ExitSearch | Action::SubmitSearch => state.exit_search_mode(),
        Action::BackspaceSearch => state.pop_search_char(),
        Action::ClearSearch => state.clear_search(),
        Action::AppendSearchChar(ch) => state.push_search_char(ch),
    }

    state.clamp_detail_scroll(metrics.detail_height, metrics.detail_width);
}

fn tui_error(source: std::io::Error) -> AppError {
    AppError::TuiRuntime { source }
}
