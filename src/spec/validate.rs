use oas3::Spec;

use crate::error::AppError;

pub const SUPPORTED_OPENAPI_VERSION: &str = "3.1.0";

pub fn parse_and_validate(bytes: &[u8], source_label: &str) -> Result<Spec, AppError> {
    let spec = parse_spec(bytes, source_label)?;
    validate_openapi_version(&spec)?;
    Ok(spec)
}

fn parse_spec(bytes: &[u8], source_label: &str) -> Result<Spec, AppError> {
    let prefer_json = bytes
        .iter()
        .copied()
        .find(|value| !value.is_ascii_whitespace())
        .is_some_and(|value| value == b'{' || value == b'[');

    let (json_error, yaml_error) = if prefer_json {
        match serde_json::from_slice::<Spec>(bytes) {
            Ok(spec) => return Ok(spec),
            Err(json_error) => match serde_yaml::from_slice::<Spec>(bytes) {
                Ok(spec) => return Ok(spec),
                Err(yaml_error) => (json_error, yaml_error),
            },
        }
    } else {
        match serde_yaml::from_slice::<Spec>(bytes) {
            Ok(spec) => return Ok(spec),
            Err(yaml_error) => match serde_json::from_slice::<Spec>(bytes) {
                Ok(spec) => return Ok(spec),
                Err(json_error) => (json_error, yaml_error),
            },
        }
    };

    Err(AppError::SpecParse {
        source_label: source_label.to_owned(),
        json_error: json_error.to_string(),
        yaml_error: yaml_error.to_string(),
    })
}

pub fn validate_openapi_version(spec: &Spec) -> Result<(), AppError> {
    if spec.openapi == SUPPORTED_OPENAPI_VERSION {
        Ok(())
    } else {
        Err(AppError::UnsupportedOpenApiVersion {
            found: spec.openapi.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_spec() {
        let json = br#"{
  "openapi":"3.1.0",
  "info":{"title":"demo","version":"1.0.0"},
  "paths":{}
}"#;
        let spec = parse_and_validate(json, "json").unwrap();
        assert_eq!(spec.openapi, SUPPORTED_OPENAPI_VERSION);
    }

    #[test]
    fn parses_yaml_spec() {
        let yaml = br#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths: {}
"#;
        let spec = parse_and_validate(yaml, "yaml").unwrap();
        assert_eq!(spec.openapi, SUPPORTED_OPENAPI_VERSION);
    }

    #[test]
    fn rejects_non_310_versions() {
        let yaml = br#"
openapi: 3.1.1
info:
  title: demo
  version: 1.0.0
paths: {}
"#;
        let error = parse_and_validate(yaml, "yaml").unwrap_err();
        assert!(matches!(error, AppError::UnsupportedOpenApiVersion { .. }));
    }
}
