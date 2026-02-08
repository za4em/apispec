use std::collections::HashSet;

use oas3::spec::{ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet, Spec};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaNode {
    pub id: String,
    pub label: String,
    pub type_label: String,
    pub required: bool,
    pub enum_values: Vec<String>,
    pub description: Option<String>,
    pub example: Option<String>,
    pub ref_name: Option<String>,
    pub children: Vec<SchemaNode>,
}

#[derive(Default)]
struct BuildContext {
    visited_ref_stack: HashSet<String>,
}

pub fn build_schema_tree(
    schema: &ObjectOrReference<ObjectSchema>,
    spec: &Spec,
    root_id: &str,
) -> SchemaNode {
    let mut context = BuildContext::default();
    build_schema_ref(schema, spec, root_id, "schema", false, &mut context)
}

fn build_schema_ref(
    schema: &ObjectOrReference<ObjectSchema>,
    spec: &Spec,
    id: &str,
    label: &str,
    required: bool,
    context: &mut BuildContext,
) -> SchemaNode {
    match schema {
        ObjectOrReference::Object(object) => {
            build_object_schema_node(object, spec, id, label, required, None, context)
        }
        ObjectOrReference::Ref { ref_path, .. } => {
            let ref_name = short_ref_label(ref_path);
            if !context.visited_ref_stack.insert(ref_path.clone()) {
                return SchemaNode {
                    id: id.to_owned(),
                    label: label.to_owned(),
                    type_label: format!("[cycle: {ref_name}]"),
                    required,
                    enum_values: Vec::new(),
                    description: None,
                    example: None,
                    ref_name: Some(ref_name),
                    children: Vec::new(),
                };
            }

            let node = match schema.resolve(spec) {
                Ok(object) => build_object_schema_node(
                    &object,
                    spec,
                    id,
                    label,
                    required,
                    Some(short_ref_label(ref_path)),
                    context,
                ),
                Err(_) => SchemaNode {
                    id: id.to_owned(),
                    label: label.to_owned(),
                    type_label: format!("[unresolved: {ref_path}]"),
                    required,
                    enum_values: Vec::new(),
                    description: None,
                    example: None,
                    ref_name: Some(short_ref_label(ref_path)),
                    children: Vec::new(),
                },
            };

            context.visited_ref_stack.remove(ref_path);
            node
        }
    }
}

fn build_schema_document(
    schema: &Schema,
    spec: &Spec,
    id: &str,
    label: &str,
    required: bool,
    context: &mut BuildContext,
) -> SchemaNode {
    match schema {
        Schema::Boolean(value) => SchemaNode {
            id: id.to_owned(),
            label: label.to_owned(),
            type_label: if value.0 {
                "any".to_owned()
            } else {
                "never".to_owned()
            },
            required,
            enum_values: Vec::new(),
            description: None,
            example: None,
            ref_name: None,
            children: Vec::new(),
        },
        Schema::Object(object) => {
            build_schema_ref(object.as_ref(), spec, id, label, required, context)
        }
    }
}

fn build_object_schema_node(
    schema: &ObjectSchema,
    spec: &Spec,
    id: &str,
    label: &str,
    required: bool,
    ref_name: Option<String>,
    context: &mut BuildContext,
) -> SchemaNode {
    let raw = serde_json::to_value(schema)
        .ok()
        .and_then(|value| value.as_object().cloned());

    let mut children = Vec::new();

    if let Some(items) = schema.items.as_ref() {
        let child_id = format!("{id}/items");
        children.push(build_schema_document(
            items, spec, &child_id, "items", true, context,
        ));
    }

    let required_names = schema.required.iter().collect::<HashSet<_>>();
    for (property_name, property_schema) in &schema.properties {
        let child_id = format!("{id}/properties/{}", escape_id_segment(property_name));
        let is_required = required_names.contains(property_name);
        children.push(build_schema_ref(
            property_schema,
            spec,
            &child_id,
            property_name,
            is_required,
            context,
        ));
    }

    if let Some(raw_object) = raw.as_ref() {
        children.extend(read_composed_children(raw_object, spec, id, context));

        if let Some(additional_properties) = raw_object.get("additionalProperties") {
            let child_id = format!("{id}/additionalProperties");
            if let Ok(parsed) = serde_json::from_value::<Schema>(additional_properties.clone()) {
                children.push(build_schema_document(
                    &parsed,
                    spec,
                    &child_id,
                    "additionalProperties",
                    false,
                    context,
                ));
            }
        }
    }

    SchemaNode {
        id: id.to_owned(),
        label: label.to_owned(),
        type_label: infer_type_label(schema, raw.as_ref()),
        required,
        enum_values: extract_enum_values(raw.as_ref()),
        description: extract_description(raw.as_ref()),
        example: extract_example(raw.as_ref()),
        ref_name,
        children,
    }
}

