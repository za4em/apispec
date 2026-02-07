use std::collections::{HashMap, HashSet};

use crate::spec::index::{EndpointSummary, ParameterView};
use crate::spec::schema_tree::SchemaNode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailSection {
    Overview,
    Parameters,
    RequestBody,
    Responses,
    Security,
}

impl DetailSection {
    pub fn next(self) -> Self {
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
pub struct DetailRow {
    pub toggle_target: Option<String>,
    pub breadcrumb: Option<String>,
    pub line_start: usize,
    pub line_len: usize,
}

#[derive(Debug, Clone)]
pub struct DetailsDocument {
    pub lines: Vec<String>,
    pub rows: Vec<DetailRow>,
    section_first_row: HashMap<DetailSection, usize>,
    toggle_rows: Vec<usize>,
    row_index_by_id: HashMap<String, usize>,
}

impl DetailsDocument {
    pub fn section_line_start(&self, section: DetailSection) -> Option<usize> {
        let row_index = self.section_first_row.get(&section).copied()?;
        self.rows.get(row_index).map(|row| row.line_start)
    }

    pub fn nearest_toggle_row(&self, line_index: usize) -> Option<&DetailRow> {
        if self.toggle_rows.is_empty() {
            return None;
        }

        if let Some(row_index) = self.toggle_rows.iter().copied().find(|row_index| {
            let Some(row) = self.rows.get(*row_index) else {
                return false;
            };
            let row_end = row.line_start.saturating_add(row.line_len.max(1));
            line_index >= row.line_start && line_index < row_end
        }) {
            return self.rows.get(row_index);
        }

        if let Some(row_index) = self.toggle_rows.iter().copied().find(|row_index| {
            self.rows
                .get(*row_index)
                .is_some_and(|row| row.line_start > line_index)
        }) {
            return self.rows.get(row_index);
        }

        self.toggle_rows
            .last()
            .and_then(|row_index| self.rows.get(*row_index))
    }

    pub fn breadcrumb_for_line(&self, line_index: usize) -> Option<&str> {
        self.rows
            .iter()
            .find(|row| {
                let row_end = row.line_start.saturating_add(row.line_len.max(1));
                line_index >= row.line_start && line_index < row_end
            })
            .and_then(|row| row.breadcrumb.as_deref())
    }

    pub fn row_index_by_id(&self, row_id: &str) -> Option<usize> {
        self.row_index_by_id.get(row_id).copied()
    }
}

struct DocumentBuilder {
    width: usize,
    lines: Vec<String>,
    rows: Vec<DetailRow>,
    section_first_row: HashMap<DetailSection, usize>,
    toggle_rows: Vec<usize>,
    row_index_by_id: HashMap<String, usize>,
}

impl DocumentBuilder {
    fn new(width: usize) -> Self {
        Self {
            width: width.max(8),
            lines: Vec::new(),
            rows: Vec::new(),
            section_first_row: HashMap::new(),
            toggle_rows: Vec::new(),
            row_index_by_id: HashMap::new(),
        }
    }

    fn finish(self) -> DetailsDocument {
        DetailsDocument {
            lines: self.lines,
            rows: self.rows,
            section_first_row: self.section_first_row,
            toggle_rows: self.toggle_rows,
            row_index_by_id: self.row_index_by_id,
        }
    }

    fn push_blank(&mut self) {
        if self.lines.last().is_some_and(|line| line.is_empty()) {
            return;
        }
        self.lines.push(String::new());
    }

    fn push_section_header(&mut self, section: DetailSection, title: &str) {
        self.push_blank();
        self.push_row(section, title, None, None, None, "  ");
    }

    fn push_row(
        &mut self,
        section: DetailSection,
        text: &str,
        row_id: Option<String>,
        toggle_target: Option<String>,
        breadcrumb: Option<String>,
        continuation_indent: &str,
    ) {
        let line_start = self.lines.len();
        push_wrapped_line(&mut self.lines, text, self.width, continuation_indent);
        let line_len = self.lines.len().saturating_sub(line_start).max(1);
        let row_index = self.rows.len();

        if let Some(row_id_value) = row_id.as_deref() {
            self.row_index_by_id
                .insert(row_id_value.to_owned(), row_index);
        }

        if toggle_target.is_some() {
            self.toggle_rows.push(row_index);
        }

        self.section_first_row.entry(section).or_insert(row_index);
        self.rows.push(DetailRow {
            toggle_target,
            breadcrumb,
            line_start,
            line_len,
        });
    }
}

pub fn build_details_document(
    endpoint: &EndpointSummary,
    width: usize,
    expanded_toggles: &HashSet<String>,
) -> DetailsDocument {
    let mut builder = DocumentBuilder::new(width);

    render_overview_section(&mut builder, endpoint);
    render_parameters_section(&mut builder, endpoint);
    render_request_body_section(&mut builder, endpoint, expanded_toggles);
    render_responses_section(&mut builder, endpoint, expanded_toggles);
    render_security_section(&mut builder);

    builder.finish()
}

fn render_overview_section(builder: &mut DocumentBuilder, endpoint: &EndpointSummary) {
    builder.push_row(
        DetailSection::Overview,
        &format!("{} {}", endpoint.method, endpoint.path),
        Some("overview:header".to_owned()),
        None,
        None,
        "  ",
    );

    let fallback_title = format!("{} {}", endpoint.method, endpoint.path);
    if endpoint.title != fallback_title {
        builder.push_row(
            DetailSection::Overview,
            &endpoint.title,
            Some("overview:title".to_owned()),
            None,
            None,
            "  ",
        );
    }

    if let Some(operation_id) = endpoint.operation_id.as_deref() {
        builder.push_row(
            DetailSection::Overview,
            &format!("Operation ID: {operation_id}"),
            Some("overview:operation_id".to_owned()),
            None,
            None,
            "  ",
        );
    }

    if !endpoint.tags.is_empty() {
        builder.push_row(
            DetailSection::Overview,
            &format!("Tags: {}", endpoint.tags.join(", ")),
            Some("overview:tags".to_owned()),
            None,
            None,
            "  ",
        );
    }

    builder.push_section_header(DetailSection::Overview, "Description");
    if let Some(description) = endpoint.description.as_deref() {
        for line in description.lines() {
            builder.push_row(DetailSection::Overview, line, None, None, None, "  ");
        }
    } else {
        builder.push_row(DetailSection::Overview, "None", None, None, None, "  ");
    }
}

fn render_parameters_section(builder: &mut DocumentBuilder, endpoint: &EndpointSummary) {
    builder.push_section_header(DetailSection::Parameters, "Parameters");

    let mut has_rows = false;
    for parameter in endpoint
        .grouped_parameters
        .path
        .iter()
        .chain(endpoint.grouped_parameters.query.iter())
        .chain(endpoint.grouped_parameters.header.iter())
        .chain(endpoint.grouped_parameters.cookie.iter())
    {
        has_rows = true;
        render_parameter_row(builder, parameter);
    }

    if !has_rows {
        builder.push_row(DetailSection::Parameters, "None", None, None, None, "  ");
    }

    for unresolved in &endpoint.grouped_parameters.unresolved_refs {
        builder.push_row(
            DetailSection::Parameters,
            &format!("Unresolved parameter ref: {unresolved}"),
            None,
            None,
            None,
            "  ",
        );
    }
}

fn render_parameter_row(builder: &mut DocumentBuilder, parameter: &ParameterView) {
    let schema = parameter.schema.as_deref().unwrap_or("any");
    let required = if parameter.required { "yes" } else { "no" };
    let description = parameter.description.as_deref().unwrap_or("-");
    builder.push_row(
        DetailSection::Parameters,
        &format!(
            "  {} | {:<6} | required: {:<3} | {:<16} | {}",
            trim_to_width(&parameter.name, 24),
            parameter.location,
            required,
            trim_to_width(schema, 20),
            description
        ),
        None,
        None,
        None,
        "    ",
    );
}

fn render_request_body_section(
    builder: &mut DocumentBuilder,
    endpoint: &EndpointSummary,
    expanded_toggles: &HashSet<String>,
) {
    builder.push_section_header(DetailSection::RequestBody, "Request Body");

    let Some(request_body) = endpoint.request_body.as_ref() else {
        builder.push_row(DetailSection::RequestBody, "None", None, None, None, "  ");
        return;
    };

    let request_toggle = toggle_id(endpoint.id, "request_body");
    let request_expanded = expanded_toggles.contains(&request_toggle);
    builder.push_row(
        DetailSection::RequestBody,
        &format!(
            "{} request body ({})",
            fold_marker(request_expanded),
            if request_body.required {
                "required"
            } else {
                "optional"
            }
        ),
        Some("request_body".to_owned()),
        Some(request_toggle.clone()),
        None,
        "    ",
    );

    if let Some(unresolved_ref) = request_body.unresolved_ref.as_deref() {
        builder.push_row(
            DetailSection::RequestBody,
            &format!("  unresolved ref: {unresolved_ref}"),
            None,
            None,
            None,
            "    ",
        );
    }

    if !request_expanded {
        return;
    }

    if request_body.media_types.is_empty() {
        builder.push_row(
            DetailSection::RequestBody,
            "  media: none",
            None,
            None,
            None,
            "    ",
        );
        return;
    }

    for media in &request_body.media_types {
        let media_toggle = toggle_id(endpoint.id, &format!("request_body:{}", media.content_type));
        let media_expanded = expanded_toggles.contains(&media_toggle);
        let summary = media.schema.as_deref().unwrap_or("any");

        builder.push_row(
            DetailSection::RequestBody,
            &format!(
                "  {} {} :: {}",
                fold_marker(media_expanded),
                media.content_type,
                summary
            ),
            Some(format!("request_body:{}", media.content_type)),
            Some(media_toggle.clone()),
            None,
            "      ",
        );

        if media_expanded && let Some(schema_root) = media.schema_tree.as_ref() {
            let mut breadcrumb = vec!["request body".to_owned(), media.content_type.clone()];
            let schema_context = format!("request_body:{}", media.content_type);
            render_schema_node(
                builder,
                DetailSection::RequestBody,
                endpoint.id,
                &schema_context,
                schema_root,
                expanded_toggles,
                2,
                &mut breadcrumb,
            );
        }
    }
}

fn render_responses_section(
    builder: &mut DocumentBuilder,
    endpoint: &EndpointSummary,
    expanded_toggles: &HashSet<String>,
) {
    builder.push_section_header(DetailSection::Responses, "Responses");

    if endpoint.responses.is_empty() {
        builder.push_row(DetailSection::Responses, "None", None, None, None, "  ");
        return;
    }

    for response in &endpoint.responses {
        let response_toggle = toggle_id(endpoint.id, &format!("response:{}", response.status));
        let response_expanded = expanded_toggles.contains(&response_toggle);

        let description = response
            .description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("no description");

        builder.push_row(
            DetailSection::Responses,
            &format!(
                "{} {}: {}",
                fold_marker(response_expanded),
                response.status,
                description
            ),
            Some(format!("response:{}", response.status)),
            Some(response_toggle.clone()),
            None,
            "    ",
        );

        if let Some(unresolved_ref) = response.unresolved_ref.as_deref() {
            builder.push_row(
                DetailSection::Responses,
                &format!("  unresolved ref: {unresolved_ref}"),
                None,
                None,
                None,
                "    ",
            );
        }

        if !response_expanded {
            continue;
        }

        if response.media_types.is_empty() {
            builder.push_row(
                DetailSection::Responses,
                "  media: none",
                None,
                None,
                None,
                "    ",
            );
            continue;
        }

        for media in &response.media_types {
            let media_toggle = toggle_id(
                endpoint.id,
                &format!("response:{}:{}", response.status, media.content_type),
            );
            let media_expanded = expanded_toggles.contains(&media_toggle);
            let summary = media.schema.as_deref().unwrap_or("any");

            builder.push_row(
                DetailSection::Responses,
                &format!(
                    "  {} media {} :: {}",
                    fold_marker(media_expanded),
                    media.content_type,
                    summary
                ),
                Some(format!(
                    "response:{}:{}",
                    response.status, media.content_type
                )),
                Some(media_toggle.clone()),
                None,
                "      ",
            );

            if media_expanded && let Some(schema_root) = media.schema_tree.as_ref() {
                let mut breadcrumb = vec![
                    format!("response {}", response.status),
                    media.content_type.clone(),
                ];
                let schema_context = format!("response:{}:{}", response.status, media.content_type);
                render_schema_node(
                    builder,
                    DetailSection::Responses,
                    endpoint.id,
                    &schema_context,
                    schema_root,
                    expanded_toggles,
                    2,
                    &mut breadcrumb,
                );
            }
        }
    }
}

fn render_security_section(builder: &mut DocumentBuilder) {
    builder.push_section_header(DetailSection::Security, "Security");
    builder.push_row(
        DetailSection::Security,
        "No security details indexed.",
        None,
        None,
        None,
        "  ",
    );
}

fn render_schema_node(
    builder: &mut DocumentBuilder,
    section: DetailSection,
    endpoint_id: usize,
    schema_context: &str,
    node: &SchemaNode,
    expanded_toggles: &HashSet<String>,
    depth: usize,
    breadcrumb: &mut Vec<String>,
) {
    breadcrumb.push(node.label.clone());
    let breadcrumb_text = breadcrumb.join(" > ");

    let toggle_target = if node.children.is_empty() {
        None
    } else {
        Some(toggle_id(
            endpoint_id,
            &format!("{schema_context}:schema:{}", node.id),
        ))
    };
    let is_expanded = toggle_target
        .as_ref()
        .is_some_and(|target| expanded_toggles.contains(target));

    let indent = "  ".repeat(depth);
    let required_suffix = if node.required { " *required" } else { "" };
    let ref_suffix = node
        .ref_name
        .as_deref()
        .map(|name| format!(" [ref:{name}]"))
        .unwrap_or_default();
    let fold = if node.children.is_empty() {
        "[ ]"
    } else {
        fold_marker(is_expanded)
    };

    builder.push_row(
        section,
        &format!(
            "{indent}{fold} {} : {}{}{}",
            node.label, node.type_label, required_suffix, ref_suffix
        ),
        Some(format!("schema:{}", node.id)),
        toggle_target,
        Some(breadcrumb_text.clone()),
        &format!("{indent}    "),
    );

    if !node.enum_values.is_empty() {
        builder.push_row(
            section,
            &format!("{indent}    enum: {}", node.enum_values.join(", ")),
            None,
            None,
            Some(breadcrumb_text.clone()),
            &format!("{indent}      "),
        );
    }

    if let Some(example) = node.example.as_deref() {
        builder.push_row(
            section,
            &format!("{indent}    example: {example}"),
            None,
            None,
            Some(breadcrumb_text.clone()),
            &format!("{indent}      "),
        );
    }

    if let Some(description) = node.description.as_deref()
        && !description.trim().is_empty()
    {
        builder.push_row(
            section,
            &format!("{indent}    {description}"),
            None,
            None,
            Some(breadcrumb_text.clone()),
            &format!("{indent}      "),
        );
    }

    if !node.children.is_empty() && is_expanded {
        for child in &node.children {
            render_schema_node(
                builder,
                section,
                endpoint_id,
                schema_context,
                child,
                expanded_toggles,
                depth + 1,
                breadcrumb,
            );
        }
    }

    breadcrumb.pop();
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

    for (char_count, (index, ch)) in value.char_indices().enumerate() {
        if !ch.is_whitespace() && first_non_whitespace == value.len() {
            first_non_whitespace = index;
        }

        if char_count >= width {
            fallback_index = index;
            break;
        }

        if ch.is_whitespace() && index >= first_non_whitespace {
            last_space = Some(index);
        }
    }

    last_space
        .filter(|index| *index > 0)
        .unwrap_or(fallback_index)
}

fn hard_split_index(value: &str, width: usize) -> usize {
    for (char_count, (index, _)) in value.char_indices().enumerate() {
        if char_count >= width {
            return index;
        }
    }
    value.len()
}

fn fold_marker(expanded: bool) -> &'static str {
    if expanded { "[-]" } else { "[+]" }
}

fn toggle_id(endpoint_id: usize, key: &str) -> String {
    format!("endpoint:{endpoint_id}:{key}")
}

fn trim_to_width(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }

