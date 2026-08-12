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
    #[arg(long, default_value = "zh")]
    pub lang: String,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Compute 3-layer signal lights + 9 indicators + tension analysis
    Score {
        /// Filter observations since date (YYYY-MM-DD), default: last 90 days
        #[arg(long)]
        since: Option<String>,
        /// Show all observations regardless of date (overrides default 90-day window)
        #[arg(long)]
        all: bool,
        /// Return a failure if LLM advice cannot be generated (for automation).
        #[arg(long)]
        require_advice: bool,
    },
    /// One-line briefing (add to .zshrc)
    Motd,
    /// Full ASCII dashboard
    Dashboard {
        /// Filter observations since date (YYYY-MM-DD), default: last 90 days
        #[arg(long)]
        since: Option<String>,
        /// Show all observations regardless of date (overrides default 90-day window)
        #[arg(long)]
        all: bool,
    },
    /// Weekly delta analysis (requires LLM)
    Weekly,
    /// Generate cognitive portrait narrative (requires LLM)
    Profile,
}
