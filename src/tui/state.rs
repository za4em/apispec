use std::collections::{HashMap, HashSet};

use crate::cache::metadata::CacheState;
use crate::spec::index::EndpointSummary;
use crate::tui::details::{DetailSection, DetailsDocument, build_details_document};
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

#[derive(Debug, Clone, Default)]
struct DetailsState {
    expanded_toggles: HashSet<String>,
    active_breadcrumb: Option<String>,
    last_detail_width: u16,
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
    detail_cursor_line: usize,
    detail_cache: HashMap<(usize, u16, String), DetailsDocument>,
    details_state: DetailsState,
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
            detail_cursor_line: 0,
            detail_cache: HashMap::new(),
            details_state: DetailsState::default(),
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
        let active_section = self.active_detail_section;
        let width = self.details_state.last_detail_width.max(MIN_DETAIL_WIDTH);
        let next_position = self
            .detail_document_for_selected(width)
            .and_then(|document| {
                document
                    .section_line_start(active_section)
                    .map(|line_start| {
                        let breadcrumb =
                            document.breadcrumb_for_line(line_start).map(str::to_owned);
                        (line_start, breadcrumb)
                    })
            });
        if let Some((line_start, breadcrumb)) = next_position {
            self.detail_scroll = line_start;
            self.detail_cursor_line = line_start;
            self.details_state.active_breadcrumb = breadcrumb;
        }
    }

    pub fn toggle_detail_item(&mut self) {
        let width = self.details_state.last_detail_width.max(MIN_DETAIL_WIDTH);
        let current_line = self.detail_cursor_line;
        let Some(toggle_target) = self
            .detail_document_for_selected(width)
            .and_then(|document| document.nearest_toggle_row(current_line))
            .and_then(|row| row.toggle_target.as_deref())
            .map(str::to_owned)
        else {
            return;
        };

        if self.details_state.expanded_toggles.remove(&toggle_target) {
            // collapsed
        } else {
            self.details_state.expanded_toggles.insert(toggle_target);
        }

        self.drop_detail_cache_for_selected_endpoint();
    }

    pub fn move_detail_row_up(&mut self, steps: usize, detail_height: u16, detail_width: u16) {
        self.details_state.last_detail_width = detail_width.max(MIN_DETAIL_WIDTH);
        let current_line = self.detail_cursor_line;
        if let Some(line_start) = self
            .detail_document_for_selected(detail_width)
            .and_then(|document| document.previous_row_line_start(current_line, steps))
        {
            self.detail_cursor_line = line_start;
        }

        self.ensure_detail_cursor_visible(detail_height, detail_width);
        self.update_active_breadcrumb_for_current_cursor();
    }

    pub fn move_detail_row_down(&mut self, steps: usize, detail_height: u16, detail_width: u16) {
        self.details_state.last_detail_width = detail_width.max(MIN_DETAIL_WIDTH);
        let current_line = self.detail_cursor_line;
        if let Some(line_start) = self
            .detail_document_for_selected(detail_width)
            .and_then(|document| document.next_row_line_start(current_line, steps))
        {
            self.detail_cursor_line = line_start;
        }

        self.ensure_detail_cursor_visible(detail_height, detail_width);
        self.update_active_breadcrumb_for_current_cursor();
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
                    self.reset_details_navigation();
                }
                true
            }
        }
    }

    pub fn scroll_detail_up(&mut self, steps: usize) {
        self.detail_scroll = self.detail_scroll.saturating_sub(steps);
        self.detail_cursor_line = self.detail_scroll;
        self.update_active_breadcrumb_for_current_cursor();
    }

    pub fn scroll_detail_down(&mut self, steps: usize, detail_height: u16, detail_width: u16) {
        self.details_state.last_detail_width = detail_width.max(MIN_DETAIL_WIDTH);
        if detail_height == 0 {
            return;
        }
        let detail_len = self.detail_lines_for_selected(detail_width).len();
        let max_scroll = detail_len.saturating_sub(detail_height as usize);
        self.detail_scroll = (self.detail_scroll + steps).min(max_scroll);
        self.detail_cursor_line = self.detail_scroll;
        self.update_active_breadcrumb_for_current_cursor();
    }

    pub fn detail_scroll(&self) -> usize {
        self.detail_scroll
    }

    pub fn clamp_detail_scroll(&mut self, detail_height: u16, detail_width: u16) {
        self.details_state.last_detail_width = detail_width.max(MIN_DETAIL_WIDTH);
        let detail_len = self.detail_lines_for_selected(detail_width).len();
        let max_scroll = detail_len.saturating_sub(detail_height as usize);
        if self.detail_scroll > max_scroll {
            self.detail_scroll = max_scroll;
        }
        self.clamp_detail_cursor_to_len(detail_len);
        self.ensure_detail_cursor_visible(detail_height, detail_width);
        self.update_active_breadcrumb_for_current_cursor();
    }

    pub fn detail_lines_for_selected(&mut self, detail_width: u16) -> &[String] {
        self.details_state.last_detail_width = detail_width.max(MIN_DETAIL_WIDTH);
        if self.selected_endpoint_index().is_none() {
            return &self.empty_detail_lines;
        }

        let current_line = self.detail_cursor_line;
        self.details_state.active_breadcrumb = self
            .detail_document_for_selected(detail_width)
            .and_then(|document| document.breadcrumb_for_line(current_line))
            .map(str::to_owned);

        let document = self
            .detail_document_for_selected(detail_width)
            .expect("selected endpoint should always render a details document");
        document.lines.as_slice()
    }

    pub fn active_breadcrumb(&self) -> Option<&str> {
        self.details_state.active_breadcrumb.as_deref()
    }

    pub fn active_detail_row_span(&mut self, detail_width: u16) -> Option<(usize, usize)> {
        self.details_state.last_detail_width = detail_width.max(MIN_DETAIL_WIDTH);
        let current_line = self.detail_cursor_line;
        self.detail_document_for_selected(detail_width)
            .and_then(|document| document.row_for_line(current_line))
            .map(|row| (row.line_start, row.line_len.max(1)))
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

    fn detail_document_for_selected(&mut self, detail_width: u16) -> Option<&DetailsDocument> {
        let endpoint_index = self.selected_endpoint_index()?;
        let endpoint = self.endpoints.get(endpoint_index)?;
        let bucket = width_bucket(detail_width);
        let expansion_fingerprint = self.expansion_fingerprint(endpoint.id);
        let cache_key = (endpoint.id, bucket, expansion_fingerprint);

        if !self.detail_cache.contains_key(&cache_key) {
            let document = build_details_document(
                endpoint,
                bucket as usize,
                &self.details_state.expanded_toggles,
            );
            self.detail_cache.insert(cache_key.clone(), document);
        }

        self.detail_cache.get(&cache_key)
    }

    fn expansion_fingerprint(&self, endpoint_id: usize) -> String {
        let prefix = format!("endpoint:{endpoint_id}:");
        let mut toggles = self
            .details_state
            .expanded_toggles
            .iter()
            .filter(|value| value.starts_with(&prefix))
            .cloned()
            .collect::<Vec<_>>();
        toggles.sort();
        toggles.join("|")
    }

    fn drop_detail_cache_for_selected_endpoint(&mut self) {
        let Some(endpoint_id) = self.selected_endpoint_id else {
            return;
        };
        self.detail_cache
            .retain(|(cached_endpoint_id, _, _), _| *cached_endpoint_id != endpoint_id);
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
                self.reset_details_navigation();
            }
            return;
        }

        if self.selected_endpoint_id.is_none() {
            self.selected_endpoint_id = self.tree_model.first_visible_endpoint_id();
            self.reset_details_navigation();
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
            self.reset_details_navigation();
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
            self.reset_details_navigation();
        }

        self.selected_endpoint_id = next_selected_endpoint;

        if self.focus_panel == FocusPanel::Details && self.selected_endpoint_id.is_none() {
            self.focus_panel = FocusPanel::Tree;
        }
    }

    fn reset_details_navigation(&mut self) {
        self.detail_scroll = 0;
        self.detail_cursor_line = 0;
        self.details_state.active_breadcrumb = None;
    }

    fn update_active_breadcrumb_for_current_cursor(&mut self) {
        let width = self.details_state.last_detail_width.max(MIN_DETAIL_WIDTH);
        let current_line = self.detail_cursor_line;
        self.details_state.active_breadcrumb = self
            .detail_document_for_selected(width)
            .and_then(|document| document.breadcrumb_for_line(current_line))
            .map(str::to_owned);
    }

    fn ensure_detail_cursor_visible(&mut self, detail_height: u16, detail_width: u16) {
        if detail_height == 0 {
            return;
        }

        let detail_len = self.detail_lines_for_selected(detail_width).len();
        self.clamp_detail_cursor_to_len(detail_len);

        let viewport_height = detail_height as usize;
        let max_scroll = detail_len.saturating_sub(viewport_height);
        if self.detail_scroll > max_scroll {
            self.detail_scroll = max_scroll;
        }

        if self.detail_cursor_line < self.detail_scroll {
            self.detail_scroll = self.detail_cursor_line;
            return;
        }

        let viewport_end = self
            .detail_scroll
            .saturating_add(viewport_height.saturating_sub(1));
        if self.detail_cursor_line > viewport_end {
            self.detail_scroll = self
                .detail_cursor_line
                .saturating_sub(viewport_height.saturating_sub(1))
                .min(max_scroll);
        }
    }

    fn clamp_detail_cursor_to_len(&mut self, detail_len: usize) {
        let max_line = detail_len.saturating_sub(1);
        if self.detail_cursor_line > max_line {
            self.detail_cursor_line = max_line;
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
    fn toggles_expand_request_body_and_schema_rows() {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths:
  /items:
    post:
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              properties:
                id:
                  type: integer
      responses:
        "200":
          description: ok
"#,
        );
        let endpoints = build_endpoint_index(&spec);
        let mut state = AppState::new(demo_context(), endpoints);

        let initial_lines = state.detail_lines_for_selected(100).to_vec();
        assert!(
            !initial_lines
                .iter()
                .any(|line| line.contains("id : integer"))
        );

        state.toggle_detail_item();
        let request_expanded = state.detail_lines_for_selected(100).to_vec();
        let media_line = request_expanded
            .iter()
            .position(|line| line.contains("application/json"))
            .expect("request media row should be visible after expanding request body");

        state.detail_scroll = media_line;
        state.detail_cursor_line = media_line;
        state.toggle_detail_item();
        let media_expanded = state.detail_lines_for_selected(100).to_vec();
        let schema_line = media_expanded
            .iter()
            .position(|line| line.contains("schema : object"))
            .expect("schema root row should be visible after expanding media type");

        state.detail_scroll = schema_line;
        state.detail_cursor_line = schema_line;
        state.toggle_detail_item();
        let schema_expanded = state.detail_lines_for_selected(100).to_vec();
        let id_line = schema_expanded
            .iter()
            .position(|line| line.contains("id : integer"))
            .expect("schema property row should be visible when schema root is expanded");

        state.detail_scroll = id_line;
        state.detail_cursor_line = id_line;
        state.clamp_detail_scroll(1, 100);
        assert!(
            state
                .active_breadcrumb()
                .is_some_and(|breadcrumb| breadcrumb.contains("id"))
        );
    }

    #[test]
    fn detail_row_navigation_works_without_scrollable_viewport() {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths:
  /items:
    post:
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
      responses:
        "200":
          description: ok
          content:
            application/json:
              schema:
                type: object
"#,
        );
        let endpoints = build_endpoint_index(&spec);
        let mut state = AppState::new(demo_context(), endpoints);
        let detail_height = 100;
        let detail_width = 120;

        state.toggle_detail_item();
        let before = state.detail_lines_for_selected(detail_width).to_vec();
        let response_row = before
            .iter()
            .position(|line| line.contains("[+] 200:"))
            .expect("response row should exist");

        while state.detail_cursor_line < response_row {
            state.move_detail_row_down(1, detail_height, detail_width);
        }

        state.toggle_detail_item();
        let after = state.detail_lines_for_selected(detail_width).to_vec();
        assert!(
            after.iter().any(|line| line.contains("[-] 200:")),
            "response toggle should switch once cursor reaches response row"
        );
    }

    #[test]
    fn handles_empty_specs_without_focus_or_selection_panics() {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths: {}
"#,
        );
        let endpoints = build_endpoint_index(&spec);
        let mut state = AppState::new(demo_context(), endpoints);

        assert_eq!(state.endpoint_count(), 0);
        assert_eq!(state.filtered_count(), 0);
        assert!(state.tree_rows().is_empty());
        assert_eq!(state.selected_tree_row_index(), None);
        assert_eq!(
            state.detail_lines_for_selected(80),
            ["No endpoints match the current filter."]
        );

        assert!(!state.activate_selected_tree_row());
        state.focus_details_panel();
        assert_eq!(state.focus_panel(), FocusPanel::Tree);
    }

    #[test]
    fn returns_focus_to_tree_when_filter_hides_all_endpoints() {
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

        assert!(state.activate_selected_tree_row());
        state.focus_details_panel();
        assert_eq!(state.focus_panel(), FocusPanel::Details);

        for ch in "missing".chars() {
            state.push_search_char(ch);
        }

        assert_eq!(state.focus_panel(), FocusPanel::Tree);
        assert_eq!(state.filtered_count(), 0);
        assert_eq!(
            state.detail_lines_for_selected(80),
            ["No endpoints match the current filter."]
        );
    }
}