fn read_composed_children(
    raw: &serde_json::Map<String, Value>,
    spec: &Spec,
    id: &str,
    context: &mut BuildContext,
) -> Vec<SchemaNode> {
    let mut children = Vec::new();

    for key in ["oneOf", "anyOf", "allOf"] {
        let Some(value) = raw.get(key) else {
            continue;
        };
        let Some(values) = value.as_array() else {
            continue;
        };

        for (index, variant) in values.iter().enumerate() {
            let child_id = format!("{id}/{key}/{index}");
            let child_label = format!("{key}[{index}]");
            match serde_json::from_value::<Schema>(variant.clone()) {
                Ok(parsed) => children.push(build_schema_document(
                    &parsed,
                    spec,
                    &child_id,
                    &child_label,
                    false,
                    context,
                )),
                Err(_) => children.push(SchemaNode {
                    id: child_id,
                    label: child_label,
                    type_label: "[unsupported composed schema]".to_owned(),
                    required: false,
                    enum_values: Vec::new(),
                    description: None,
                    example: None,
                    ref_name: None,
                    children: Vec::new(),
                }),
            }
        }
    }

    if let Some(not_schema) = raw.get("not") {
        let child_id = format!("{id}/not");
        match serde_json::from_value::<Schema>(not_schema.clone()) {
            Ok(parsed) => children.push(build_schema_document(
                &parsed, spec, &child_id, "not", false, context,
            )),
            Err(_) => children.push(SchemaNode {
                id: child_id,
                label: "not".to_owned(),
                type_label: "[unsupported not schema]".to_owned(),
                required: false,
                enum_values: Vec::new(),
                description: None,
                example: None,
                ref_name: None,
                children: Vec::new(),
            }),
        }
    }

    children
}

fn infer_type_label(schema: &ObjectSchema, raw: Option<&serde_json::Map<String, Value>>) -> String {
    let mut labels = match schema.schema_type.as_ref() {
        Some(SchemaTypeSet::Single(single)) => vec![schema_type_label(*single)],
        Some(SchemaTypeSet::Multiple(multiple)) => {
            multiple.iter().copied().map(schema_type_label).collect()
        }
        None => Vec::new(),
    };

    if labels.is_empty() {
        if schema.items.is_some() {
            labels.push("array".to_owned());
        } else if !schema.properties.is_empty() {
            labels.push("object".to_owned());
        } else if raw
            .is_some_and(|value| value.contains_key("oneOf") || value.contains_key("anyOf"))
        {
            labels.push("union".to_owned());
        } else if raw.is_some_and(|value| value.contains_key("allOf")) {
            labels.push("intersection".to_owned());
        } else {
            labels.push("any".to_owned());
        }
    }

    if labels.len() == 1 {
        if let Some(format) = schema.format.as_deref()
            && labels[0] != "any"
        {
            return format!("{}({format})", labels[0]);
        }
        return labels.remove(0);
    }

    labels.sort();
    labels.dedup();
    labels.join("|")
}

fn schema_type_label(schema_type: SchemaType) -> String {
    match schema_type {
        SchemaType::Boolean => "boolean".to_owned(),
        SchemaType::Integer => "integer".to_owned(),
        SchemaType::Number => "number".to_owned(),
        SchemaType::String => "string".to_owned(),
        SchemaType::Array => "array".to_owned(),
        SchemaType::Object => "object".to_owned(),
        SchemaType::Null => "null".to_owned(),
    }
}

fn extract_enum_values(raw: Option<&serde_json::Map<String, Value>>) -> Vec<String> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    let Some(values) = raw.get("enum").and_then(Value::as_array) else {
        return Vec::new();
    };

    values.iter().map(compact_value).collect()
}

fn extract_description(raw: Option<&serde_json::Map<String, Value>>) -> Option<String> {
    raw.and_then(|value| value.get("description").and_then(Value::as_str))
        .map(str::to_owned)
}

fn extract_example(raw: Option<&serde_json::Map<String, Value>>) -> Option<String> {
    let Some(raw) = raw else {
        return None;
    };

    if let Some(example) = raw.get("example") {
        return Some(format_example_value(example));
    }

    raw.get("examples")
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .map(format_example_value)
}

fn format_example_value(value: &Value) -> String {
    match value {
        Value::String(inner) => inner.clone(),
        Value::Object(_) | Value::Array(_) => {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| compact_value(value))
        }
        _ => serde_json::to_string(value).unwrap_or_else(|_| compact_value(value)),
    }
}

fn compact_value(value: &Value) -> String {
    match value {
        Value::String(inner) => inner.clone(),
        _ => {
            let text = serde_json::to_string(value).unwrap_or_else(|_| String::new());
            if text.chars().count() > 100 {
                let mut output = String::new();
                for (index, ch) in text.chars().enumerate() {
                    if index >= 97 {
                        output.push_str("...");
                        break;
                    }
                    output.push(ch);
                }
                output
            } else {
                text
            }
        }
    }
}

