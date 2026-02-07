use std::path::PathBuf;

use oas3::Spec;

use crate::cache::metadata::{CacheMetadata, CacheState};
use crate::cache::store::{CacheStore, CachedSpec};
use crate::error::AppError;
use crate::source::{
    ConditionalFetchHeaders, FetchOutcome, SourceInput, SourceKind, discover_spec_url, fetch_spec,
};
use crate::spec::validate::parse_and_validate;

#[derive(Debug, Clone)]
pub struct LoadedSpec {
    pub spec: Spec,
    pub cache_state: CacheState,
    pub source_label: String,
    pub cached_at_utc: Option<String>,
}

pub fn load_spec_for_source(source: &SourceInput) -> Result<LoadedSpec, AppError> {
    let cache = CacheStore::new()?;
    load_spec_for_source_with_cache(source, &cache)
}

fn load_spec_for_source_with_cache(
    source: &SourceInput,
    cache: &CacheStore,
) -> Result<LoadedSpec, AppError> {
    match source.kind {
        SourceKind::LocalFile => load_local_spec(source, cache),
        SourceKind::DirectUrl => load_remote_spec(source, cache),
        SourceKind::BaseUrl => load_base_url_spec(source, cache),
    }
}

fn load_local_spec(source: &SourceInput, cache: &CacheStore) -> Result<LoadedSpec, AppError> {
    let path = PathBuf::from(&source.normalized_key);
    let bytes = std::fs::read(&path).map_err(|source| AppError::ReadLocalPath {
        path: path.clone(),
        source,
    })?;

    let spec = parse_and_validate(&bytes, &source.normalized_key)?;
    let metadata = CacheMetadata::new(
        &source.normalized_key,
        None,
        None,
        None,
        &spec.openapi,
        &bytes,
    );
    cache.write(&source.normalized_key, &bytes, &metadata)?;

    Ok(LoadedSpec {
        spec,
        cache_state: CacheState::Fresh,
        source_label: source.normalized_key.clone(),
        cached_at_utc: Some(metadata.last_success_at_utc),
    })
}

fn load_remote_spec(source: &SourceInput, cache: &CacheStore) -> Result<LoadedSpec, AppError> {
    load_remote_spec_from_url(source, &source.normalized_key, cache)
}

fn load_base_url_spec(source: &SourceInput, cache: &CacheStore) -> Result<LoadedSpec, AppError> {
    let cached_spec = cache.read(&source.normalized_key)?;
    let discovered = match discover_spec_url(&source.normalized_key) {
        Ok(discovered) => discovered,
        Err(err @ AppError::NetworkUnavailable { .. }) => {
            return load_cached_when_offline(cached_spec, &source.normalized_key, err);
        }
        Err(other) => return Err(other),
    };

    load_remote_spec_from_url(source, &discovered.spec_url, cache)
}

fn load_remote_spec_from_url(
    source: &SourceInput,
    request_url: &str,
    cache: &CacheStore,
) -> Result<LoadedSpec, AppError> {
    let cached_spec = cache.read(&source.normalized_key)?;
    let conditional = conditional_headers(&cached_spec);

    match fetch_spec(request_url, &conditional) {
        Ok(FetchOutcome::Downloaded(success)) => {
            let spec = parse_and_validate(&success.bytes, &success.resolved_url)?;
            let metadata = CacheMetadata::new(
                &source.normalized_key,
                Some(success.resolved_url.clone()),
                success.etag,
                success.last_modified,
                &spec.openapi,
                &success.bytes,
            );
            cache.write(&source.normalized_key, &success.bytes, &metadata)?;
            Ok(LoadedSpec {
                spec,
                cache_state: CacheState::Fresh,
                source_label: success.resolved_url,
                cached_at_utc: Some(metadata.last_success_at_utc),
            })
        }
        Ok(FetchOutcome::NotModified) => {
            let cached = cached_spec.ok_or_else(|| AppError::NotModifiedWithoutCache {
                url: request_url.to_owned(),
            })?;
            let source_label = cached
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.resolved_spec_url.clone())
                .unwrap_or_else(|| request_url.to_owned());
            let cached_at = cached
                .metadata
                .as_ref()
                .map(|metadata| metadata.last_success_at_utc.clone());
            let spec = parse_and_validate(&cached.bytes, &source_label)?;
            Ok(LoadedSpec {
                spec,
                cache_state: CacheState::Revalidated304,
                source_label,
                cached_at_utc: cached_at,
            })
        }
        Err(err @ AppError::NetworkUnavailable { .. }) => {
            load_cached_when_offline(cached_spec, request_url, err)
        }
        Err(other) => Err(other),
    }
}

