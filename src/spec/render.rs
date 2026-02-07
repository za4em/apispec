use std::collections::HashSet;

use oas3::spec::{
    MediaType, ObjectOrReference, ObjectSchema, Parameter, Schema, SchemaType, SchemaTypeSet, Spec,
};

const MAX_SCHEMA_DEPTH: usize = 2;
const MAX_SCHEMA_NODES: usize = 40;
const MAX_OBJECT_PROPERTIES: usize = 6;

#[derive(Debug, Clone)]
struct RenderContext {
    remaining_nodes: usize,
    truncated_nodes: usize,
    visited_refs: HashSet<String>,
}

impl RenderContext {
    fn new(max_nodes: usize) -> Self {
        Self {
            remaining_nodes: max_nodes,
            truncated_nodes: 0,
            visited_refs: HashSet::new(),
        }
    }

    fn consume_node(&mut self) -> bool {
        if self.remaining_nodes == 0 {
            self.truncated_nodes += 1;
            false
        } else {
            self.remaining_nodes -= 1;
            true
        }
    }
}

pub fn summarize_schema(schema: &ObjectOrReference<ObjectSchema>, spec: &Spec) -> String {
    let mut context = RenderContext::new(MAX_SCHEMA_NODES);
    let summary = summarize_schema_ref(schema, spec, 0, &mut context);
    finalize_summary(summary, &context)
}

pub fn summarize_media_type_schema(media_type: &MediaType, spec: &Spec) -> Option<String> {
    media_type
        .schema
        .as_ref()
        .map(|schema| summarize_schema(schema, spec))
}

pub fn summarize_parameter_schema(parameter: &Parameter, spec: &Spec) -> Option<String> {
    if let Some(schema) = parameter.schema.as_ref() {
        return Some(summarize_schema(schema, spec));
    }

    let content = parameter.content.as_ref()?;
    if content.is_empty() {
        return None;
    }

    let mut summaries = Vec::new();
    for (content_type, media_type) in content {
        let summary =
            summarize_media_type_schema(media_type, spec).unwrap_or_else(|| "any".to_owned());
        summaries.push(format!("{content_type}: {summary}"));
    }
    Some(summaries.join(", "))
}

fn summarize_schema_ref(
    schema: &ObjectOrReference<ObjectSchema>,
    spec: &Spec,
    depth: usize,
    context: &mut RenderContext,
) -> String {
    match schema {
        ObjectOrReference::Object(object) => summarize_object_schema(object, spec, depth, context),
        ObjectOrReference::Ref { ref_path, .. } => {
            if !context.consume_node() {
                return "...".to_owned();
            }
            if !context.visited_refs.insert(ref_path.clone()) {
                return format!("ref({}) [cycle]", short_ref_label(ref_path));
            }

            let summary = match schema.resolve(spec) {
                Ok(object) => summarize_object_schema(&object, spec, depth, context),
                Err(_) => format!("ref({}) (unresolved)", short_ref_label(ref_path)),
            };
            context.visited_refs.remove(ref_path);
            summary
        }
    }
}

fn summarize_schema_document(
    schema: &Schema,
    spec: &Spec,
    depth: usize,
    context: &mut RenderContext,
) -> String {
    match schema {
        Schema::Boolean(value) => {
            if value.0 {
                "any".to_owned()
            } else {
                "never".to_owned()
            }
        }
        Schema::Object(object) => summarize_schema_ref(object.as_ref(), spec, depth, context),
    }
}

fn summarize_object_schema(
    schema: &ObjectSchema,
    spec: &Spec,
    depth: usize,
    context: &mut RenderContext,
) -> String {
    if !context.consume_node() {
        return "...".to_owned();
    }

    if is_array_schema(schema) {
        let item_summary = schema
            .items
            .as_ref()
            .map(|item| summarize_schema_document(item, spec, depth + 1, context))
            .unwrap_or_else(|| "any".to_owned());
        let mut summary = format!("array<{item_summary}>");
        if is_nullable_schema(schema) {
            summary.push_str("|null");
        }
        return summary;
    }

    if is_object_schema(schema) {
        if depth >= MAX_SCHEMA_DEPTH {
            return "object{...}".to_owned();
        }

        if schema.properties.is_empty() {
            let mut summary = "object".to_owned();
            if is_nullable_schema(schema) {
                summary.push_str("|null");
            }
            return summary;
        }

        let required = schema
            .required
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let mut property_names = Vec::new();
        for name in &schema.required {
            if schema.properties.contains_key(name) {
                property_names.push(name.as_str());
            }
        }
        for name in schema.properties.keys() {
            if !required.contains(name.as_str()) {
                property_names.push(name.as_str());
            }
        }

        let mut fields = Vec::new();
        for name in property_names.iter().take(MAX_OBJECT_PROPERTIES) {
            let schema_ref = schema
                .properties
                .get(*name)
                .expect("property name comes from properties map");
            let field_type = summarize_schema_ref(schema_ref, spec, depth + 1, context);
            if required.contains(*name) {
                fields.push(format!("{name}: {field_type}"));
            } else {
                fields.push(format!("{name}?: {field_type}"));
            }
        }

        if property_names.len() > MAX_OBJECT_PROPERTIES {
            context.truncated_nodes += property_names.len() - MAX_OBJECT_PROPERTIES;
            fields.push("...".to_owned());
        }

        let mut summary = format!("object{{{}}}", fields.join(", "));
        if is_nullable_schema(schema) {
            summary.push_str("|null");
        }
        return summary;
    }

    primitive_schema_label(schema)
}

