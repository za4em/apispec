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
}
