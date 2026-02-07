use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use regex::Regex;
use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, LINK, USER_AGENT};
use url::Url;

use crate::error::AppError;
use crate::spec::validate::parse_and_validate;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const DISCOVERY_BUDGET: Duration = Duration::from_secs(20);
const MAX_HINT_BODY_BYTES: usize = 256 * 1024;
const SNIFF_BYTES: usize = 8 * 1024;

const KNOWN_DISCOVERY_PATHS: [&str; 8] = [
    "openapi.json",
    "openapi.yaml",
    "openapi.yml",
    "v3/api-docs",
    "v3/api-docs.yaml",
    "swagger/v1/swagger.json",
    "swagger.json",
    "swagger.yaml",
];

static HTML_SERVICE_DESC_LINK_REL_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)<link\b[^>]*\brel\s*=\s*["'][^"']*\bservice-desc\b[^"']*["'][^>]*\bhref\s*=\s*["']([^"']+)["'][^>]*>"#,
    )
    .expect("valid regex")
});

static HTML_SERVICE_DESC_LINK_HREF_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)<link\b[^>]*\bhref\s*=\s*["']([^"']+)["'][^>]*\brel\s*=\s*["'][^"']*\bservice-desc\b[^"']*["'][^>]*>"#,
    )
    .expect("valid regex")
});

static SCRIPT_URL_HINT_JS_STYLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)\burl\s*[:=]\s*["']([^"']+)["']"#).expect("valid regex"));