fn primitive_schema_label(schema: &ObjectSchema) -> String {
    let mut labels = match schema.schema_type.as_ref() {
        Some(SchemaTypeSet::Single(single)) => vec![schema_type_label(*single)],
        Some(SchemaTypeSet::Multiple(multiple)) => {
            multiple.iter().copied().map(schema_type_label).collect()
        }
        None => vec!["any".to_owned()],
    };
    if labels.is_empty() {
        labels.push("any".to_owned());
    }

    if labels.len() == 1 {
        if let Some(format) = schema.format.as_deref()
            && labels[0] != "any"
        {
            return format!("{}({format})", labels[0]);
        }
        return labels.remove(0);
    }

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

fn is_array_schema(schema: &ObjectSchema) -> bool {
    schema.items.is_some()
        || schema
            .schema_type
            .as_ref()
            .is_some_and(SchemaTypeSet::is_array_or_nullable_array)
}

fn is_object_schema(schema: &ObjectSchema) -> bool {
    !schema.properties.is_empty()
        || schema
            .schema_type
            .as_ref()
            .is_some_and(SchemaTypeSet::is_object_or_nullable_object)
}

fn is_nullable_schema(schema: &ObjectSchema) -> bool {
    schema.is_nullable().unwrap_or(false)
}

fn finalize_summary(mut summary: String, context: &RenderContext) -> String {
    if context.truncated_nodes == 0 {
        return summary;
    }

    if summary.ends_with("...") {
        summary.push_str(&format!("(+{})", context.truncated_nodes));
    } else {
        summary.push_str(&format!(" ...(+{})", context.truncated_nodes));
    }
    summary
}

fn short_ref_label(ref_path: &str) -> String {
    ref_path.rsplit('/').next().unwrap_or(ref_path).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use oas3::Spec;

    fn parse_spec(yaml: &str) -> Spec {
        serde_yaml::from_str::<Spec>(yaml).unwrap()
    }

    #[test]
    fn summarizes_object_with_required_properties_first() {
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
      required: [id, email]
      properties:
        email:
          type: string
          format: email
        id:
          type: integer
        name:
          type: string
"##,
        );

        let schema_ref = ObjectOrReference::Ref {
            ref_path: "#/components/schemas/User".to_owned(),
            summary: None,
            description: None,
        };
        let summary = summarize_schema(&schema_ref, &spec);

        let email_index = summary.find("email:").unwrap();
        let name_index = summary.find("name?:").unwrap();
        assert!(email_index < name_index);
        assert!(summary.contains("string(email)"));
    }

    #[test]
    fn summarizes_cycles_without_recursing_forever() {
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
        let summary = summarize_schema(&schema_ref, &spec);

        assert!(summary.contains("[cycle]"));
    }

    #[test]
    fn adds_cutoff_hint_when_depth_limit_is_hit() {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths: {}
components:
  schemas:
    Level1:
      type: object
      properties:
        level2:
          type: object
          properties:
            level3:
              type: object
              properties:
                value:
                  type: string
"#,
        );

        let schema_ref = ObjectOrReference::Ref {
            ref_path: "#/components/schemas/Level1".to_owned(),
            summary: None,
            description: None,
        };
        let summary = summarize_schema(&schema_ref, &spec);

        assert!(summary.contains("..."));
    }

    #[test]
    fn renders_unresolved_references_as_placeholders() {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths: {}
"#,
        );

        let schema_ref = ObjectOrReference::Ref {
            ref_path: "#/components/schemas/Missing".to_owned(),
            summary: None,
            description: None,
        };
        let summary = summarize_schema(&schema_ref, &spec);

        assert!(summary.contains("unresolved"));
    }

    #[test]
    fn snapshot_summary_for_object_with_nested_structures() {
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
      required: [id, name]
      properties:
        id:
          type: string
          format: uuid
        name:
          type: string
        profile:
          type: object
          properties:
            age:
              type: integer
            email:
              type: string
              format: email
        tags:
          type: array
          items:
            type: string
"##,
        );

        let schema_ref = ObjectOrReference::Ref {
            ref_path: "#/components/schemas/User".to_owned(),
            summary: None,
            description: None,
        };
        let summary = summarize_schema(&schema_ref, &spec);
        assert_eq!(
            summary,
            include_str!("snapshots/schema_object_nested.snap").trim_end()
        );
    }

    #[test]
    fn snapshot_summary_for_cyclic_schema() {
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
        let summary = summarize_schema(&schema_ref, &spec);
        assert_eq!(
            summary,
            include_str!("snapshots/schema_cycle.snap").trim_end()
        );
    }

    #[test]
    fn snapshot_summary_for_property_cutoff() {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths: {}
components:
  schemas:
    LargeObject:
      type: object
      required: [id, code]
      properties:
        id:
          type: string
        code:
          type: integer
        alpha:
          type: string
        beta:
          type: string
        delta:
          type: string
        epsilon:
          type: string
        gamma:
          type: string
        zeta:
          type: string
"#,
        );

        let schema_ref = ObjectOrReference::Ref {
            ref_path: "#/components/schemas/LargeObject".to_owned(),
            summary: None,
            description: None,
        };
        let summary = summarize_schema(&schema_ref, &spec);
        assert_eq!(
            summary,
            include_str!("snapshots/schema_property_cutoff.snap").trim_end()
        );
    }

    #[test]
    fn snapshot_summary_for_unresolved_reference() {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths: {}
"#,
        );

        let schema_ref = ObjectOrReference::Ref {
            ref_path: "#/components/schemas/Missing".to_owned(),
            summary: None,
            description: None,
        };
        let summary = summarize_schema(&schema_ref, &spec);
        assert_eq!(
            summary,
            include_str!("snapshots/schema_unresolved_ref.snap").trim_end()
        );
    }
}
