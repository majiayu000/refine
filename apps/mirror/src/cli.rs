use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mirror")]
#[command(about = "Mirror — cognitive growth tracker for AI-assisted development")]
#[command(version)]
pub struct Cli {
    /// Database path (defaults to shared refine path)
    #[arg(long)]
    pub db: Option<String>,

    /// Display language: en or zh (default: en)
    #[arg(long, default_value = "en")]
    pub lang: String,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Compute 3-layer signal lights + 9 indicators + tension analysis
    Score,
    /// One-line briefing (add to .zshrc)
    Motd,
    /// Full ASCII dashboard
    Dashboard,
    /// Weekly delta analysis (requires LLM)
    Weekly,
    /// Generate cognitive portrait narrative (requires LLM)
    Profile,
}
