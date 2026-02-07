use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::tui::event::FrameMetrics;
use crate::tui::state::{AppState, InputMode};

pub fn draw(frame: &mut Frame<'_>, state: &mut AppState) -> FrameMetrics {
    let root_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let status = Paragraph::new(state.status_line()).style(
        Style::default()
            .bg(Color::DarkGray)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(status, root_layout[0]);

    let main_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(root_layout[1]);

    let left_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(main_layout[0]);

    draw_search(frame, state, left_layout[0]);
    draw_endpoint_list(frame, state, left_layout[1]);
    let metrics = draw_endpoint_details(frame, state, main_layout[1]);

    let help = Paragraph::new(help_text(state.input_mode())).style(
        Style::default()
            .bg(Color::DarkGray)
            .fg(Color::White)
            .add_modifier(Modifier::DIM),
    );
    frame.render_widget(help, root_layout[2]);

    metrics
}

fn draw_search(frame: &mut Frame<'_>, state: &AppState, area: ratatui::layout::Rect) {
    let search_title = match state.input_mode() {
        InputMode::Normal => "Search (/ or Ctrl+s)",
        InputMode::Search => "Search (typing)",
    };
    let search_block = Block::default().borders(Borders::ALL).title(search_title);
    let search_text = if state.search_query().is_empty() {
        "<empty>".to_owned()
    } else {
        state.search_query().to_owned()
    };
    frame.render_widget(Paragraph::new(search_text).block(search_block), area);
}

fn draw_endpoint_list(frame: &mut Frame<'_>, state: &AppState, area: ratatui::layout::Rect) {
    let title = format!(
        "Endpoints ({}/{})",
        state.filtered_count(),
        state.endpoint_count()
    );

    if state.filtered_indices().is_empty() {
        let empty = Paragraph::new("No endpoints match the current filter.")
            .block(Block::default().title(title).borders(Borders::ALL));
        frame.render_widget(empty, area);
        return;
    }

    let items = state
        .filtered_indices()
        .iter()
        .map(|index| ListItem::new(state.endpoint_label(*index).to_owned()))
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(Block::default().title(title).borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    let mut list_state = ListState::default();
    list_state.select(state.selected_filtered_index());
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn draw_endpoint_details(
    frame: &mut Frame<'_>,
    state: &mut AppState,
    area: ratatui::layout::Rect,
) -> FrameMetrics {
    let title = format!("Details ({})", state.context().source_label);
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    let detail_width = inner.width.max(1);
    let detail_height = inner.height.max(1);
    let scroll = state.detail_scroll().min(u16::MAX as usize) as u16;

    let lines = {
        let rendered = state.detail_lines_for_selected(detail_width);
        rendered
            .iter()
            .map(|line| Line::raw(line.as_str()))
            .collect::<Vec<_>>()
    };

    let detail = Paragraph::new(lines)
        .block(block)
        .scroll((scroll, 0))
        .style(Style::default().fg(Color::White));
    frame.render_widget(detail, area);

    FrameMetrics {
        detail_width,
        detail_height,
    }
}

fn help_text(mode: InputMode) -> &'static str {
    match mode {
        InputMode::Normal => {
            "q quit | j/k or up/down move | g/G first/last | PgUp/PgDn jump | h/l or left/right scroll details | / or Ctrl+s search | Ctrl+u clear filter"
        }
        InputMode::Search => {
            "Search mode: type to filter | Backspace delete | Ctrl+u clear | Enter or Esc to return"
        }
    }
}
