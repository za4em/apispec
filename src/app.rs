use crate::cli::Cli;
use crate::error::AppError;
use crate::source::classify_source;
use crate::spec::index::build_endpoint_index;
use crate::spec::load::load_spec_for_source;
use crate::tui;
use crate::tui::state::TuiContext;

pub fn run(cli: Cli) -> Result<(), AppError> {
    let source = classify_source(&cli.source)?;
    let loaded_spec = load_spec_for_source(&source)?;
    let endpoints = build_endpoint_index(&loaded_spec.spec);

    if cli.no_tui || std::env::var_os("APISPEC_NO_TUI").is_some() {
        print_plain_summary(
            &source.kind.to_string(),
            &loaded_spec.source_label,
            &loaded_spec.spec.openapi,
            &loaded_spec.cache_state.to_string(),
            loaded_spec.cached_at_utc.as_deref(),
            &endpoints,
        );
        return Ok(());
    }

    tui::run(
        TuiContext {
            source_label: loaded_spec.source_label,
            cache_state: loaded_spec.cache_state,
            cached_at_utc: loaded_spec.cached_at_utc,
            openapi_version: loaded_spec.spec.openapi,
        },
        endpoints,
        tui::TuiOptions {
            use_alt_screen: !cli.no_alt_screen
                && std::env::var_os("APISPEC_NO_ALT_SCREEN").is_none()
                && !is_ghostty(),
        },
    )
}

fn print_plain_summary(
    source_kind: &str,
    source_label: &str,
    openapi_version: &str,
    cache_state: &str,
    cached_at_utc: Option<&str>,
    endpoints: &[crate::spec::index::EndpointSummary],
) {
    println!("Resolved source kind: {source_kind}");
    println!("Resolved spec source: {source_label}");
    println!("Loaded OpenAPI version: {openapi_version}");
    match cached_at_utc {
        Some(value) => println!("Source: {cache_state} ({value})"),
        None => println!("Source: {cache_state}"),
    }
    println!("Indexed endpoints: {}", endpoints.len());

    let preview_count = endpoints.len().min(40);
    if preview_count > 0 {
        println!("Endpoint preview ({}):", preview_count);
        for endpoint in endpoints.iter().take(preview_count) {
            println!(
                "- {:<7} {}{}",
                endpoint.method,
                endpoint.path,
                endpoint
                    .operation_id
                    .as_deref()
                    .map(|id| format!(" [operationId: {id}]"))
                    .unwrap_or_default()
            );
        }
        if endpoints.len() > preview_count {
            println!("... and {} more", endpoints.len() - preview_count);
        }
    }
}

fn is_ghostty() -> bool {
    std::env::var("TERM_PROGRAM")
        .map(|value| value.eq_ignore_ascii_case("ghostty"))
        .unwrap_or(false)
}
