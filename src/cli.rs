use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "apispec",
    version,
    about = "Inspect OpenAPI 3.1.0 specifications from local files or URLs."
)]
pub struct Cli {
    #[arg(
        value_name = "source",
        help = "Local spec file path, direct spec URL, or base API URL."
    )]
    pub source: String,
}
