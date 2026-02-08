use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::tui::details::DetailSection;
use crate::tui::event::FrameMetrics;
use crate::tui::state::{AppState, FocusPanel, InputMode};
use crate::tui::tree::{TreeRow, TreeRowKind};

const SECTION_HEADERS: [&str; 5] = [
    "Description",
    "Parameters",
    "Request Body",
    "Responses",
    "Security",
];
const HTTP_METHODS: [&str; 8] = [
    "GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD", "TRACE",
];

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

fn draw_search(frame: &mut Frame<'_>, state: &AppState, area: Rect) {
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

fn draw_endpoint_tree(frame: &mut Frame<'_>, state: &AppState, area: Rect) {
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
        .map(|row| ListItem::new(tree_row_line(state, row)))
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

fn tree_row_line(state: &AppState, row: &TreeRow) -> Line<'static> {
    match row.kind {
        TreeRowKind::Group => {
            let marker = if row.is_expanded { "[-]" } else { "[+]" };
            Line::from(vec![
                Span::styled(
                    marker.to_owned(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    row.group_label.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ])
        }
        TreeRowKind::Endpoint => {
            let label = state.tree_row_display_label(row);
            render_method_prefixed_line(&label)
                .unwrap_or_else(|| Line::from(vec![Span::raw(label)]))
        }
    }
}

fn draw_endpoint_details(frame: &mut Frame<'_>, state: &mut AppState, area: Rect) -> FrameMetrics {
    let title = format!("Details ({})", state.context().source_label);
    let is_focused = state.focus_panel() == FocusPanel::Details;
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(if is_focused {
            focused_border_style()
        } else {
            unfocused_border_style()
        });
    let inner = block.inner(area);

    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return FrameMetrics {
            detail_width: 1,
            detail_height: 1,
        };
    }

    let has_breadcrumb = inner.height >= 2;
    let regions = if has_breadcrumb {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(inner)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1)])
            .split(inner)
    };

    let detail_area = if has_breadcrumb {
        regions[1]
    } else {
        regions[0]
    };
    let detail_width = detail_area.width.max(1);
    let detail_height = detail_area.height.max(1);
    let scroll = state.detail_scroll().min(u16::MAX as usize) as u16;

    let raw_lines = state.detail_lines_for_selected(detail_width).to_vec();
    let active_span = if is_focused {
        state.active_detail_row_span(detail_width)
    } else {
        None
    };

    if has_breadcrumb {
        let breadcrumb_value = state
            .active_breadcrumb()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!("section: {}", section_label(state.active_detail_section()))
            });
        let breadcrumb_line = Line::from(vec![
            Span::styled(
                "Path ",
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                breadcrumb_value,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::ITALIC),
            ),
        ]);
        let breadcrumb = Paragraph::new(breadcrumb_line);
        frame.render_widget(breadcrumb, regions[0]);
    }

    let lines = raw_lines
        .iter()
        .enumerate()
        .map(|(line_index, line)| {
            let is_active_row = active_span.is_some_and(|(start, len)| {
                line_index >= start && line_index < start.saturating_add(len)
            });
            styled_detail_line(line, is_active_row)
        })
        .collect::<Vec<_>>();

    let detail = Paragraph::new(lines)
        .scroll((scroll, 0))
        .style(Style::default().fg(Color::White));
    frame.render_widget(detail, detail_area);

    FrameMetrics {
        detail_width,
        detail_height,
    }
}

fn styled_detail_line(line: &str, active: bool) -> Line<'static> {
    let rendered = if line.trim().is_empty() {
        Line::from(vec![Span::raw(String::new())])
    } else if let Some(styled) = render_section_header(line) {
        styled
    } else if let Some(styled) = render_method_prefixed_line(line) {
        styled
    } else if let Some(styled) = render_response_row(line) {
        styled
    } else if let Some(styled) = render_media_row(line) {
        styled
    } else if let Some(styled) = render_parameter_row(line) {
        styled
    } else if let Some(styled) = render_label_value_row(line) {
        styled
    } else if let Some(styled) = render_json_line(line) {
        styled
    } else {
        Line::from(vec![Span::raw(line.to_owned())])
    };

    if active {
        return rendered.patch_style(Style::default().bg(Color::DarkGray));
    }

    rendered
}

fn render_section_header(line: &str) -> Option<Line<'static>> {
    let trimmed = line.trim();
    if !SECTION_HEADERS.iter().any(|header| *header == trimmed) {
        return None;
    }

    Some(Line::from(vec![Span::styled(
        format!("-- {trimmed} --"),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )]))
}