static SCRIPT_URL_HINT_JSON_STYLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)["']url["']\s*:\s*["']([^"']+)["']"#).expect("valid regex")
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryResult {
    pub spec_url: String,
    pub attempted_urls: Vec<String>,
    pub discovery_trace: Vec<String>,
}

#[derive(Debug, Clone)]
struct HttpSuccess {
    resolved_url: String,
    headers: HeaderMap,
    bytes: Vec<u8>,
}

pub fn discover_spec_url(base_url: &str) -> Result<DiscoveryResult, AppError> {
    let base = Url::parse(base_url).map_err(|source| AppError::InvalidUrl {
        input: base_url.to_owned(),
        source,
    })?;
    let started_at = Instant::now();
    let client = build_client(base_url)?;
    let mut discovery_trace = vec![format!("Starting discovery from {base_url}")];
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    match fetch_success_response(&client, base.as_str()) {
        Ok(base_response) => {
            if looks_like_openapi_document(&base_response.headers, &base_response.bytes) {
                match parse_and_validate(&base_response.bytes, &base_response.resolved_url) {
                    Ok(_) => {
                        return Ok(DiscoveryResult {
                            spec_url: base_response.resolved_url.clone(),
                            attempted_urls: vec![base_response.resolved_url],
                            discovery_trace,
                        });
                    }
                    Err(AppError::SpecParse { .. })
                    | Err(AppError::UnsupportedOpenApiVersion { .. }) => {}
                    Err(other) => return Err(other),
                }
            } else {
                discovery_trace.push(
                    "Base URL response is not OpenAPI-like; continuing discovery probes".to_owned(),
                );
            }

            let hint_body_len = base_response.bytes.len().min(MAX_HINT_BODY_BYTES);
            let body = String::from_utf8_lossy(&base_response.bytes[..hint_body_len]);
            let hint_base =
                Url::parse(&base_response.resolved_url).unwrap_or_else(|_| base.clone());
            for hint in extract_service_desc_link_hints(&base_response.headers) {
                if let Some(candidate) = resolve_candidate_url(&hint_base, &hint) {
                    push_unique_candidate(&mut candidates, &mut seen, candidate);
                }
            }
            for hint in extract_html_service_desc_hints(&body) {
                if let Some(candidate) = resolve_candidate_url(&hint_base, &hint) {
                    push_unique_candidate(&mut candidates, &mut seen, candidate);
                }
            }
            for hint in extract_script_url_hints(&body) {
                if let Some(candidate) = resolve_candidate_url(&hint_base, &hint) {
                    push_unique_candidate(&mut candidates, &mut seen, candidate);
                }
            }
        }
        Err(AppError::HttpStatus { status, .. }) => {
            discovery_trace.push(format!(
                "Base URL probe returned HTTP {status}; continuing with known candidates",
            ));
        }
        Err(err @ AppError::NetworkUnavailable { .. }) => return Err(err),
        Err(err @ AppError::FetchFailed { .. }) => return Err(err),
        Err(other) => return Err(other),
    }

    push_known_candidates(&base, &mut candidates, &mut seen);

    let mut attempted_urls = Vec::new();
    let mut timed_out = false;
    for candidate in candidates {
        if started_at.elapsed() >= DISCOVERY_BUDGET {
            timed_out = true;
            discovery_trace.push(format!(
                "Stopped discovery after {}s time budget",
                DISCOVERY_BUDGET.as_secs()
            ));
            break;
        }

        attempted_urls.push(candidate.clone());
        match fetch_success_response(&client, &candidate) {
            Ok(success) => {
                if !looks_like_openapi_document(&success.headers, &success.bytes) {
                    discovery_trace
                        .push(format!("Rejected {candidate}: payload is not OpenAPI-like"));
                    continue;
                }

                match parse_and_validate(&success.bytes, &success.resolved_url) {
                    Ok(_) => {
                        discovery_trace.push(format!(
                            "Resolved OpenAPI document at {}",
                            success.resolved_url
                        ));
                        return Ok(DiscoveryResult {
                            spec_url: success.resolved_url,
                            attempted_urls,
                            discovery_trace,
                        });
                    }
                    Err(AppError::SpecParse { .. }) => {
                        discovery_trace
                            .push(format!("Rejected {candidate}: not an OpenAPI document"));
                    }
                    Err(AppError::UnsupportedOpenApiVersion { found }) => {
                        discovery_trace.push(format!(
                            "Rejected {candidate}: unsupported OpenAPI version {found}"
                        ));
                    }
                    Err(other) => return Err(other),
                }
            }
            Err(AppError::HttpStatus { status, .. }) => {
                discovery_trace.push(format!("Probe {candidate} returned HTTP {status}"));
            }
            Err(err @ AppError::NetworkUnavailable { .. }) => return Err(err),
            Err(AppError::FetchFailed { .. }) => {
                discovery_trace.push(format!(
                    "Probe {candidate} failed before receiving a response"
                ));
            }
            Err(other) => return Err(other),
        }
    }

    let mut attempted = if attempted_urls.is_empty() {
        "(no candidates generated)".to_owned()
    } else {
        attempted_urls.join(", ")
    };
    if timed_out {
        attempted.push_str(&format!(
            ", [stopped after {}s discovery budget]",
            DISCOVERY_BUDGET.as_secs()
        ));
    }

    Err(AppError::DiscoveryFailed {
        base_url: base_url.to_owned(),
        attempted,
    })
}

fn build_client(url: &str) -> Result<Client, AppError> {
    Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|source| AppError::FetchFailed {
            url: url.to_owned(),
            source,
        })
}

fn fetch_success_response(client: &Client, url: &str) -> Result<HttpSuccess, AppError> {
    let response = client
        .get(url)
        .header(ACCEPT, "application/json, application/yaml, text/yaml, */*")
        .header(USER_AGENT, format!("apispec/{}", env!("CARGO_PKG_VERSION")))
        .send()
        .map_err(|source| {
            if is_likely_network_unavailable(&source) {
                AppError::NetworkUnavailable {
                    url: url.to_owned(),
                    source,
                }
            } else {
                AppError::FetchFailed {
                    url: url.to_owned(),
                    source,
                }
            }
        })?;

    if response.status() == StatusCode::NOT_MODIFIED {
        return Err(AppError::HttpStatus {
            url: url.to_owned(),
            status: StatusCode::NOT_MODIFIED.as_u16(),
            details: String::new(),
        });
    }

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body_preview = response.text().unwrap_or_default();
        let details = if body_preview.trim().is_empty() {
            String::new()
        } else {
            format!(
                "; body preview: {}",
                body_preview.chars().take(200).collect::<String>()
            )
        };
        return Err(AppError::HttpStatus {
            url: url.to_owned(),
            status,
            details,
        });
    }

    let resolved_url = response.url().to_string();
    let headers = response.headers().clone();
    let bytes = response
        .bytes()
        .map_err(|source| AppError::FetchFailed {
            url: url.to_owned(),
            source,
        })?
        .to_vec();

    Ok(HttpSuccess {
        resolved_url,
        headers,
        bytes,
    })
}

fn push_unique_candidate(
    candidates: &mut Vec<String>,
    seen: &mut HashSet<String>,
    candidate: String,
) {
    if seen.insert(candidate.clone()) {
        candidates.push(candidate);
    }
}

