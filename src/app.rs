use crate::cli::Cli;
use crate::error::AppError;
use crate::source::classify_source;
use crate::spec::load::load_spec_for_source;

pub fn run(cli: Cli) -> Result<(), AppError> {
    let source = classify_source(&cli.source)?;
    let loaded_spec = load_spec_for_source(&source)?;

    println!("Resolved source kind: {}", source.kind);
    println!("Resolved spec source: {}", loaded_spec.source_label);
    println!("Loaded OpenAPI version: {}", loaded_spec.spec.openapi);
    if let Some(cached_at_utc) = loaded_spec.cached_at_utc {
        println!("Source: {} ({cached_at_utc})", loaded_spec.cache_state);
    } else {
        println!("Source: {}", loaded_spec.cache_state);
    }

    Ok(())
}
