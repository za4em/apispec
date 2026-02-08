use std::collections::{HashMap, HashSet};

use crate::spec::index::{EndpointSummary, MediaExampleView, ParameterView};
use crate::spec::schema_tree::SchemaNode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailSection {
    Overview,
    Parameters,
    RequestBody,
    Responses,
}

impl DetailSection {
    pub fn next(self) -> Self {
        match self {
            Self::Overview => Self::Parameters,
            Self::Parameters => Self::RequestBody,
            Self::RequestBody => Self::Responses,
            Self::Responses => Self::Overview,
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

    pub fn previous_toggle_line_start(&self, line_index: usize, steps: usize) -> Option<usize> {
        let mut toggle_index = self.toggle_index_for_line_or_next(line_index)?;
        for _ in 0..steps {
            if toggle_index == 0 {
                break;
            }
            toggle_index -= 1;
        }
        self.toggle_rows
            .get(toggle_index)
            .and_then(|row_index| self.rows.get(*row_index))
            .map(|row| row.line_start)
    }

    pub fn next_toggle_line_start(&self, line_index: usize, steps: usize) -> Option<usize> {
        let mut toggle_index = self.toggle_index_for_line_or_next(line_index)?;
        for _ in 0..steps {
            let next = toggle_index.saturating_add(1);
            if next >= self.toggle_rows.len() {
                break;
            }
            toggle_index = next;
        }
        self.toggle_rows
            .get(toggle_index)
            .and_then(|row_index| self.rows.get(*row_index))
            .map(|row| row.line_start)
    }

    pub fn breadcrumb_for_line(&self, line_index: usize) -> Option<&str> {
        self.row_for_line(line_index)
            .and_then(|row| row.breadcrumb.as_deref())
    }

    pub fn row_for_line(&self, line_index: usize) -> Option<&DetailRow> {
        self.rows.iter().find(|row| {
            let row_end = row.line_start.saturating_add(row.line_len.max(1));
            line_index >= row.line_start && line_index < row_end
        })
    }

    pub fn row_index_by_id(&self, row_id: &str) -> Option<usize> {
        self.row_index_by_id.get(row_id).copied()
    }

    fn toggle_index_for_line_or_next(&self, line_index: usize) -> Option<usize> {
        if self.toggle_rows.is_empty() {
            return None;
        }

        if let Some(found) = self.toggle_rows.iter().position(|row_index| {
            self.rows.get(*row_index).is_some_and(|row| {
                let row_end = row.line_start.saturating_add(row.line_len.max(1));
                line_index >= row.line_start && line_index < row_end
            })
        }) {
            return Some(found);
        }

        if let Some(found) = self.toggle_rows.iter().position(|row_index| {
            self.rows
                .get(*row_index)
                .is_some_and(|row| row.line_start > line_index)
        }) {
            return Some(found);
        }

        Some(self.toggle_rows.len().saturating_sub(1))
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
    has_rows |= render_parameter_group(builder, "Path", &endpoint.grouped_parameters.path);
    has_rows |= render_parameter_group(builder, "Query", &endpoint.grouped_parameters.query);
    has_rows |= render_parameter_group(builder, "Header", &endpoint.grouped_parameters.header);
    has_rows |= render_parameter_group(builder, "Cookie", &endpoint.grouped_parameters.cookie);

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

fn render_parameter_group(
    builder: &mut DocumentBuilder,
    label: &str,
    parameters: &[ParameterView],
) -> bool {
    if parameters.is_empty() {
        return false;
    }

    builder.push_row(
        DetailSection::Parameters,
        &format!("{label} parameters"),
        None,
        None,
        None,
        "  ",
    );

    for parameter in parameters {
        render_parameter_row(builder, parameter);
    }

    true
}

fn render_parameter_row(builder: &mut DocumentBuilder, parameter: &ParameterView) {
    builder.push_row(
        DetailSection::Parameters,
        &format!(
            "  - {} ({})",
            parameter.name,
            if parameter.required {
                "required"
            } else {
                "optional"
            }
        ),
        None,
        None,
        None,
        "    ",
    );

    builder.push_row(
        DetailSection::Parameters,
        &format!(
            "    schema: {}",
            parameter.schema.as_deref().unwrap_or("any")
        ),
        None,
        None,
        None,
        "      ",
    );

    if let Some(description) = parameter.description.as_deref() {
        if !description.trim().is_empty() {
            builder.push_row(
                DetailSection::Parameters,
                &format!("    description: {description}"),
                None,
                None,
                None,
                "      ",
            );
        }
    }

    builder.push_row(
        DetailSection::Parameters,
        &format!("    in: {}", parameter.location),
        None,
        None,
        None,
        "      ",
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

        builder.push_row(
            DetailSection::RequestBody,
            &format!("  {} {}", fold_marker(media_expanded), media.content_type),
            Some(format!("request_body:{}", media.content_type)),
            Some(media_toggle.clone()),
            None,
            "      ",
        );

        if media_expanded {
            let schema_context = format!("request_body:{}", media.content_type);
            render_schema_subsection(
                builder,
                DetailSection::RequestBody,
                endpoint.id,
                &schema_context,
                &["request body".to_owned(), media.content_type.clone()],
                media.schema_tree.as_ref(),
                expanded_toggles,
                2,
            );
            render_examples_subsection(
                builder,
                DetailSection::RequestBody,
                endpoint.id,
                &schema_context,
                &["request body".to_owned(), media.content_type.clone()],
                &media.examples,
                expanded_toggles,
                2,
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

            builder.push_row(
                DetailSection::Responses,
                &format!("  {} {}", fold_marker(media_expanded), media.content_type),
                Some(format!(
                    "response:{}:{}",
                    response.status, media.content_type
                )),
                Some(media_toggle.clone()),
                None,
                "      ",
            );

            if media_expanded {
                let schema_context = format!("response:{}:{}", response.status, media.content_type);
                render_schema_subsection(
                    builder,
                    DetailSection::Responses,
                    endpoint.id,
                    &schema_context,
                    &[
                        format!("response {}", response.status),
                        media.content_type.clone(),
                    ],
                    media.schema_tree.as_ref(),
                    expanded_toggles,
                    2,
                );
                render_examples_subsection(
                    builder,
                    DetailSection::Responses,
                    endpoint.id,
                    &schema_context,
                    &[
                        format!("response {}", response.status),
                        media.content_type.clone(),
                    ],
                    &media.examples,
                    expanded_toggles,
                    2,
                );
            }
        }
    }
}

fn render_schema_subsection(
    builder: &mut DocumentBuilder,
    section: DetailSection,
    endpoint_id: usize,
    schema_context: &str,
    breadcrumb_prefix: &[String],
    schema_root: Option<&SchemaNode>,
    expanded_toggles: &HashSet<String>,
    depth: usize,
) {
    let indent = "  ".repeat(depth);
    let Some(schema_root) = schema_root else {
        builder.push_row(
            section,
            &format!("{indent}[ ] schema: none"),
            None,
            None,
            Some(breadcrumb_prefix.join(" > ")),
            &format!("{indent}    "),
        );
        return;
    };

    let schema_toggle = toggle_id(endpoint_id, &format!("{schema_context}:schema"));
    let schema_expanded = expanded_toggles.contains(&schema_toggle);
    builder.push_row(
        section,
        &format!("{indent}{} schema", fold_marker(schema_expanded)),
        Some(format!("{schema_context}:schema")),
        Some(schema_toggle),
        Some(breadcrumb_prefix.join(" > ")),
        &format!("{indent}    "),
    );

    if schema_expanded {
        let mut breadcrumb = breadcrumb_prefix.to_owned();
        breadcrumb.push("schema".to_owned());
        render_schema_node(
            builder,
            section,
            endpoint_id,
            schema_context,
            schema_root,
            expanded_toggles,
            depth + 1,
            &mut breadcrumb,
        );
    }
}

fn render_examples_subsection(
    builder: &mut DocumentBuilder,
    section: DetailSection,
    endpoint_id: usize,
    schema_context: &str,
    breadcrumb_prefix: &[String],
    examples: &[MediaExampleView],
    expanded_toggles: &HashSet<String>,
    depth: usize,
) {
    if examples.is_empty() {
        return;
    }

    let indent = "  ".repeat(depth);
    let examples_toggle = toggle_id(endpoint_id, &format!("{schema_context}:examples"));
    let examples_expanded = expanded_toggles.contains(&examples_toggle);
    let count = examples.len();
    builder.push_row(
        section,
        &format!(
            "{indent}{} example{} ({count})",
            fold_marker(examples_expanded),
            if count == 1 { "" } else { "s" }
        ),
        Some(format!("{schema_context}:examples")),
        Some(examples_toggle),
        Some(breadcrumb_prefix.join(" > ")),
        &format!("{indent}    "),
    );

    if !examples_expanded {
        return;
    }

    for example in examples {
        render_media_example(builder, section, example, depth + 1, breadcrumb_prefix);
    }
}

fn render_media_example(
    builder: &mut DocumentBuilder,
    section: DetailSection,
    example: &MediaExampleView,
    depth: usize,
    breadcrumb_prefix: &[String],
) {
    let indent = "  ".repeat(depth);
    let mut breadcrumb = breadcrumb_prefix.join(" > ");
    breadcrumb.push_str(" > example ");
    breadcrumb.push_str(&example.name);

    builder.push_row(
        section,
        &format!("{indent}- {}", example.name),
        None,
        None,
        Some(breadcrumb.clone()),
        &format!("{indent}  "),
    );

    if let Some(summary) = example.summary.as_deref() {
        builder.push_row(
            section,
            &format!("{indent}  summary: {summary}"),
            None,
            None,
            Some(breadcrumb.clone()),
            &format!("{indent}    "),
        );
    }

    if let Some(description) = example.description.as_deref() {
        builder.push_row(
            section,
            &format!("{indent}  description: {description}"),
            None,
            None,
            Some(breadcrumb.clone()),
            &format!("{indent}    "),
        );
    }

    if let Some(value) = example.value.as_deref() {
        builder.push_row(
            section,
            &format!("{indent}  value:"),
            None,
            None,
            Some(breadcrumb.clone()),
            &format!("{indent}    "),
        );

        for line in value.lines() {
            builder.push_row(
                section,
                &format!("{indent}    {line}"),
                None,
                None,
                Some(breadcrumb.clone()),
                &format!("{indent}    "),
            );
        }
    }
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
    let fold = if node.children.is_empty() {
        "[ ]"
    } else {
        fold_marker(is_expanded)
    };

    builder.push_row(
        section,
        &format!(
            "{indent}{fold} {} : {}{}",
            node.label, node.type_label, required_suffix
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
            "endpoint:0:request_body:application/json:schema".to_owned(),
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

    #[test]
    fn exposes_toggle_navigation_helpers() {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths:
  /items:
    get:
      responses:
        "200":
          description: ok
"#,
        );
        let endpoints = build_endpoint_index(&spec);
        let endpoint = &endpoints[0];
        let doc = build_details_document(
            endpoint,
            80,
            &HashSet::from_iter(["endpoint:0:response:200".to_owned()]),
        );

        let first = doc
            .nearest_toggle_row(0)
            .expect("detail document should include toggle rows")
            .line_start;
        let second = doc
            .next_toggle_line_start(first, 1)
            .expect("second toggle row should exist");

        assert_eq!(doc.previous_toggle_line_start(second, 1), Some(first));
    }
}
