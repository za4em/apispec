use std::collections::HashMap;

use crate::cache::metadata::CacheState;
use crate::spec::index::{EndpointSummary, RequestBodyView, ResponseView};
use crate::tui::tree::{TreeModel, TreeRow, TreeRowKind};

const MIN_DETAIL_WIDTH: u16 = 24;
const WIDTH_BUCKET_SIZE: u16 = 8;
const PAGE_STEP: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPanel {
    Tree,
    Details,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailSection {
    Overview,
    Parameters,
    RequestBody,
    Responses,
    Security,
}

impl DetailSection {
    fn next(self) -> Self {
        match self {
            Self::Overview => Self::Parameters,
            Self::Parameters => Self::RequestBody,
            Self::RequestBody => Self::Responses,
            Self::Responses => Self::Security,
            Self::Security => Self::Overview,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TuiContext {
    pub source_label: String,
    pub cache_state: CacheState,
    pub cached_at_utc: Option<String>,
    pub openapi_version: String,
}

pub struct AppState {
    endpoints: Vec<EndpointSummary>,
    endpoint_labels: Vec<String>,
    endpoint_positions_by_id: HashMap<usize, usize>,
    tree_model: TreeModel,
    selected_tree_row: usize,
    selected_endpoint_id: Option<usize>,
    detail_scroll: usize,
    detail_cache: HashMap<(usize, u16), Vec<String>>,
    search_query: String,
    input_mode: InputMode,
    focus_panel: FocusPanel,
    active_detail_section: DetailSection,
    should_quit: bool,
    context: TuiContext,
    empty_detail_lines: Vec<String>,
}

impl AppState {
    pub fn new(context: TuiContext, endpoints: Vec<EndpointSummary>) -> Self {
        let endpoint_labels = endpoints
            .iter()
            .map(build_endpoint_label)
            .collect::<Vec<_>>();
        let endpoint_positions_by_id = endpoints
            .iter()
            .enumerate()
            .map(|(index, endpoint)| (endpoint.id, index))
            .collect::<HashMap<_, _>>();
        let tree_model = TreeModel::new(&endpoints);
        let selected_tree_row = tree_model
            .first_endpoint_row_index()
            .or_else(|| {
                if tree_model.rows_visible.is_empty() {
                    None
                } else {
                    Some(0)
                }
            })
            .unwrap_or(0);
        let selected_endpoint_id = tree_model
            .rows_visible
            .get(selected_tree_row)
            .and_then(|row| row.endpoint_id)
            .or_else(|| tree_model.first_visible_endpoint_id());

        Self {
            endpoints,
            endpoint_labels,
            endpoint_positions_by_id,
            tree_model,
            selected_tree_row,
            selected_endpoint_id,
            detail_scroll: 0,
            detail_cache: HashMap::new(),
            search_query: String::new(),
            input_mode: InputMode::Normal,
            focus_panel: FocusPanel::Tree,
            active_detail_section: DetailSection::Overview,
            should_quit: false,
            context,
            empty_detail_lines: vec!["No endpoints match the current filter.".to_owned()],
        }
    }

    pub fn context(&self) -> &TuiContext {
        &self.context
    }

    pub fn input_mode(&self) -> InputMode {
        self.input_mode
    }

    pub fn focus_panel(&self) -> FocusPanel {
        self.focus_panel
    }

    pub fn active_detail_section(&self) -> DetailSection {
        self.active_detail_section
    }

    pub fn is_quit_requested(&self) -> bool {
        self.should_quit
    }

    pub fn request_quit(&mut self) {
        self.should_quit = true;
    }

    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    pub fn enter_search_mode(&mut self) {
        self.input_mode = InputMode::Search;
    }

    pub fn exit_search_mode(&mut self) {
        self.input_mode = InputMode::Normal;
    }

    pub fn focus_tree_panel(&mut self) {
        self.focus_panel = FocusPanel::Tree;
    }

    pub fn focus_details_panel(&mut self) {
        if self.selected_endpoint_index().is_some() {
            self.focus_panel = FocusPanel::Details;
        }
    }

    pub fn cycle_detail_section(&mut self) {
        self.active_detail_section = self.active_detail_section.next();
    }

    pub fn toggle_detail_item(&mut self) {
        // Placeholder for Phase 3 expandable details toggles.
    }

    pub fn push_search_char(&mut self, ch: char) {
        self.search_query.push(ch);
        self.rebuild_tree_projection();
    }

    pub fn pop_search_char(&mut self) {
        self.search_query.pop();
        self.rebuild_tree_projection();
    }

    pub fn clear_search(&mut self) {
        if self.search_query.is_empty() {
            return;
        }
        self.search_query.clear();
        self.rebuild_tree_projection();
    }

    pub fn tree_rows(&self) -> &[TreeRow] {
        self.tree_model.rows_visible.as_slice()
    }

    pub fn tree_row_display_label(&self, row: &TreeRow) -> String {
        match row.kind {
            TreeRowKind::Group => {
                let indicator = if row.is_expanded { "[-]" } else { "[+]" };
                format!("{indicator} {}", row.group_label)
            }
            TreeRowKind::Endpoint => {
                let indent = "  ".repeat(row.depth as usize);
                let label = row
                    .endpoint_id
                    .and_then(|endpoint_id| self.endpoint_label_by_id(endpoint_id))
                    .unwrap_or("<missing endpoint>");
                format!("{indent}{label}")
            }
        }
    }

    pub fn endpoint_count(&self) -> usize {
        self.endpoints.len()
    }

    pub fn filtered_count(&self) -> usize {
        self.tree_model.filtered_endpoint_count()
    }

    pub fn selected_tree_row_index(&self) -> Option<usize> {
        if self.tree_model.rows_visible.is_empty() {
            None
        } else {
            Some(self.selected_tree_row)
        }
    }

    pub fn select_first(&mut self) {
        if self.tree_model.rows_visible.is_empty() {
            return;
        }
        self.selected_tree_row = 0;
        self.sync_selected_endpoint_from_selected_row();
    }

    pub fn select_last(&mut self) {
        if self.tree_model.rows_visible.is_empty() {
            return;
        }
        self.selected_tree_row = self.tree_model.rows_visible.len().saturating_sub(1);
        self.sync_selected_endpoint_from_selected_row();
    }

    pub fn move_selection_up(&mut self, steps: usize) {
        if self.tree_model.rows_visible.is_empty() {
            return;
        }
        self.selected_tree_row = self.selected_tree_row.saturating_sub(steps);
        self.sync_selected_endpoint_from_selected_row();
    }

    pub fn move_selection_down(&mut self, steps: usize) {
        if self.tree_model.rows_visible.is_empty() {
            return;
        }
        let max_index = self.tree_model.rows_visible.len().saturating_sub(1);
        self.selected_tree_row = (self.selected_tree_row + steps).min(max_index);
        self.sync_selected_endpoint_from_selected_row();
    }

    pub fn page_up(&mut self) {
        self.move_selection_up(PAGE_STEP);
    }

    pub fn page_down(&mut self) {
        self.move_selection_down(PAGE_STEP);
    }

    pub fn toggle_selected_group(&mut self) {
        let Some(group_id) = self.selected_group_id() else {
            return;
        };

        if self.tree_model.toggle_group(&group_id) {
            self.rebuild_tree_projection();
        }
    }

    pub fn activate_selected_tree_row(&mut self) -> bool {
        let Some(row) = self.selected_row().cloned() else {
            return false;
        };

        match row.kind {
            TreeRowKind::Group => {
                self.toggle_selected_group();
                false
            }
            TreeRowKind::Endpoint => {
                if self.selected_endpoint_id != row.endpoint_id {
                    self.selected_endpoint_id = row.endpoint_id;
                    self.detail_scroll = 0;
                }
                true
            }
        }
    }

    pub fn scroll_detail_up(&mut self, steps: usize) {
        self.detail_scroll = self.detail_scroll.saturating_sub(steps);
    }

    pub fn scroll_detail_down(&mut self, steps: usize, detail_height: u16, detail_width: u16) {
        if detail_height == 0 {
            return;
        }
        let detail_len = self.detail_lines_for_selected(detail_width).len();
        let max_scroll = detail_len.saturating_sub(detail_height as usize);
        self.detail_scroll = (self.detail_scroll + steps).min(max_scroll);
    }

    pub fn detail_scroll(&self) -> usize {
        self.detail_scroll
    }

    pub fn clamp_detail_scroll(&mut self, detail_height: u16, detail_width: u16) {
        let detail_len = self.detail_lines_for_selected(detail_width).len();
        let max_scroll = detail_len.saturating_sub(detail_height as usize);
        if self.detail_scroll > max_scroll {
            self.detail_scroll = max_scroll;
        }
    }

    pub fn detail_lines_for_selected(&mut self, detail_width: u16) -> &[String] {
        let Some(endpoint_index) = self.selected_endpoint_index() else {
            return &self.empty_detail_lines;
        };
        let bucket = width_bucket(detail_width);
        let endpoint_id = self.endpoints[endpoint_index].id;
        let key = (endpoint_id, bucket);

        if !self.detail_cache.contains_key(&key) {
            let rendered =
                render_endpoint_detail_lines(&self.endpoints[endpoint_index], bucket as usize);
            self.detail_cache.insert(key, rendered);
        }

        self.detail_cache
            .get(&key)
            .expect("detail cache entry must exist after insertion check")
            .as_slice()
    }

    pub fn status_line(&self) -> String {
        let source_state = match self.context.cache_state {
            CacheState::Fresh => "Source: fresh".to_owned(),
            CacheState::Revalidated304 => "Source: cached (not modified)".to_owned(),
            CacheState::OfflineStale => {
                if let Some(timestamp) = self.context.cached_at_utc.as_deref() {
                    format!("Source: offline, using cached copy from {timestamp}")
                } else {
                    "Source: offline, using cached copy".to_owned()
                }
            }
        };

        let filter_part = if self.search_query.is_empty() {
            "Filter: none".to_owned()
        } else {
            format!("Filter: {}", self.search_query)
        };

        format!(
            "{source_state} | OpenAPI {} | Endpoints {}/{} | {}",
            self.context.openapi_version,
            self.filtered_count(),
            self.endpoint_count(),
            filter_part
        )
    }

    fn selected_endpoint_index(&self) -> Option<usize> {
        let endpoint_id = self.selected_endpoint_id?;
        self.endpoint_positions_by_id.get(&endpoint_id).copied()
    }

    fn endpoint_label_by_id(&self, endpoint_id: usize) -> Option<&str> {
        let index = self.endpoint_positions_by_id.get(&endpoint_id).copied()?;
        self.endpoint_labels.get(index).map(String::as_str)
    }

    fn selected_row(&self) -> Option<&TreeRow> {
        self.tree_model.rows_visible.get(self.selected_tree_row)
    }

    fn selected_group_id(&self) -> Option<String> {
        let row = self.selected_row()?;
        if matches!(row.kind, TreeRowKind::Group) {
            Some(row.group_id.clone())
        } else {
            None
        }
    }

    fn sync_selected_endpoint_from_selected_row(&mut self) {
        let row_endpoint = self.selected_row().and_then(|row| row.endpoint_id);
        if let Some(endpoint_id) = row_endpoint {
            if self.selected_endpoint_id != Some(endpoint_id) {
                self.selected_endpoint_id = Some(endpoint_id);
                self.detail_scroll = 0;
            }
            return;
        }

        if self.selected_endpoint_id.is_none() {
            self.selected_endpoint_id = self.tree_model.first_visible_endpoint_id();
            self.detail_scroll = 0;
        }
    }

    fn rebuild_tree_projection(&mut self) {
        let previous_selected_endpoint_id = self.selected_endpoint_id;
        let previous_selected_row_endpoint = self.selected_row().and_then(|row| row.endpoint_id);
        let previous_selected_group_id = self.selected_group_id();
        let previous_tree_row = self.selected_tree_row;

        self.tree_model
            .rebuild_visible_rows(&self.endpoints, &self.search_query);

        if self.tree_model.rows_visible.is_empty() {
            self.selected_tree_row = 0;
            self.selected_endpoint_id = None;
            self.detail_scroll = 0;
            self.focus_panel = FocusPanel::Tree;
            return;
        }

        let mut next_tree_row = None;

        if let Some(endpoint_id) = previous_selected_row_endpoint.or(previous_selected_endpoint_id)
        {
            next_tree_row = self.tree_model.row_index_for_endpoint(endpoint_id);
        }

        if next_tree_row.is_none() {
            if let Some(group_id) = previous_selected_group_id.as_deref() {
                next_tree_row = self.tree_model.row_index_for_group(group_id);
            }
        }

        if next_tree_row.is_none() {
            next_tree_row = Some(previous_tree_row.min(self.tree_model.rows_visible.len() - 1));
        }

        self.selected_tree_row = next_tree_row.unwrap_or(0);

        let mut next_selected_endpoint = self
            .tree_model
            .rows_visible
            .get(self.selected_tree_row)
            .and_then(|row| row.endpoint_id);

        if next_selected_endpoint.is_none() {
            if let Some(previous_endpoint) = previous_selected_endpoint_id {
                if self
                    .tree_model
                    .row_index_for_endpoint(previous_endpoint)
                    .is_some()
                {
                    next_selected_endpoint = Some(previous_endpoint);
                }
            }
        }

        if next_selected_endpoint.is_none() {
            next_selected_endpoint = self.tree_model.first_visible_endpoint_id();
        }

        if self.selected_endpoint_id != next_selected_endpoint {
            self.detail_scroll = 0;
        }

        self.selected_endpoint_id = next_selected_endpoint;

        if self.focus_panel == FocusPanel::Details && self.selected_endpoint_id.is_none() {
            self.focus_panel = FocusPanel::Tree;
        }
    }
}

fn build_endpoint_label(endpoint: &EndpointSummary) -> String {
    let fallback_title = format!("{} {}", endpoint.method, endpoint.path);
    if endpoint.title == fallback_title {
        format!("{:<7} {}", endpoint.method, endpoint.path)
    } else {
        format!(
            "{:<7} {}  {}",
            endpoint.method, endpoint.path, endpoint.title
        )
    }
}

fn width_bucket(width: u16) -> u16 {
    let width = width.max(MIN_DETAIL_WIDTH);
    (width / WIDTH_BUCKET_SIZE).max(1) * WIDTH_BUCKET_SIZE
}

fn render_endpoint_detail_lines(endpoint: &EndpointSummary, width: usize) -> Vec<String> {
    let mut lines = Vec::new();

    push_wrapped_line(
        &mut lines,
        &format!("{} {}", endpoint.method, endpoint.path),
        width,
        "  ",
    );
    push_wrapped_line(&mut lines, &endpoint.title, width, "  ");
    if let Some(operation_id) = endpoint.operation_id.as_deref() {
        push_wrapped_line(
            &mut lines,
            &format!("Operation ID: {operation_id}"),
            width,
            "  ",
        );
    }
    lines.push(String::new());

    lines.push("Description".to_owned());
    if let Some(description) = endpoint.description.as_deref() {
        for line in description.lines() {
            push_wrapped_line(&mut lines, line, width, "  ");
        }
    } else {
        lines.push("None".to_owned());
    }
    lines.push(String::new());

    lines.push("Parameters".to_owned());
    render_parameter_group(&mut lines, "Path", &endpoint.grouped_parameters.path, width);
    render_parameter_group(
        &mut lines,
        "Query",
        &endpoint.grouped_parameters.query,
        width,
    );
    render_parameter_group(
        &mut lines,
        "Header",
        &endpoint.grouped_parameters.header,
        width,
    );
    render_parameter_group(
        &mut lines,
        "Cookie",
        &endpoint.grouped_parameters.cookie,
        width,
    );
    if endpoint.grouped_parameters.path.is_empty()
        && endpoint.grouped_parameters.query.is_empty()
        && endpoint.grouped_parameters.header.is_empty()
        && endpoint.grouped_parameters.cookie.is_empty()
    {
        lines.push("None".to_owned());
    }
    for unresolved in &endpoint.grouped_parameters.unresolved_refs {
        push_wrapped_line(
            &mut lines,
            &format!("Unresolved parameter ref: {unresolved}"),
            width,
            "  ",
        );
    }
    lines.push(String::new());

    lines.push("Request Body".to_owned());
    render_request_body(&mut lines, endpoint.request_body.as_ref(), width);
    lines.push(String::new());

    lines.push("Responses".to_owned());
    render_responses(&mut lines, &endpoint.responses, width);

    lines
}

fn render_parameter_group(
    lines: &mut Vec<String>,
    title: &str,
    parameters: &[crate::spec::index::ParameterView],
    width: usize,
) {
    if parameters.is_empty() {
        return;
    }

    lines.push(format!("{title}:"));
    for parameter in parameters {
        let required_flag = if parameter.required {
            "required"
        } else {
            "optional"
        };
        let mut line = format!("  - {} ({required_flag})", parameter.name);
        if let Some(schema) = parameter.schema.as_deref() {
            line.push_str(&format!(" :: {schema}"));
        }
        push_wrapped_line(lines, &line, width, "    ");
        if let Some(description) = parameter.description.as_deref() {
            push_wrapped_line(lines, &format!("    {description}"), width, "      ");
        }
    }
}

fn render_request_body(
    lines: &mut Vec<String>,
    request_body: Option<&RequestBodyView>,
    width: usize,
) {
    let Some(request_body) = request_body else {
        lines.push("None".to_owned());
        return;
    };

    let required = if request_body.required {
        "required"
    } else {
        "optional"
    };
    lines.push(format!("Required: {required}"));

    if request_body.media_types.is_empty() {
        lines.push("Media types: none".to_owned());
    } else {
        lines.push("Media types:".to_owned());
        for media_type in &request_body.media_types {
            let mut line = format!("  - {}", media_type.content_type);
            if let Some(schema) = media_type.schema.as_deref() {
                line.push_str(&format!(" :: {schema}"));
            }
            push_wrapped_line(lines, &line, width, "    ");
        }
    }

    if let Some(unresolved) = request_body.unresolved_ref.as_deref() {
        push_wrapped_line(
            lines,
            &format!("Unresolved request body ref: {unresolved}"),
            width,
            "  ",
        );
    }
}

fn render_responses(lines: &mut Vec<String>, responses: &[ResponseView], width: usize) {
    if responses.is_empty() {
        lines.push("None".to_owned());
        return;
    }

    for response in responses {
        let description = response
            .description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("no description");
        push_wrapped_line(
            lines,
            &format!("{}: {}", response.status, description),
            width,
            "  ",
        );
        if response.media_types.is_empty() {
            lines.push("  media: none".to_owned());
        } else {
            for media_type in &response.media_types {
                let mut line = format!("  media {} ", media_type.content_type);
                if let Some(schema) = media_type.schema.as_deref() {
                    line.push_str(&format!(":: {schema}"));
                } else {
                    line.push_str(":: any");
                }
                push_wrapped_line(lines, &line, width, "    ");
            }
        }
        if let Some(unresolved) = response.unresolved_ref.as_deref() {
            push_wrapped_line(
                lines,
                &format!("  unresolved ref: {unresolved}"),
                width,
                "    ",
            );
        }
    }
}

fn push_wrapped_line(lines: &mut Vec<String>, line: &str, width: usize, continuation_indent: &str) {
    let width = width.max(8);
    let trimmed = line.trim_end();
    if trimmed.is_empty() {
        lines.push(String::new());
        return;
    }

    let mut current = trimmed.to_owned();
    while current.chars().count() > width {
        let mut split_index = split_index_for_width(&current, width);
        if split_index == 0 || split_index >= current.len() {
            split_index = hard_split_index(&current, width);
        }
        if split_index == 0 || split_index >= current.len() {
            lines.push(current);
            return;
        }

        let (head, tail) = current.split_at(split_index);
        lines.push(head.trim_end().to_owned());
        let remaining = tail.trim_start();
        if remaining.is_empty() {
            return;
        }
        current = format!("{continuation_indent}{remaining}");
    }
    lines.push(current);
}

fn split_index_for_width(value: &str, width: usize) -> usize {
    let mut last_space = None;
    let mut fallback_index = value.len();
    let mut first_non_whitespace = value.len();

    for (char_count, (idx, ch)) in value.char_indices().enumerate() {
        if !ch.is_whitespace() && first_non_whitespace == value.len() {
            first_non_whitespace = idx;
        }

        if char_count >= width {
            fallback_index = idx;
            break;
        }

        // Ignore leading indentation when choosing whitespace split points;
        // otherwise wrapping can repeatedly split before the content and loop.
        if ch.is_whitespace() && idx >= first_non_whitespace {
            last_space = Some(idx);
        }
    }

    last_space.filter(|idx| *idx > 0).unwrap_or(fallback_index)
}

fn hard_split_index(value: &str, width: usize) -> usize {
    for (char_count, (idx, _)) in value.char_indices().enumerate() {
        if char_count >= width {
            return idx;
        }
    }
    value.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::index::build_endpoint_index;
    use oas3::Spec;

    fn parse_spec(yaml: &str) -> Spec {
        serde_yaml::from_str::<Spec>(yaml).unwrap()
    }

    fn demo_context() -> TuiContext {
        TuiContext {
            source_label: "demo-source".to_owned(),
            cache_state: CacheState::Fresh,
            cached_at_utc: None,
            openapi_version: "3.1.0".to_owned(),
        }
    }

    fn endpoint_id_for_path(state: &AppState, path: &str) -> usize {
        state
            .endpoints
            .iter()
            .find(|endpoint| endpoint.path == path)
            .expect("endpoint not found")
            .id
    }

    fn selected_path(state: &AppState) -> Option<&str> {
        let index = state.selected_endpoint_index()?;
        Some(state.endpoints[index].path.as_str())
    }

    #[test]
    fn filters_endpoints_case_insensitively() {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths:
  /pets:
    get:
      summary: List pets
      responses:
        "200":
          description: ok
  /users:
    get:
      summary: List users
      responses:
        "200":
          description: ok
"#,
        );
        let endpoints = build_endpoint_index(&spec);
        let mut state = AppState::new(demo_context(), endpoints);

        for ch in "PETS".chars() {
            state.push_search_char(ch);
        }

        assert_eq!(state.filtered_count(), 1);
        assert_eq!(selected_path(&state), Some("/pets"));
    }

    #[test]
    fn detail_scroll_is_reset_when_selection_changes() {
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
  /users:
    get:
      responses:
        "200":
          description: ok
"#,
        );
        let endpoints = build_endpoint_index(&spec);
        let mut state = AppState::new(demo_context(), endpoints);

        state.scroll_detail_down(100, 5, 80);
        assert!(state.detail_scroll() > 0);

        state.move_selection_down(2);
        assert_eq!(state.detail_scroll(), 0);
    }

    #[test]
    fn preserves_selected_endpoint_across_filter_updates() {
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
  /users:
    get:
      responses:
        "200":
          description: ok
"#,
        );
        let endpoints = build_endpoint_index(&spec);
        let mut state = AppState::new(demo_context(), endpoints);

        let users_id = endpoint_id_for_path(&state, "/users");
        state.selected_tree_row = state
            .tree_model
            .row_index_for_endpoint(users_id)
            .expect("users row must be visible");
        state.sync_selected_endpoint_from_selected_row();
        assert_eq!(state.selected_endpoint_id, Some(users_id));

        for ch in "users".chars() {
            state.push_search_char(ch);
        }

        assert_eq!(state.selected_endpoint_id, Some(users_id));
        assert_eq!(selected_path(&state), Some("/users"));
    }

    #[test]
    fn auto_expand_is_applied_only_while_filter_is_active() {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths:
  /pets:
    get:
      tags: ["animals"]
      responses:
        "200":
          description: ok
  /users:
    get:
      tags: ["users"]
      responses:
        "200":
          description: ok
"#,
        );
        let endpoints = build_endpoint_index(&spec);
        let mut state = AppState::new(demo_context(), endpoints);

        let animals_group = state
            .tree_model
            .row_index_for_group("animals")
            .expect("animals group row must exist");
        state.selected_tree_row = animals_group;
        state.toggle_selected_group();

        assert!(!state.tree_model.expanded_groups.contains("animals"));

        for ch in "pets".chars() {
            state.push_search_char(ch);
        }

        assert!(state.tree_model.filter_active());
        assert!(state.tree_model.expanded_groups.contains("animals"));

        state.clear_search();

        assert!(!state.tree_model.filter_active());
        assert!(!state.tree_model.expanded_groups.contains("animals"));
    }

    #[test]
    fn focus_transitions_between_tree_and_details() {
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
        let endpoints = build_endpoint_index(&spec);
        let mut state = AppState::new(demo_context(), endpoints);

        assert_eq!(state.focus_panel(), FocusPanel::Tree);

        assert!(state.activate_selected_tree_row());
        state.focus_details_panel();
        assert_eq!(state.focus_panel(), FocusPanel::Details);

        state.focus_tree_panel();
        assert_eq!(state.focus_panel(), FocusPanel::Tree);

        let previous = state.active_detail_section();
        state.cycle_detail_section();
        assert_ne!(state.active_detail_section(), previous);
    }

    #[test]
    fn wraps_long_unbroken_tokens_without_infinite_loop() {
        let mut lines = Vec::new();
        push_wrapped_line(
            &mut lines,
            "  - aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            12,
            "      ",
        );

        assert!(lines.len() > 1);
        assert!(lines.len() < 20);
        assert!(lines.iter().any(|line| line.contains("aaaa")));
    }
}