    let mut output = String::new();
    for (index, ch) in value.chars().enumerate() {
        if index >= max_chars.saturating_sub(3) {
            output.push_str("...");
            break;
        }
        output.push(ch);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::index::build_endpoint_index;
    use oas3::Spec;

    fn parse_spec(yaml: &str) -> Spec {
        serde_yaml::from_str::<Spec>(yaml).unwrap()
    }

    #[test]
    fn builds_toggle_rows_for_request_body_and_responses() {
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
          content:
            application/json:
              schema:
                type: object
                properties:
                  name:
                    type: string
"#,
        );

        let endpoints = build_endpoint_index(&spec);
        let endpoint = &endpoints[0];
        let expanded = HashSet::from_iter(["endpoint:0:request_body".to_owned()]);
        let doc = build_details_document(endpoint, 80, &expanded);

        assert!(doc.row_index_by_id("request_body").is_some());
        assert!(doc.row_index_by_id("response:200").is_some());
    }

    #[test]
    fn exposes_breadcrumb_for_schema_rows() {
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
                payload:
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
        let endpoint = &endpoints[0];
        let expanded = HashSet::from_iter([
            "endpoint:0:request_body".to_owned(),
            "endpoint:0:request_body:application/json".to_owned(),
            "endpoint:0:request_body:application/json:schema:schema".to_owned(),
            "endpoint:0:request_body:application/json:schema:schema/properties/payload".to_owned(),
        ]);

        let doc = build_details_document(endpoint, 90, &expanded);
        let breadcrumb = doc
            .lines
            .iter()
            .enumerate()
            .find_map(|(line_index, line)| {
                if line.contains("id : integer") {
                    doc.breadcrumb_for_line(line_index).map(str::to_owned)
                } else {
                    None
                }
            })
            .expect("breadcrumb missing for nested schema row");

        assert!(breadcrumb.contains("request body"));
        assert!(breadcrumb.contains("payload"));
        assert!(breadcrumb.contains("id"));
    }
}