fn short_ref_label(ref_path: &str) -> String {
    ref_path.rsplit('/').next().unwrap_or(ref_path).to_owned()
}

fn escape_id_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use oas3::Spec;

    fn parse_spec(yaml: &str) -> Spec {
        serde_yaml::from_str::<Spec>(yaml).unwrap()
    }

    #[test]
    fn builds_schema_tree_with_required_and_enum() {
        let spec = parse_spec(
            r##"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths: {}
components:
  schemas:
    User:
      type: object
      required: [id]
      properties:
        id:
          type: integer
        role:
          type: string
          enum: [admin, member]
"##,
        );

        let schema_ref = ObjectOrReference::Ref {
            ref_path: "#/components/schemas/User".to_owned(),
            summary: None,
            description: None,
        };
        let root = build_schema_tree(&schema_ref, &spec, "schema");

        assert_eq!(root.ref_name.as_deref(), Some("User"));
        assert_eq!(root.children.len(), 2);

        let id = root
            .children
            .iter()
            .find(|child| child.label == "id")
            .expect("id child missing");
        assert!(id.required);

        let role = root
            .children
            .iter()
            .find(|child| child.label == "role")
            .expect("role child missing");
        assert_eq!(
            role.enum_values,
            vec!["admin".to_owned(), "member".to_owned()]
        );
    }

    #[test]
    fn renders_unresolved_references_as_placeholder_nodes() {
        let spec = parse_spec(
            r##"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths: {}
"##,
        );

        let schema_ref = ObjectOrReference::Ref {
            ref_path: "#/components/schemas/Missing".to_owned(),
            summary: None,
            description: None,
        };

        let root = build_schema_tree(&schema_ref, &spec, "schema");
        assert!(root.type_label.contains("[unresolved"));
    }

    #[test]
    fn handles_cycles_without_recursing_forever() {
        let spec = parse_spec(
            r##"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths: {}
components:
  schemas:
    Node:
      type: object
      properties:
        next:
          $ref: "#/components/schemas/Node"
"##,
        );

        let schema_ref = ObjectOrReference::Ref {
            ref_path: "#/components/schemas/Node".to_owned(),
            summary: None,
            description: None,
        };

        let root = build_schema_tree(&schema_ref, &spec, "schema");
        let next = root
            .children
            .iter()
            .find(|child| child.label == "next")
            .expect("next child missing");
        assert!(next.type_label.contains("[cycle:"));
    }

    #[test]
    fn captures_composed_schema_variants() {
        let spec = parse_spec(
            r##"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths: {}
components:
  schemas:
    SearchResult:
      oneOf:
        - type: string
        - type: integer
"##,
        );

        let schema_ref = ObjectOrReference::Ref {
            ref_path: "#/components/schemas/SearchResult".to_owned(),
            summary: None,
            description: None,
        };

        let root = build_schema_tree(&schema_ref, &spec, "schema");
        assert!(
            root.children
                .iter()
                .any(|child| child.label.starts_with("oneOf["))
        );
    }

    #[test]
    fn supports_deeply_nested_object_schemas() {
        let spec = parse_spec(
            r##"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths: {}
components:
  schemas:
    Deep:
      type: object
      properties:
        level1:
          type: object
          properties:
            level2:
              type: object
              properties:
                level3:
                  type: object
                  properties:
                    leaf:
                      type: string
"##,
        );

        let schema_ref = ObjectOrReference::Ref {
            ref_path: "#/components/schemas/Deep".to_owned(),
            summary: None,
            description: None,
        };

        let root = build_schema_tree(&schema_ref, &spec, "schema");
        let level1 = root
            .children
            .iter()
            .find(|child| child.label == "level1")
            .expect("level1 child missing");
        let level2 = level1
            .children
            .iter()
            .find(|child| child.label == "level2")
            .expect("level2 child missing");
        let level3 = level2
            .children
            .iter()
            .find(|child| child.label == "level3")
            .expect("level3 child missing");
        let leaf = level3
            .children
            .iter()
            .find(|child| child.label == "leaf")
            .expect("leaf child missing");

        assert_eq!(leaf.type_label, "string");
    }

    #[test]
    fn keeps_structured_examples_as_pretty_json() {
        let spec = parse_spec(
            r##"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths: {}
components:
  schemas:
    Sample:
      type: object
      properties:
        payload:
          type: object
          example:
            id: 1
            tags: [a, b]
"##,
        );

        let schema_ref = ObjectOrReference::Ref {
            ref_path: "#/components/schemas/Sample".to_owned(),
            summary: None,
            description: None,
        };

        let root = build_schema_tree(&schema_ref, &spec, "schema");
        let payload = root
            .children
            .iter()
            .find(|child| child.label == "payload")
            .expect("payload child missing");
        let example = payload
            .example
            .as_deref()
            .expect("example should be extracted");

        assert!(example.contains('\n'));
        assert!(example.contains("\"id\": 1"));
    }
}