fn push_known_candidates(base: &Url, candidates: &mut Vec<String>, seen: &mut HashSet<String>) {
    let base_dir = ensure_directory_base(base);
    for path in KNOWN_DISCOVERY_PATHS {
        if let Ok(prefixed_url) = base_dir.join(path) {
            push_unique_candidate(candidates, seen, prefixed_url.to_string());
        }

        if let Ok(root_url) = base.join(&format!("/{path}")) {
            push_unique_candidate(candidates, seen, root_url.to_string());
        }
    }
}

fn looks_like_openapi_document(headers: &HeaderMap, bytes: &[u8]) -> bool {
    if let Some(content_type) = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
    {
        let content_type = content_type.to_ascii_lowercase();
        if content_type.contains("text/html") {
            return false;
        }
        if content_type.contains("application/json")
            || content_type.contains("application/yaml")
            || content_type.contains("text/yaml")
            || content_type.contains("application/x-yaml")
            || content_type.contains("application/vnd.oai.openapi")
        {
            return true;
        }
    }

    let sniff_len = bytes.len().min(SNIFF_BYTES);
    let sniff_bytes = &bytes[..sniff_len];
    let trimmed = trim_ascii_start(sniff_bytes);
    if trimmed.is_empty() {
        return false;
    }
    if trimmed[0] == b'{' || trimmed[0] == b'[' {
        return true;
    }

    let sniff_text = String::from_utf8_lossy(trimmed).to_ascii_lowercase();
    sniff_text.contains("openapi:")
        || sniff_text.contains("\"openapi\"")
        || sniff_text.contains("'openapi'")
}

fn trim_ascii_start(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|value| !value.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    &bytes[start..]
}

fn extract_service_desc_link_hints(headers: &HeaderMap) -> Vec<String> {
    let mut hints = Vec::new();

    for value in headers.get_all(LINK) {
        let Ok(value) = value.to_str() else {
            continue;
        };

        for segment in split_link_header_value(value) {
            if let Some(link) = parse_service_desc_link_header_segment(segment) {
                hints.push(link);
            }
        }
    }

    hints
}

fn split_link_header_value(value: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0usize;
    let mut in_quotes = false;
    let mut in_angle_brackets = false;
    let mut escaped = false;

    for (index, ch) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' if in_quotes => {
                escaped = true;
            }
            '"' => {
                in_quotes = !in_quotes;
            }
            '<' if !in_quotes => {
                in_angle_brackets = true;
            }
            '>' if !in_quotes => {
                in_angle_brackets = false;
            }
            ',' if !in_quotes && !in_angle_brackets => {
                let segment = value[start..index].trim();
                if !segment.is_empty() {
                    segments.push(segment);
                }
                start = index + 1;
            }
            _ => {}
        }
    }

    let trailing = value[start..].trim();
    if !trailing.is_empty() {
        segments.push(trailing);
    }

    segments
}

fn parse_service_desc_link_header_segment(segment: &str) -> Option<String> {
    let segment = segment.trim();
    let start = segment.find('<')?;
    let end = segment[start + 1..].find('>')? + start + 1;
    let target = segment[start + 1..end].trim();
    if target.is_empty() {
        return None;
    }

    let mut is_service_desc = false;
    for part in segment[end + 1..].split(';') {
        let part = part.trim();
        let Some((name, value)) = part.split_once('=') else {
            continue;
        };
        if !name.trim().eq_ignore_ascii_case("rel") {
            continue;
        }

        let value = value.trim().trim_matches('"').trim_matches('\'');
        if value
            .split_ascii_whitespace()
            .any(|token| token.eq_ignore_ascii_case("service-desc"))
        {
            is_service_desc = true;
            break;
        }
    }

    if is_service_desc {
        Some(target.to_owned())
    } else {
        None
    }
}

fn extract_html_service_desc_hints(body: &str) -> Vec<String> {
    let mut hints = Vec::new();
    for captures in HTML_SERVICE_DESC_LINK_REL_FIRST.captures_iter(body) {
        if let Some(href) = captures.get(1) {
            hints.push(href.as_str().trim().to_owned());
        }
    }
    for captures in HTML_SERVICE_DESC_LINK_HREF_FIRST.captures_iter(body) {
        if let Some(href) = captures.get(1) {
            hints.push(href.as_str().trim().to_owned());
        }
    }
    hints
}

