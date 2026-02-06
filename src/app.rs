use crate::cli::Cli;
use crate::error::AppError;
use crate::source::classify_source;

pub fn run(cli: Cli) -> Result<(), AppError> {
    let source = classify_source(&cli.source)?;
    println!("Resolved source kind: {}", source.kind);
    Ok(())
}
