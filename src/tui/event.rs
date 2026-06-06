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
    let Some(action) = map_key(state.input_mode(), state.focus_panel(), key) else {
        return;
    };

    match action {
        Action::Quit => state.request_quit(),
        Action::TreeMoveUp => state.move_selection_up(1),
        Action::TreeMoveDown => state.move_selection_down(1),
        Action::TreeSelectFirst => state.select_first(),
        Action::TreeSelectLast => state.select_last(),
        Action::TreePageUp => state.page_up(),
        Action::TreePageDown => state.page_down(),
        Action::TreeToggleGroup => state.toggle_selected_group(),
        Action::TreeActivate => {
            if state.activate_selected_tree_row() {
                state.focus_details_panel();
            }
        }
        Action::DetailScrollUp => state.scroll_detail_up(1),
        Action::DetailScrollDown => {
            state.scroll_detail_down(1, metrics.detail_height, metrics.detail_width)
        }
        Action::DetailMoveUp => {
            state.move_detail_row_up(1, metrics.detail_height, metrics.detail_width)
        }
        Action::DetailMoveDown => {
            state.move_detail_row_down(1, metrics.detail_height, metrics.detail_width)
        }
        Action::DetailPageUp => state.scroll_detail_up(12),
        Action::DetailPageDown => {
            state.scroll_detail_down(12, metrics.detail_height, metrics.detail_width)
        }
        Action::DetailNextSection => state.cycle_detail_section(),
        Action::DetailToggle => state.toggle_detail_item(),
        Action::FocusTree => state.focus_tree_panel(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::metadata::CacheState;
    use crate::spec::index::build_endpoint_index;
    use crate::tui::state::{FocusPanel, TuiContext};
    use oas3::Spec;

    fn parse_spec(yaml: &str) -> Spec {
        serde_yaml::from_str::<Spec>(yaml).unwrap()
    }

    fn demo_state() -> AppState {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths:
  /pets:
    get:
      responses:
        "200":
          description: ok
"#,
        );
        AppState::new(
            TuiContext {
                source_label: "demo".to_owned(),
                cache_state: CacheState::Fresh,
                cached_at_utc: None,
                openapi_version: "3.1.0".to_owned(),
            },
            build_endpoint_index(&spec),
        )
    }

    #[test]
    fn enter_on_tree_endpoint_switches_focus_to_details() {
        let mut state = demo_state();
        assert_eq!(state.focus_panel(), FocusPanel::Tree);

        handle_key_event(
            &mut state,
            KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ),
            FrameMetrics {
                detail_width: 80,
                detail_height: 20,
            },
        );

        assert_eq!(state.focus_panel(), FocusPanel::Details);
    }

    #[test]
    fn esc_in_details_returns_focus_to_tree() {
        let mut state = demo_state();
        state.focus_details_panel();

        handle_key_event(
            &mut state,
            KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::NONE,
            ),
            FrameMetrics {
                detail_width: 80,
                detail_height: 20,
            },
        );

        assert_eq!(state.focus_panel(), FocusPanel::Tree);
    }
}