fn extract_script_url_hints(body: &str) -> Vec<String> {
    let mut hints = Vec::new();
    for captures in SCRIPT_URL_HINT_JS_STYLE.captures_iter(body) {
        if let Some(url) = captures.get(1) {
            let candidate = url.as_str().trim();
            if looks_like_url_hint(candidate) {
                hints.push(candidate.to_owned());
            }
        }
    }
    for captures in SCRIPT_URL_HINT_JSON_STYLE.captures_iter(body) {
        if let Some(url) = captures.get(1) {
            let candidate = url.as_str().trim();
            if looks_like_url_hint(candidate) {
                hints.push(candidate.to_owned());
            }
        }
    }
    hints
}

fn looks_like_url_hint(value: &str) -> bool {
    value.starts_with("http://")
        || value.starts_with("https://")
        || value.starts_with('/')
        || value.starts_with("./")
        || value.starts_with("../")
}

fn resolve_candidate_url(base: &Url, candidate: &str) -> Option<String> {
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return None;
    }

    if let Ok(parsed) = Url::parse(candidate) {
        return Some(parsed.to_string());
    }

    if candidate.starts_with('/') {
        return base.join(candidate).ok().map(|url| url.to_string());
    }

    ensure_directory_base(base)
        .join(candidate)
        .ok()
        .map(|url| url.to_string())
}

fn ensure_directory_base(base: &Url) -> Url {
    let mut directory_base = base.clone();
    directory_base.set_query(None);
    directory_base.set_fragment(None);

    let path = directory_base.path().to_owned();
    if path.is_empty() {
        directory_base.set_path("/");
    } else if !path.ends_with('/') {
        directory_base.set_path(&format!("{path}/"));
    }

    directory_base
}

