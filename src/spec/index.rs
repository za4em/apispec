use std::collections::{BTreeMap, HashSet};

use oas3::spec::{
    ObjectOrReference, Operation, Parameter, ParameterIn, PathItem, RequestBody, Response, Spec,
};

use crate::spec::render::{summarize_media_type_schema, summarize_parameter_schema};

const METHOD_ORDER: [&str; 8] = [
    "GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD", "TRACE",
];

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

            endpoints.push(EndpointSummary {
                id: 0,
                method: method.to_owned(),
                path: path.clone(),
                title: title.clone(),
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
                ),
            });
        }
    }

    endpoints.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(method_rank(&left.method).cmp(&method_rank(&right.method)))
            .then(left.method.cmp(&right.method))
    });

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
        rendered.push(MediaTypeView {
            content_type: content_type.clone(),
            schema: summarize_media_type_schema(media_type, spec),
        });
    }
    rendered
}

fn build_search_text(method: &str, path: &str, title: &str, operation_id: Option<&str>) -> String {
    format!(
        "{} {} {} {}",
        method,
        path,
        title,
        operation_id.unwrap_or_default()
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
            .map(|endpoint| endpoint.method.clone())
            .collect::<Vec<_>>();
        assert_eq!(methods, vec!["GET", "POST", "DELETE"]);
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
