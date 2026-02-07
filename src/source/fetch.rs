use std::time::Duration;

use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::header::{ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED};

use crate::error::AppError;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConditionalFetchHeaders {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchSuccess {
    pub bytes: Vec<u8>,
    pub resolved_url: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchOutcome {
    Downloaded(FetchSuccess),
    NotModified,
}

pub fn fetch_spec(
    url: &str,
    conditional: &ConditionalFetchHeaders,
) -> Result<FetchOutcome, AppError> {
    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|source| AppError::FetchFailed {
            url: url.to_owned(),
            source,
        })?;

    let mut request = client
        .get(url)
        .header(
            reqwest::header::ACCEPT,
            "application/json, application/yaml, text/yaml, */*",
        )
        .header(
            reqwest::header::USER_AGENT,
            format!("apispec/{}", env!("CARGO_PKG_VERSION")),
        );

    if let Some(etag) = &conditional.etag {
        request = request.header(IF_NONE_MATCH, etag);
    }
    if let Some(last_modified) = &conditional.last_modified {
        request = request.header(IF_MODIFIED_SINCE, last_modified);
    }

    let response = request.send().map_err(|source| {
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
        return Ok(FetchOutcome::NotModified);
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

    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let last_modified = response
        .headers()
        .get(LAST_MODIFIED)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let resolved_url = response.url().to_string();
    let bytes = response
        .bytes()
        .map_err(|source| AppError::FetchFailed {
            url: url.to_owned(),
            source,
        })?
        .to_vec();

    Ok(FetchOutcome::Downloaded(FetchSuccess {
        bytes,
        resolved_url,
        etag,
        last_modified,
    }))
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

    #[test]
    fn downloads_body_and_headers() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/openapi.json");
            then.status(200)
                .header("ETag", "\"abc\"")
                .header("Last-Modified", "Tue, 03 Jan 2023 09:00:00 GMT")
                .body(r#"{"openapi":"3.1.0","info":{"title":"t","version":"1"},"paths":{}}"#);
        });

        let url = format!("{}/openapi.json", server.base_url());
        let result = fetch_spec(&url, &ConditionalFetchHeaders::default()).unwrap();

        mock.assert();
        match result {
            FetchOutcome::Downloaded(success) => {
                assert_eq!(success.etag.as_deref(), Some("\"abc\""));
                assert_eq!(
                    success.last_modified.as_deref(),
                    Some("Tue, 03 Jan 2023 09:00:00 GMT")
                );
                assert!(!success.bytes.is_empty());
            }
            FetchOutcome::NotModified => panic!("expected downloaded response"),
        }
    }

    #[test]
    fn returns_not_modified() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/openapi.json")
                .header("If-None-Match", "\"etag\"");
            then.status(304);
        });

        let url = format!("{}/openapi.json", server.base_url());
        let result = fetch_spec(
            &url,
            &ConditionalFetchHeaders {
                etag: Some("\"etag\"".to_owned()),
                last_modified: None,
            },
        )
        .unwrap();

        mock.assert();
        assert_eq!(result, FetchOutcome::NotModified);
    }
}
