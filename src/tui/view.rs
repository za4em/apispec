use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::tui::event::FrameMetrics;
use crate::tui::state::{AppState, FocusPanel, InputMode};
use crate::tui::tree::TreeRowKind;

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
    draw_endpoint_tree(frame, state, left_layout[1]);
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
    let border_style = if matches!(state.input_mode(), InputMode::Search) {
        focused_border_style()
    } else {
        unfocused_border_style()
    };
    let search_block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(search_title);
    let search_text = if state.search_query().is_empty() {
        "<empty>".to_owned()
    } else {
        state.search_query().to_owned()
    };
    frame.render_widget(Paragraph::new(search_text).block(search_block), area);
}

fn draw_endpoint_tree(frame: &mut Frame<'_>, state: &AppState, area: ratatui::layout::Rect) {
    let title = format!(
        "Tree ({}/{})",
        state.filtered_count(),
        state.endpoint_count()
    );
    let is_focused = state.focus_panel() == FocusPanel::Tree;
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(if is_focused {
            focused_border_style()
        } else {
            unfocused_border_style()
        });

    if state.tree_rows().is_empty() {
        let empty = Paragraph::new("No endpoints match the current filter.").block(block);
        frame.render_widget(empty, area);
        return;
    }

    let items = state
        .tree_rows()
        .iter()
        .map(|row| {
            let text = state.tree_row_display_label(row);
            let style = match row.kind {
                TreeRowKind::Group => Style::default().add_modifier(Modifier::BOLD),
                TreeRowKind::Endpoint => Style::default(),
            };
            ListItem::new(text).style(style)
        })
        .collect::<Vec<_>>();

    let highlight_style = if is_focused {
        Style::default()
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .bg(Color::DarkGray)
            .fg(Color::White)
            .add_modifier(Modifier::DIM)
    };

    let list = List::new(items)
        .block(block)
        .highlight_style(highlight_style)
        .highlight_symbol("> ");

    let mut list_state = ListState::default();
    list_state.select(state.selected_tree_row_index());
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn draw_endpoint_details(
    frame: &mut Frame<'_>,
    state: &mut AppState,
    area: ratatui::layout::Rect,
) -> FrameMetrics {
    let title = format!("Details ({})", state.context().source_label);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(if state.focus_panel() == FocusPanel::Details {
            focused_border_style()
        } else {
            unfocused_border_style()
        });
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

fn focused_border_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

fn unfocused_border_style() -> Style {
    Style::default().fg(Color::Gray)
}

fn help_text(mode: InputMode) -> &'static str {
    match mode {
        InputMode::Normal => {
            "q quit | Tree: j/k move g/G first/last PgUp/PgDn jump Enter open Right toggle group | Details: j/k scroll Tab next section Enter toggle Esc back | / or Ctrl+s search | Ctrl+u clear"
        }
        InputMode::Search => {
            "Search mode: type to filter | Backspace delete | Ctrl+u clear | Enter or Esc to return"
        }
    }
}
