use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};

use oas3::spec::{
    ObjectOrReference, Operation, Parameter, ParameterIn, PathItem, RequestBody, Response, Spec,
};

use crate::spec::render::{summarize_media_type_schema, summarize_parameter_schema};
use crate::spec::schema_tree::{SchemaNode, build_schema_tree};

const METHOD_ORDER: [&str; 8] = [
    "GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD", "TRACE",
];
const UNTAGGED_GROUP: &str = "Untagged";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupedParameters {
    pub path: Vec<ParameterView>,
    pub query: Vec<ParameterView>,
    pub header: Vec<ParameterView>,
    pub cookie: Vec<ParameterView>,
    pub unresolved_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterView {
    pub name: String,
    pub location: String,
    pub required: bool,
    pub description: Option<String>,
    pub schema: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaTypeView {
    pub content_type: String,
    pub schema: Option<String>,
    pub schema_tree: Option<SchemaNode>,
    pub examples: Vec<MediaExampleView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaExampleView {
    pub name: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestBodyView {
    pub required: bool,
    pub media_types: Vec<MediaTypeView>,
    pub unresolved_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseView {
    pub status: String,
    pub description: Option<String>,
    pub media_types: Vec<MediaTypeView>,
    pub unresolved_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointSummary {
    pub id: usize,
    pub method: String,
    pub path: String,
    pub title: String,
    pub tags: Vec<String>,
    pub group_key: String,
    pub group_sort_key: String,
    pub description: Option<String>,
    pub operation_id: Option<String>,
    pub grouped_parameters: GroupedParameters,
    pub request_body: Option<RequestBodyView>,
    pub responses: Vec<ResponseView>,
    pub search_text: String,
}

pub fn build_endpoint_index(spec: &Spec) -> Vec<EndpointSummary> {
    let mut endpoints = Vec::new();
    let Some(paths) = spec.paths.as_ref() else {
        return endpoints;
    };

    for (path, path_item) in paths {
        let resolved_path_item = resolve_path_item(path_item, spec);
        for (method, operation) in operations_in_order(&resolved_path_item) {
            let title = operation
                .summary
                .clone()
                .unwrap_or_else(|| format!("{method} {path}"));
            let tags = normalize_operation_tags(&operation.tags);
            let group_key = derive_group_key(&tags, path);
            let group_sort_key = normalize_group_sort_key(&group_key);

            endpoints.push(EndpointSummary {
                id: 0,
                method: method.to_owned(),
                path: path.clone(),
                title: title.clone(),
                tags: tags.clone(),
                group_key: group_key.clone(),
                group_sort_key,
                description: operation.description.clone(),
                operation_id: operation.operation_id.clone(),
                grouped_parameters: merge_parameters(&resolved_path_item, operation, spec),
                request_body: build_request_body_view(operation.request_body.as_ref(), spec),
                responses: build_response_views(operation.responses.as_ref(), spec),
                search_text: build_search_text(
                    method,
                    path,
                    &title,
                    operation.operation_id.as_deref(),
                    operation.description.as_deref(),
                    &group_key,
                    &tags,
                ),
            });
        }
    }

    endpoints.sort_by(compare_endpoint_order);

    for (id, endpoint) in endpoints.iter_mut().enumerate() {
        endpoint.id = id;
    }

    endpoints
}

fn operations_in_order(path_item: &PathItem) -> Vec<(&'static str, &Operation)> {
    let mut operations = Vec::new();
    macro_rules! push {
        ($field:ident, $method:literal) => {
            if let Some(operation) = path_item.$field.as_ref() {
                operations.push(($method, operation));
            }
        };
    }
    push!(get, "GET");
    push!(post, "POST");
    push!(put, "PUT");
    push!(patch, "PATCH");
    push!(delete, "DELETE");
    push!(options, "OPTIONS");
    push!(head, "HEAD");
    push!(trace, "TRACE");
    operations
}

fn resolve_path_item(path_item: &PathItem, spec: &Spec) -> PathItem {
    let mut visited = HashSet::new();
    resolve_path_item_inner(path_item, spec, &mut visited).unwrap_or_else(|| path_item.clone())
}

fn resolve_path_item_inner(
    path_item: &PathItem,
    spec: &Spec,
    visited: &mut HashSet<String>,
) -> Option<PathItem> {
    let Some(reference) = path_item.reference.as_deref() else {
        return Some(path_item.clone());
    };

    resolve_path_item_reference(reference, spec, visited).or_else(|| Some(path_item.clone()))
}

fn resolve_path_item_reference(
    reference: &str,
    spec: &Spec,
    visited: &mut HashSet<String>,
) -> Option<PathItem> {
    if !visited.insert(reference.to_owned()) {
        return None;
    }

    let result = (|| {
        let name = decode_json_pointer_token(reference.strip_prefix("#/components/pathItems/")?);
        let components = spec.components.as_ref()?;
        let path_item_ref = components.path_items.get(&name)?;
        match path_item_ref {
            ObjectOrReference::Object(path_item) => {
                resolve_path_item_inner(path_item, spec, visited)
            }
            ObjectOrReference::Ref { ref_path, .. } => {
                resolve_path_item_reference(ref_path, spec, visited)
            }
        }
    })();

    visited.remove(reference);
    result
}

fn decode_json_pointer_token(token: &str) -> String {
    token.replace("~1", "/").replace("~0", "~")
}

fn method_rank(method: &str) -> usize {
    METHOD_ORDER
        .iter()
        .position(|candidate| *candidate == method)
        .unwrap_or(METHOD_ORDER.len())
}

fn normalize_operation_tags(tags: &[String]) -> Vec<String> {
    tags.iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn derive_group_key(tags: &[String], path: &str) -> String {
    if let Some(tag) = tags.first() {
        return tag.clone();
    }

    if let Some(segment) = first_meaningful_path_segment(path) {
        return segment;
    }

    UNTAGGED_GROUP.to_owned()
}

fn first_meaningful_path_segment(path: &str) -> Option<String> {
    path.split('/')
        .map(str::trim)
        .find(|segment| {
            !segment.is_empty()
                && !(segment.starts_with('{') && segment.ends_with('}') && segment.len() >= 2)
        })
        .map(str::to_owned)
}

fn normalize_group_sort_key(group_key: &str) -> String {
    group_key.trim().to_ascii_lowercase()
}

fn is_untagged_group(group_key: &str) -> bool {
    group_key == UNTAGGED_GROUP
}

fn compare_group_order(left: &EndpointSummary, right: &EndpointSummary) -> Ordering {
    match (
        is_untagged_group(&left.group_key),
        is_untagged_group(&right.group_key),
    ) {
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        _ => left
            .group_sort_key
            .cmp(&right.group_sort_key)
            .then(left.group_key.cmp(&right.group_key)),
    }
}

fn compare_endpoint_order(left: &EndpointSummary, right: &EndpointSummary) -> Ordering {
    compare_group_order(left, right)
        .then(left.path.cmp(&right.path))
        .then(method_rank(&left.method).cmp(&method_rank(&right.method)))
        .then(left.method.cmp(&right.method))
        .then(left.title.cmp(&right.title))
}

fn merge_parameters(path_item: &PathItem, operation: &Operation, spec: &Spec) -> GroupedParameters {
    let mut merged = BTreeMap::new();
    let mut unresolved_refs = Vec::new();

    insert_parameters(
        &path_item.parameters,
        spec,
        &mut merged,
        &mut unresolved_refs,
    );
    insert_parameters(
        &operation.parameters,
        spec,
        &mut merged,
        &mut unresolved_refs,
    );

    let mut grouped = GroupedParameters {
        path: Vec::new(),
        query: Vec::new(),
        header: Vec::new(),
        cookie: Vec::new(),
        unresolved_refs,
    };

    for parameter in merged.into_values() {
        match parameter.location.as_str() {
            "path" => grouped.path.push(parameter),
            "query" => grouped.query.push(parameter),
            "header" => grouped.header.push(parameter),
            "cookie" => grouped.cookie.push(parameter),
            _ => grouped.query.push(parameter),
        }
    }

    grouped
        .path
        .sort_by(|left, right| left.name.cmp(&right.name));
    grouped
        .query
        .sort_by(|left, right| left.name.cmp(&right.name));
    grouped
        .header
        .sort_by(|left, right| left.name.cmp(&right.name));
    grouped
        .cookie
        .sort_by(|left, right| left.name.cmp(&right.name));

    grouped
}

fn insert_parameters(
    parameters: &[ObjectOrReference<Parameter>],
    spec: &Spec,
    target: &mut BTreeMap<(String, String), ParameterView>,
    unresolved_refs: &mut Vec<String>,
) {
    for parameter_ref in parameters {
        let parameter = match parameter_ref.resolve(spec) {
            Ok(parameter) => parameter,
            Err(_) => {
                if let ObjectOrReference::Ref { ref_path, .. } = parameter_ref {
                    unresolved_refs.push(ref_path.clone());
                }
                continue;
            }
        };
        let location = parameter_location_label(parameter.location);
        let key = (parameter.name.clone(), location.clone());
        target.insert(
            key,
            ParameterView {
                name: parameter.name.clone(),
                location,
                required: parameter
                    .required
                    .unwrap_or(matches!(parameter.location, ParameterIn::Path)),
                description: parameter.description.clone(),
                schema: summarize_parameter_schema(&parameter, spec),
            },
        );
    }
}

fn parameter_location_label(location: ParameterIn) -> String {
    match location {
        ParameterIn::Path => "path",
        ParameterIn::Query => "query",
        ParameterIn::Header => "header",
        ParameterIn::Cookie => "cookie",
    }
    .to_owned()
}

fn build_request_body_view(
    request_body: Option<&ObjectOrReference<RequestBody>>,
    spec: &Spec,
) -> Option<RequestBodyView> {
    let request_body = request_body?;
    match request_body.resolve(spec) {
        Ok(request_body) => Some(RequestBodyView {
            required: request_body.required.unwrap_or(false),
            media_types: build_media_type_views(&request_body.content, spec),
            unresolved_ref: None,
        }),
        Err(_) => Some(RequestBodyView {
            required: false,
            media_types: Vec::new(),
            unresolved_ref: extract_ref_path(request_body),
        }),
    }
}

fn build_response_views(
    responses: Option<&BTreeMap<String, ObjectOrReference<Response>>>,
    spec: &Spec,
) -> Vec<ResponseView> {
    let Some(responses) = responses else {
        return Vec::new();
    };

    let mut rendered = Vec::new();
    for (status, response_ref) in responses {
        match response_ref.resolve(spec) {
            Ok(response) => rendered.push(ResponseView {
                status: status.clone(),
                description: response.description.clone(),
                media_types: build_media_type_views(&response.content, spec),
                unresolved_ref: None,
            }),
            Err(_) => rendered.push(ResponseView {
                status: status.clone(),
                description: extract_ref_path(response_ref)
                    .map(|path| format!("Unresolved response reference: {path}")),
                media_types: Vec::new(),
                unresolved_ref: extract_ref_path(response_ref),
            }),
        }
    }

    rendered
}

fn build_media_type_views(
    media_types: &BTreeMap<String, oas3::spec::MediaType>,
    spec: &Spec,
) -> Vec<MediaTypeView> {
    let mut rendered = Vec::new();
    for (content_type, media_type) in media_types {
        let examples = media_type
            .examples(spec)
            .into_iter()
            .map(|(name, example)| MediaExampleView {
                name,
                summary: normalize_optional_text(example.summary),
                description: normalize_optional_text(example.description),
                value: example
                    .value
                    .as_ref()
                    .and_then(|value| serde_json::to_string_pretty(value).ok()),
            })
            .collect::<Vec<_>>();

        rendered.push(MediaTypeView {
            content_type: content_type.clone(),
            schema: summarize_media_type_schema(media_type, spec),
            schema_tree: media_type
                .schema
                .as_ref()
                .map(|schema| build_schema_tree(schema, spec, "schema")),
            examples,
        });
    }
    rendered
}

fn build_search_text(
    method: &str,
    path: &str,
    title: &str,
    operation_id: Option<&str>,
    description: Option<&str>,
    group_key: &str,
    tags: &[String],
) -> String {
    format!(
        "{} {} {} {} {} {} {}",
        method,
        path,
        title,
        operation_id.unwrap_or_default(),
        description.unwrap_or_default(),
        group_key,
        tags.join(" ")
    )
    .to_ascii_lowercase()
}

fn extract_ref_path<T>(value: &ObjectOrReference<T>) -> Option<String> {
    if let ObjectOrReference::Ref { ref_path, .. } = value {
        Some(ref_path.clone())
    } else {
        None
    }
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oas3::Spec;

    fn parse_spec(yaml: &str) -> Spec {
        serde_yaml::from_str::<Spec>(yaml).unwrap()
    }

    #[test]
    fn merges_path_and_operation_parameters_with_override() {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths:
  /pets:
    parameters:
      - name: limit
        in: query
        required: false
        schema:
          type: integer
    get:
      summary: list pets
      parameters:
        - name: limit
          in: query
          required: true
          schema:
            type: number
        - name: x-trace-id
          in: header
          schema:
            type: string
      responses:
        "200":
          description: ok
"#,
        );

        let endpoints = build_endpoint_index(&spec);
        assert_eq!(endpoints.len(), 1);

        let endpoint = &endpoints[0];
        assert_eq!(endpoint.grouped_parameters.query.len(), 1);
        assert_eq!(endpoint.grouped_parameters.header.len(), 1);
        assert!(endpoint.grouped_parameters.query[0].required);
        assert_eq!(endpoint.grouped_parameters.query[0].name, "limit");
        assert_eq!(
            endpoint.grouped_parameters.query[0].schema.as_deref(),
            Some("number")
        );
    }

    #[test]
    fn sorts_methods_by_defined_order() {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths:
  /items:
    delete:
      responses:
        "204":
          description: no content
    get:
      responses:
        "200":
          description: ok
    post:
      responses:
        "201":
          description: created
"#,
        );

        let endpoints = build_endpoint_index(&spec);
        let methods = endpoints
            .iter()
            .filter(|endpoint| endpoint.path == "/items")
            .map(|endpoint| endpoint.method.clone())
            .collect::<Vec<_>>();
        assert_eq!(methods, vec!["GET", "POST", "DELETE"]);
    }

    #[test]
    fn derives_groups_with_tag_then_path_segment_then_untagged_fallback() {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths:
  /pets:
    get:
      tags: ["Animals"]
      responses:
        "200":
          description: ok
  /users/{id}:
    get:
      responses:
        "200":
          description: ok
  /{id}:
    get:
      responses:
        "200":
          description: ok
"#,
        );

        let endpoints = build_endpoint_index(&spec);
        assert_eq!(endpoints.len(), 3);

        let pets = endpoints
            .iter()
            .find(|endpoint| endpoint.path == "/pets")
            .expect("pets endpoint missing");
        assert_eq!(pets.tags, vec!["Animals".to_owned()]);
        assert_eq!(pets.group_key, "Animals");
        assert_eq!(pets.group_sort_key, "animals");

        let users = endpoints
            .iter()
            .find(|endpoint| endpoint.path == "/users/{id}")
            .expect("users endpoint missing");
        assert_eq!(users.tags, Vec::<String>::new());
        assert_eq!(users.group_key, "users");
        assert_eq!(users.group_sort_key, "users");

        let parameter_only = endpoints
            .iter()
            .find(|endpoint| endpoint.path == "/{id}")
            .expect("parameter-only endpoint missing");
        assert_eq!(parameter_only.group_key, "Untagged");
        assert_eq!(parameter_only.group_sort_key, "untagged");
    }

    #[test]
    fn sorts_groups_alphabetically_and_keeps_untagged_last() {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths:
  /zeta:
    get:
      tags: ["zeta"]
      responses:
        "200":
          description: ok
  /alpha-post:
    post:
      tags: ["alpha"]
      responses:
        "200":
          description: ok
  /alpha-get:
    get:
      tags: ["alpha"]
      responses:
        "200":
          description: ok
  /{id}:
    get:
      responses:
        "200":
          description: ok
"#,
        );

        let endpoints = build_endpoint_index(&spec);
        let ordered = endpoints
            .iter()
            .map(|endpoint| {
                (
                    endpoint.group_key.clone(),
                    endpoint.path.clone(),
                    endpoint.method.clone(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            ordered,
            vec![
                (
                    "alpha".to_owned(),
                    "/alpha-get".to_owned(),
                    "GET".to_owned()
                ),
                (
                    "alpha".to_owned(),
                    "/alpha-post".to_owned(),
                    "POST".to_owned()
                ),
                ("zeta".to_owned(), "/zeta".to_owned(), "GET".to_owned()),
                ("Untagged".to_owned(), "/{id}".to_owned(), "GET".to_owned()),
            ]
        );
    }

    #[test]
    fn search_text_includes_group_and_tags() {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths:
  /pets:
    get:
      tags: [" Animals ", "catalog"]
      operationId: listPets
      description: List all pets in inventory
      responses:
        "200":
          description: ok
"#,
        );

        let endpoints = build_endpoint_index(&spec);
        assert_eq!(endpoints.len(), 1);
        let search_text = endpoints[0].search_text.clone();

        assert!(search_text.contains("animals"));
        assert!(search_text.contains("catalog"));
        assert!(search_text.contains("listpets"));
        assert!(search_text.contains("inventory"));
    }

    #[test]
    fn renders_unresolved_refs_as_placeholders() {
        let spec = parse_spec(
            r##"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths:
  /items:
    get:
      requestBody:
        $ref: "#/components/requestBodies/Missing"
      responses:
        "200":
          $ref: "#/components/responses/Missing"
"##,
        );

        let endpoints = build_endpoint_index(&spec);
        let endpoint = &endpoints[0];

        assert_eq!(
            endpoint
                .request_body
                .as_ref()
                .and_then(|body| body.unresolved_ref.as_deref()),
            Some("#/components/requestBodies/Missing")
        );
        assert_eq!(
            endpoint.responses[0].unresolved_ref.as_deref(),
            Some("#/components/responses/Missing")
        );
    }

    #[test]
    fn resolves_path_item_component_references() {
        let spec = parse_spec(
            r##"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths:
  /pets:
    $ref: "#/components/pathItems/PetsPath"
components:
  pathItems:
    PetsPath:
      get:
        summary: list pets
        responses:
          "200":
            description: ok
"##,
        );

        let endpoints = build_endpoint_index(&spec);
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].path, "/pets");
        assert_eq!(endpoints[0].method, "GET");
    }

    #[test]
    fn resolves_nested_path_item_component_references() {
        let spec = parse_spec(
            r##"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths:
  /pets:
    $ref: "#/components/pathItems/First"
components:
  pathItems:
    First:
      $ref: "#/components/pathItems/Second"
    Second:
      post:
        summary: create pet
        responses:
          "201":
            description: created
"##,
        );

        let endpoints = build_endpoint_index(&spec);
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].path, "/pets");
        assert_eq!(endpoints[0].method, "POST");
    }
}
