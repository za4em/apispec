use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheMetadata {
    pub canonical_source: String,
    pub resolved_spec_url: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub fetched_at_utc: String,
    pub last_success_at_utc: String,
    pub openapi_version: String,
    pub content_sha256: String,
}

impl CacheMetadata {
    pub fn new(
        canonical_source: &str,
        resolved_spec_url: Option<String>,
        etag: Option<String>,
        last_modified: Option<String>,
        openapi_version: &str,
        bytes: &[u8],
    ) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            canonical_source: canonical_source.to_owned(),
            resolved_spec_url,
            etag,
            last_modified,
            fetched_at_utc: now.clone(),
            last_success_at_utc: now,
            openapi_version: openapi_version.to_owned(),
            content_sha256: digest_sha256(bytes),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheState {
    Fresh,
    Revalidated304,
    OfflineStale,
}

impl fmt::Display for CacheState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            CacheState::Fresh => "fresh",
            CacheState::Revalidated304 => "cached (not modified)",
            CacheState::OfflineStale => "offline, using cached copy",
        };
        write!(f, "{label}")
    }
}

pub fn digest_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