fn render_method_prefixed_line(line: &str) -> Option<Line<'static>> {
    let indent_size = line.len().saturating_sub(line.trim_start().len());
    let indent = &line[..indent_size];
    let trimmed = line.trim_start();

    let method = trimmed.split_whitespace().next()?;
    if !HTTP_METHODS.contains(&method) {
        return None;
    }

    let rest = trimmed.strip_prefix(method).unwrap_or_default().to_owned();

    Some(Line::from(vec![
        Span::raw(indent.to_owned()),
        Span::styled(
            method.to_owned(),
            method_style(method).add_modifier(Modifier::BOLD),
        ),
        Span::styled(rest, Style::default().add_modifier(Modifier::BOLD)),
    ]))
}

fn render_response_row(line: &str) -> Option<Line<'static>> {
    let indent_size = line.len().saturating_sub(line.trim_start().len());
    let indent = &line[..indent_size];
    let trimmed = line.trim_start();
    let marker = if trimmed.starts_with("[+]") {
        "[+]"
    } else if trimmed.starts_with("[-]") {
        "[-]"
    } else {
        return None;
    };

    let body = trimmed.strip_prefix(marker)?.trim_start();
    let status_token = body.split(':').next()?.trim();
    if status_token.is_empty() {
        return None;
    }

    let status_like = status_token == "default"
        || status_token
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_digit());
    if !status_like {
        return None;
    }

    let suffix = body.strip_prefix(status_token).unwrap_or_default();
    Some(Line::from(vec![
        Span::raw(indent.to_owned()),
        Span::styled(marker.to_owned(), Style::default().fg(Color::Gray)),
        Span::raw(" "),
        Span::styled(
            status_token.to_owned(),
            status_style(status_token).add_modifier(Modifier::BOLD),
        ),
        Span::raw(suffix.to_owned()),
    ]))
}

fn render_media_row(line: &str) -> Option<Line<'static>> {
    let indent_size = line.len().saturating_sub(line.trim_start().len());
    let indent = &line[..indent_size];
    let trimmed = line.trim_start();
    let marker = if trimmed.starts_with("[+]") {
        "[+]"
    } else if trimmed.starts_with("[-]") {
        "[-]"
    } else {
        return None;
    };

    let body = trimmed.strip_prefix(marker)?.trim_start();
    let (left, right) = body.split_once("::")?;

    let left_trimmed = left.trim();
    let is_response_media = left_trimmed.starts_with("media ");
    let content_type = if is_response_media {
        left_trimmed.trim_start_matches("media ").trim()
    } else {
        left_trimmed
    };
    if !content_type.contains('/') {
        return None;
    }

    let mut spans = vec![
        Span::raw(indent.to_owned()),
        Span::styled(marker.to_owned(), Style::default().fg(Color::Gray)),
        Span::raw(" "),
    ];

    if is_response_media {
        spans.push(Span::styled(
            "media ".to_owned(),
            Style::default().fg(Color::Gray),
        ));
    }

    spans.push(Span::styled(
        content_type.to_owned(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(
        " :: ".to_owned(),
        Style::default().fg(Color::Gray),
    ));
    spans.push(Span::styled(
        right.trim().to_owned(),
        Style::default().fg(Color::LightYellow),
    ));

    Some(Line::from(spans))
}

fn render_parameter_row(line: &str) -> Option<Line<'static>> {
    if !line.contains(" | ") || !line.contains("| required:") {
        return None;
    }

    let mut parts = line.splitn(5, '|');
    let name = parts.next()?.trim_end().to_owned();
    let location = parts.next()?.trim().to_owned();
    let required_part = parts.next()?.trim().to_owned();
    let param_type = parts.next()?.trim().to_owned();
    let description = parts.next()?.trim().to_owned();

    let required_value = required_part
        .split_once(':')
        .map(|(_, value)| value.trim())
        .unwrap_or(required_part.trim());
    let required_style = if required_value.eq_ignore_ascii_case("yes") {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red)
    };

    Some(Line::from(vec![
        Span::styled(name, Style::default().fg(Color::Cyan)),
        Span::styled(" | ".to_owned(), Style::default().fg(Color::Gray)),
        Span::styled(location, Style::default().fg(Color::Magenta)),
        Span::styled(" | required: ".to_owned(), Style::default().fg(Color::Gray)),
        Span::styled(required_value.to_owned(), required_style),
        Span::styled(" | ".to_owned(), Style::default().fg(Color::Gray)),
        Span::styled(param_type, Style::default().fg(Color::Yellow)),
        Span::styled(" | ".to_owned(), Style::default().fg(Color::Gray)),
        Span::styled(description, Style::default().fg(Color::White)),
    ]))
}

