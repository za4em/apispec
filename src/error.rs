use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Invalid source `{input}`: failed to parse URL ({source}).")]
    InvalidUrl {
        input: String,
        #[source]
        source: url::ParseError,
    },

    #[error(
        "Unsupported URL scheme `{scheme}` in source `{input}`. Supported schemes: http, https."
    )]
    UnsupportedUrlScheme { input: String, scheme: String },

    #[error("Local path does not exist: `{path}`.")]
    LocalPathNotFound { path: PathBuf },

    #[error("Local path is not a file: `{path}`.")]
    LocalPathNotFile { path: PathBuf },

    #[error("Failed to canonicalize local path `{path}` ({source}).")]
    CanonicalizePath {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to read local path `{path}` ({source}).")]
    ReadLocalPath {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "Base API URL discovery for `{input}` is not implemented in Phase 2. Pass a direct spec URL or local file."
    )]
    BaseUrlDiscoveryNotImplemented { input: String },

    #[error("Could not determine a cache directory on this system.")]
    CacheDirUnavailable,

    #[error("Cache I/O error at `{path}` ({source}).")]
    CacheIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to serialize cache metadata ({source}).")]
    CacheMetadataSerialize {
        #[source]
        source: serde_json::Error,
    },

    #[error("Network unavailable while fetching `{url}` ({source}).")]
    NetworkUnavailable {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error(
        "Network unavailable while fetching `{url}`, and no cached spec is available ({source})."
    )]
    OfflineNoCache {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("Request failed while fetching `{url}` ({source}).")]
    FetchFailed {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("Received HTTP status {status} from `{url}`{details}.")]
    HttpStatus {
        url: String,
        status: u16,
        details: String,
    },

    #[error("Received HTTP 304 for `{url}` but no cached specification was found.")]
    NotModifiedWithoutCache { url: String },

    #[error(
        "Could not parse OpenAPI document from `{source_label}` as JSON or YAML.\nJSON error: {json_error}\nYAML error: {yaml_error}"
    )]
    SpecParse {
        source_label: String,
        json_error: String,
        yaml_error: String,
    },

    #[error("Unsupported OpenAPI version `{found}`. This tool currently supports only `3.1.0`.")]
    UnsupportedOpenApiVersion { found: String },
}