fn is_likely_network_unavailable(error: &reqwest::Error) -> bool {
    if error.is_connect() || error.is_timeout() {
        return true;
    }

    let message = error.to_string().to_ascii_lowercase();
    message.contains("dns error")
        || message.contains("failed to lookup address information")
        || message.contains("network is unreachable")
        || message.contains("temporary failure in name resolution")
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::Method::GET;
    use httpmock::MockServer;
    use reqwest::header::HeaderValue;

    fn demo_spec_json() -> &'static str {
        r#"{"openapi":"3.1.0","info":{"title":"demo","version":"1.0.0"},"paths":{}}"#
    }

    #[test]
    fn discovers_from_service_desc_link_header() {
        let server = MockServer::start();
        let root = server.mock(|when, then| {
            when.method(GET).path("/");
            then.status(200)
                .header("Link", "</docs/openapi.json>; rel=\"service-desc\"")
                .body("ok");
        });
        let spec = server.mock(|when, then| {
            when.method(GET).path("/docs/openapi.json");
            then.status(200).body(demo_spec_json());
        });

        let discovered = discover_spec_url(&server.base_url()).unwrap();

        root.assert();
        spec.assert();
        assert_eq!(
            discovered.spec_url,
            format!("{}/docs/openapi.json", server.base_url())
        );
        assert!(
            discovered
                .attempted_urls
                .iter()
                .any(|value| value.ends_with("/docs/openapi.json"))
        );
    }

    #[test]
    fn discovers_from_html_link_rel_service_desc() {
        let server = MockServer::start();
        let root = server.mock(|when, then| {
            when.method(GET).path("/");
            then.status(200).body(
                r#"<html><head><link rel="service-desc" href="/spec/openapi.yaml"></head></html>"#,
            );
        });
        let spec = server.mock(|when, then| {
            when.method(GET).path("/spec/openapi.yaml");
            then.status(200).body(
                r#"openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths: {}
"#,
            );
        });

        let discovered = discover_spec_url(&server.base_url()).unwrap();

        root.assert();
        spec.assert();
        assert!(discovered.spec_url.ends_with("/spec/openapi.yaml"));
    }

    #[test]
    fn discovers_from_default_candidates() {
        let server = MockServer::start();
        let root = server.mock(|when, then| {
            when.method(GET).path("/");
            then.status(200).body("<html><body>no hints</body></html>");
        });
        let openapi_json = server.mock(|when, then| {
            when.method(GET).path("/openapi.json");
            then.status(404);
        });
        let v3_docs = server.mock(|when, then| {
            when.method(GET).path("/v3/api-docs");
            then.status(200).body(demo_spec_json());
        });

        let discovered = discover_spec_url(&server.base_url()).unwrap();

        root.assert();
        openapi_json.assert();
        v3_docs.assert();
        assert!(discovered.spec_url.ends_with("/v3/api-docs"));
    }

    #[test]
    fn probes_prefixed_candidates_for_base_url_with_path_prefix() {
        let server = MockServer::start();
        let root = server.mock(|when, then| {
            when.method(GET).path("/api");
            then.status(200).body("<html><body>no hints</body></html>");
        });
        let prefixed_openapi = server.mock(|when, then| {
            when.method(GET).path("/api/openapi.json");
            then.status(200).body(demo_spec_json());
        });

        let discovered = discover_spec_url(&format!("{}/api", server.base_url())).unwrap();

        root.assert();
        prefixed_openapi.assert();
        assert_eq!(
            discovered.spec_url,
            format!("{}/api/openapi.json", server.base_url())
        );
    }

    #[test]
    fn returns_deterministic_error_when_no_candidates_match() {
        let server = MockServer::start();
        let _root = server.mock(|when, then| {
            when.method(GET).path("/");
            then.status(404);
        });

        let error = discover_spec_url(&server.base_url()).unwrap_err();
        let AppError::DiscoveryFailed {
            base_url,
            attempted,
        } = error
        else {
            panic!("expected DiscoveryFailed");
        };

        assert_eq!(base_url, server.base_url());
        assert!(attempted.contains("/openapi.json"));
        assert!(attempted.contains("/v3/api-docs"));
    }

    #[test]
    fn discovers_relative_hints_for_base_url_with_path_prefix() {
        let server = MockServer::start();
        let root = server.mock(|when, then| {
            when.method(GET).path("/api");
            then.status(200)
                .body(r#"<html><head><link rel="service-desc" href="openapi.json"></head></html>"#);
        });
        let spec = server.mock(|when, then| {
            when.method(GET).path("/api/openapi.json");
            then.status(200).body(demo_spec_json());
        });

        let discovered = discover_spec_url(&format!("{}/api", server.base_url())).unwrap();

        root.assert();
        spec.assert();
        assert_eq!(
            discovered.spec_url,
            format!("{}/api/openapi.json", server.base_url())
        );
    }

    #[test]
    fn parses_link_header_with_quoted_commas() {
        let mut headers = HeaderMap::new();
        headers.insert(
            LINK,
            HeaderValue::from_static(
                "</docs/openapi.json>; rel=\"service-desc\"; title=\"OpenAPI, v3\", </alt>; rel=\"alternate\"",
            ),
        );

        let hints = extract_service_desc_link_hints(&headers);

        assert_eq!(hints, vec!["/docs/openapi.json"]);
    }

    #[test]
    fn skips_html_catch_all_candidates() {
        let server = MockServer::start();
        let root = server.mock(|when, then| {
            when.method(GET).path("/");
            then.status(200)
                .header("Content-Type", "text/html; charset=utf-8")
                .body("<html><body>docs shell</body></html>");
        });
        let openapi_json = server.mock(|when, then| {
            when.method(GET).path("/openapi.json");
            then.status(200)
                .header("Content-Type", "text/html; charset=utf-8")
                .body("<html><body>docs shell</body></html>");
        });
        let v3_docs = server.mock(|when, then| {
            when.method(GET).path("/v3/api-docs");
            then.status(200).body(demo_spec_json());
        });

        let discovered = discover_spec_url(&server.base_url()).unwrap();

        root.assert();
        openapi_json.assert();
        v3_docs.assert();
        assert!(discovered.spec_url.ends_with("/v3/api-docs"));
        assert!(
            discovered
                .discovery_trace
                .iter()
                .any(|line| line.contains("payload is not OpenAPI-like"))
        );
    }

    #[test]
    fn openapi_payload_sniffer_rejects_html() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/html"));
        assert!(!looks_like_openapi_document(
            &headers,
            b"<html><body>docs</body></html>"
        ));
    }

    #[test]
    fn openapi_payload_sniffer_accepts_yaml_without_content_type() {
        let headers = HeaderMap::new();
        assert!(looks_like_openapi_document(
            &headers,
            b"openapi: 3.1.0\ninfo:\n  title: demo\n  version: 1.0.0\npaths: {}\n"
        ));
    }
}