fn render_label_value_row(line: &str) -> Option<Line<'static>> {
    let (label, value) = line.split_once(':')?;
    let trimmed_label = label.trim();

    let label_style = match trimmed_label {
        "Operation ID" | "Tags" | "enum" | "example" => Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::BOLD),
        "unresolved ref" | "Unresolved parameter ref" => {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        }
        _ => return None,
    };

    Some(Line::from(vec![
        Span::styled(format!("{label}:"), label_style),
        Span::raw(value.to_owned()),
    ]))
}

fn render_json_line(line: &str) -> Option<Line<'static>> {
    let indent_size = line.len().saturating_sub(line.trim_start().len());
    let indent = &line[..indent_size];
    let trimmed = line.trim_start();

    if trimmed.is_empty() {
        return None;
    }

    if is_json_bracket_line(trimmed) {
        return Some(Line::from(vec![
            Span::raw(indent.to_owned()),
            Span::styled(
                trimmed.to_owned(),
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    if let Some((key, value_part)) = parse_json_object_entry(trimmed) {
        let mut spans = vec![
            Span::raw(indent.to_owned()),
            Span::styled(format!("\"{key}\""), Style::default().fg(Color::Cyan)),
            Span::styled(": ".to_owned(), Style::default().fg(Color::Gray)),
        ];
        spans.extend(style_json_value(value_part));
        return Some(Line::from(spans));
    }

    if looks_like_json_value(trimmed) {
        let mut spans = vec![Span::raw(indent.to_owned())];
        spans.extend(style_json_value(trimmed));
        return Some(Line::from(spans));
    }

    None
}

fn parse_json_object_entry(line: &str) -> Option<(String, &str)> {
    if !line.starts_with('"') {
        return None;
    }
    let key_end = line.find("\":")?;
    let key = line.get(1..key_end)?.to_owned();
    let value = line.get((key_end + 2)..)?.trim_start();
    Some((key, value))
}

fn is_json_bracket_line(line: &str) -> bool {
    matches!(line, "{" | "}" | "[" | "]" | "{," | "}," | "]," | "[,")
}

fn looks_like_json_value(line: &str) -> bool {
    let value = strip_trailing_comma(line);
    value.starts_with('"')
        || matches!(value, "true" | "false" | "null")
        || value.parse::<f64>().is_ok()
        || matches!(value, "{" | "[" | "}" | "]")
}

fn style_json_value(value_with_optional_comma: &str) -> Vec<Span<'static>> {
    let has_comma = value_with_optional_comma.trim_end().ends_with(',');
    let core = strip_trailing_comma(value_with_optional_comma);

    let style = if core.starts_with('"') {
        Style::default().fg(Color::Green)
    } else if matches!(core, "true" | "false") {
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD)
    } else if core == "null" {
        Style::default().fg(Color::DarkGray)
    } else if core.parse::<f64>().is_ok() {
        Style::default().fg(Color::Yellow)
    } else if matches!(core, "{" | "[" | "}" | "]") {
        Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let mut spans = vec![Span::styled(core.to_owned(), style)];
    if has_comma {
        spans.push(Span::styled(
            ",".to_owned(),
            Style::default().fg(Color::Gray),
        ));
    }
    spans
}

fn strip_trailing_comma(value: &str) -> &str {
    value.trim().trim_end_matches(',').trim_end()
}

fn method_style(method: &str) -> Style {
    let color = match method {
        "GET" => Color::Green,
        "POST" => Color::Blue,
        "PUT" => Color::Cyan,
        "PATCH" => Color::Yellow,
        "DELETE" => Color::Red,
        "OPTIONS" => Color::Magenta,
        "HEAD" => Color::LightBlue,
        "TRACE" => Color::LightMagenta,
        _ => Color::White,
    };
    Style::default().fg(color)
}

fn status_style(status: &str) -> Style {
    let color = match status.chars().next() {
        Some('1') => Color::Cyan,
        Some('2') => Color::Green,
        Some('3') => Color::Blue,
        Some('4') => Color::Yellow,
        Some('5') => Color::Red,
        _ => Color::Gray,
    };
    Style::default().fg(color)
}

fn section_label(section: DetailSection) -> &'static str {
    match section {
        DetailSection::Overview => "overview",
        DetailSection::Parameters => "parameters",
        DetailSection::RequestBody => "request body",
        DetailSection::Responses => "responses",
        DetailSection::Security => "security",
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
            "q quit | Tree: j/k move g/G first/last PgUp/PgDn jump Enter open Right toggle group | Details: j/k row nav h/l scroll Tab next section Enter toggle Esc back | / or Ctrl+s search | Ctrl+u clear"
        }
        InputMode::Search => {
            "Search mode: type to filter | Backspace delete | Ctrl+u clear | Enter or Esc to return"
        }
    }
}