fn load_cached_when_offline(
    cached_spec: Option<CachedSpec>,
    fallback_source_label: &str,
    err: AppError,
) -> Result<LoadedSpec, AppError> {
    if let Some(cached) = cached_spec {
        let source_label = cached
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.resolved_spec_url.clone())
            .unwrap_or_else(|| fallback_source_label.to_owned());
        let cached_at = cached
            .metadata
            .as_ref()
            .map(|metadata| metadata.last_success_at_utc.clone());
        let spec = parse_and_validate(&cached.bytes, &source_label)?;
        return Ok(LoadedSpec {
            spec,
            cache_state: CacheState::OfflineStale,
            source_label,
            cached_at_utc: cached_at,
        });
    }

    if let AppError::NetworkUnavailable { url, source } = err {
        return Err(AppError::OfflineNoCache { url, source });
    }
    unreachable!("offline fallback is only used for AppError::NetworkUnavailable")
}

fn conditional_headers(cached_spec: &Option<CachedSpec>) -> ConditionalFetchHeaders {
    if let Some(cached_spec) = cached_spec
        && let Some(metadata) = &cached_spec.metadata
    {
        return ConditionalFetchHeaders {
            etag: metadata.etag.clone(),
            last_modified: metadata.last_modified.clone(),
        };
    }

    ConditionalFetchHeaders::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::classify_source;
    use httpmock::Method::GET;
    use httpmock::MockServer;
    use tempfile::TempDir;

    fn demo_spec_json() -> &'static [u8] {
        br#"{"openapi":"3.1.0","info":{"title":"demo","version":"1.0.0"},"paths":{}}"#
    }

    #[test]
    fn loads_local_spec_and_updates_cache() {
        let temp = TempDir::new().unwrap();
        let source_file = temp.path().join("spec.yaml");
        std::fs::write(
            &source_file,
            "openapi: 3.1.0\ninfo:\n  title: demo\n  version: 1.0.0\npaths: {}\n",
        )
        .unwrap();

        let source = classify_source(source_file.to_string_lossy().as_ref()).unwrap();
        let cache = CacheStore::with_root(temp.path().join("cache"));
        let loaded = load_spec_for_source_with_cache(&source, &cache).unwrap();

        assert_eq!(loaded.cache_state, CacheState::Fresh);
        let cached = cache.read(&source.normalized_key).unwrap().unwrap();
        assert!(!cached.bytes.is_empty());
    }

    #[test]
    fn loads_direct_url_and_writes_cache() {
        let temp = TempDir::new().unwrap();
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/openapi.json");
            then.status(200)
                .header("ETag", "\"etag-1\"")
                .body(demo_spec_json());
        });

        let source = classify_source(&format!("{}/openapi.json", server.base_url())).unwrap();
        let cache = CacheStore::with_root(temp.path().join("cache"));
        let loaded = load_spec_for_source_with_cache(&source, &cache).unwrap();

        mock.assert();
        assert_eq!(loaded.cache_state, CacheState::Fresh);

        let cached = cache.read(&source.normalized_key).unwrap().unwrap();
        let metadata = cached.metadata.unwrap();
        assert_eq!(metadata.etag.as_deref(), Some("\"etag-1\""));
        assert_eq!(metadata.openapi_version, "3.1.0");
    }

    #[test]
    fn uses_cached_copy_when_network_is_unavailable() {
        let temp = TempDir::new().unwrap();
        let cache = CacheStore::with_root(temp.path().join("cache"));
        let source = classify_source("https://127.0.0.1:9/openapi.json").unwrap();

        let metadata = CacheMetadata::new(
            &source.normalized_key,
            Some(source.normalized_key.clone()),
            None,
            None,
            "3.1.0",
            demo_spec_json(),
        );
        cache
            .write(&source.normalized_key, demo_spec_json(), &metadata)
            .unwrap();

        let loaded = load_spec_for_source_with_cache(&source, &cache).unwrap();
        assert_eq!(loaded.cache_state, CacheState::OfflineStale);
        assert_eq!(loaded.spec.openapi, "3.1.0");
    }

    #[test]
    fn fails_when_network_is_unavailable_and_cache_is_missing() {
        let temp = TempDir::new().unwrap();
        let cache = CacheStore::with_root(temp.path().join("cache"));
        let source = classify_source("https://127.0.0.1:9/openapi.json").unwrap();

        let error = load_spec_for_source_with_cache(&source, &cache).unwrap_err();
        assert!(matches!(error, AppError::OfflineNoCache { .. }));
    }

    #[test]
    fn revalidates_cached_copy_when_server_returns_304() {
        let temp = TempDir::new().unwrap();
        let cache = CacheStore::with_root(temp.path().join("cache"));
        let server = MockServer::start();

        let not_modified_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/openapi.json")
                .header("If-None-Match", "\"etag-1\"");
            then.status(304);
        });
        let fresh_mock = server.mock(|when, then| {
            when.method(GET).path("/openapi.json");
            then.status(200)
                .header("ETag", "\"etag-1\"")
                .body(demo_spec_json());
        });

        let source = classify_source(&format!("{}/openapi.json", server.base_url())).unwrap();
        let fresh = load_spec_for_source_with_cache(&source, &cache).unwrap();
        let revalidated = load_spec_for_source_with_cache(&source, &cache).unwrap();

        assert_eq!(fresh.cache_state, CacheState::Fresh);
        assert_eq!(revalidated.cache_state, CacheState::Revalidated304);
        fresh_mock.assert_hits(1);
        not_modified_mock.assert_hits(1);
    }

    #[test]
    fn loads_base_url_via_discovery_and_updates_cache() {
        let temp = TempDir::new().unwrap();
        let cache = CacheStore::with_root(temp.path().join("cache"));
        let server = MockServer::start();

        let root = server.mock(|when, then| {
            when.method(GET).path("/");
            then.status(200).body("<html>no hints</html>");
        });
        let discovered = server.mock(|when, then| {
            when.method(GET).path("/v3/api-docs");
            then.status(200)
                .header("ETag", "\"etag-1\"")
                .body(demo_spec_json());
        });

        let source = classify_source(&server.base_url()).unwrap();
        let loaded = load_spec_for_source_with_cache(&source, &cache).unwrap();

        assert!(root.hits() >= 1);
        discovered.assert_hits(2);
        assert_eq!(loaded.cache_state, CacheState::Fresh);
        assert!(loaded.source_label.ends_with("/v3/api-docs"));

        let cached = cache.read(&source.normalized_key).unwrap().unwrap();
        let metadata = cached.metadata.unwrap();
        assert!(
            metadata
                .resolved_spec_url
                .unwrap()
                .ends_with("/v3/api-docs")
        );
    }

    #[test]
    fn uses_cached_copy_for_base_url_when_network_is_unavailable() {
        let temp = TempDir::new().unwrap();
        let cache = CacheStore::with_root(temp.path().join("cache"));
        let source = classify_source("https://127.0.0.1:9").unwrap();

        let metadata = CacheMetadata::new(
            &source.normalized_key,
            Some("https://127.0.0.1:9/openapi.json".to_owned()),
            None,
            None,
            "3.1.0",
            demo_spec_json(),
        );
        cache
            .write(&source.normalized_key, demo_spec_json(), &metadata)
            .unwrap();

        let loaded = load_spec_for_source_with_cache(&source, &cache).unwrap();
        assert_eq!(loaded.cache_state, CacheState::OfflineStale);
        assert_eq!(loaded.spec.openapi, "3.1.0");
        assert_eq!(loaded.source_label, "https://127.0.0.1:9/openapi.json");
    }
}
