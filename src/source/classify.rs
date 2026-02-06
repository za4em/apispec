use std::fmt;
use std::path::{Path, PathBuf};
use url::Url;

use crate::error::AppError;

const SPEC_HINTS: &[&str] = &[".json", ".yaml", ".yml", "openapi", "swagger", "api-docs"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    LocalFile,
    DirectUrl,
    BaseUrl,
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            SourceKind::LocalFile => "LocalFile",
            SourceKind::DirectUrl => "DirectUrl",
            SourceKind::BaseUrl => "BaseUrl",
        };
        write!(f, "{label}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceInput {
    pub raw: String,
    pub kind: SourceKind,
    pub normalized_key: String,
}

pub fn classify_source(raw: &str) -> Result<SourceInput, AppError> {
    let trimmed = raw.trim();
    match Url::parse(trimmed) {
        Ok(url) => classify_url(trimmed, url),
        Err(source) => {
            if looks_like_url(trimmed) {
                return Err(AppError::InvalidUrl {
                    input: trimmed.to_owned(),
                    source,
                });
            }
            classify_local_file(trimmed)
        }
    }
}

fn classify_url(raw: &str, mut url: Url) -> Result<SourceInput, AppError> {
    let scheme = url.scheme().to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(AppError::UnsupportedUrlScheme {
            input: raw.to_owned(),
            scheme,
        });
    }

    url.set_fragment(None);
    let kind = if looks_like_spec_url(&url) {
        SourceKind::DirectUrl
    } else {
        SourceKind::BaseUrl
    };

    let normalized_key = match kind {
        SourceKind::BaseUrl => normalize_base_url(url).to_string(),
        SourceKind::DirectUrl => url.to_string(),
        SourceKind::LocalFile => unreachable!("local files are handled separately"),
    };

    Ok(SourceInput {
        raw: raw.to_owned(),
        kind,
        normalized_key,
    })
}

fn classify_local_file(raw: &str) -> Result<SourceInput, AppError> {
    let path = PathBuf::from(raw);
    if !path.exists() {
        return Err(AppError::LocalPathNotFound { path });
    }
    if !path.is_file() {
        return Err(AppError::LocalPathNotFile { path });
    }

    let normalized_key = normalize_local_path(&path)?;
    Ok(SourceInput {
        raw: raw.to_owned(),
        kind: SourceKind::LocalFile,
        normalized_key,
    })
}

fn normalize_local_path(path: &Path) -> Result<String, AppError> {
    let canonical = path
        .canonicalize()
        .map_err(|source| AppError::CanonicalizePath {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(canonical.to_string_lossy().into_owned())
}

fn normalize_base_url(mut url: Url) -> Url {
    let trimmed_path = url.path().trim_end_matches('/').to_owned();
    if trimmed_path.is_empty() {
        url.set_path("/");
    } else {
        url.set_path(&trimmed_path);
    }
    url.set_query(None);
    url
}

fn looks_like_spec_url(url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    let query = url.query().unwrap_or_default().to_ascii_lowercase();
    SPEC_HINTS
        .iter()
        .any(|hint| path.contains(hint) || query.contains(hint))
}

fn looks_like_url(raw: &str) -> bool {
    raw.contains("://") || raw.starts_with("http:") || raw.starts_with("https:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn classifies_direct_url() {
        let source = classify_source("https://example.com/openapi.json").unwrap();
        assert_eq!(source.kind, SourceKind::DirectUrl);
    }

    #[test]
    fn classifies_base_url() {
        let source = classify_source("https://example.com").unwrap();
        assert_eq!(source.kind, SourceKind::BaseUrl);
    }

    #[test]
    fn classifies_local_file() {
        let path = unique_temp_file_path();
        fs::write(&path, b"openapi: 3.1.0\n").unwrap();

        let source = classify_source(path.to_string_lossy().as_ref()).unwrap();
        assert_eq!(source.kind, SourceKind::LocalFile);

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_missing_local_file() {
        let missing = "/tmp/does-not-exist-openapi.yaml";
        let err = classify_source(missing).unwrap_err();
        assert!(matches!(err, AppError::LocalPathNotFound { .. }));
    }

    fn unique_temp_file_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("apispec-{}-{nanos}.yaml", process::id()))
    }
}
