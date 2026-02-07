pub mod classify;
pub mod discover;
pub mod fetch;

pub use classify::{SourceInput, SourceKind, classify_source};
pub use discover::discover_spec_url;
pub use fetch::{ConditionalFetchHeaders, FetchOutcome, fetch_spec};
