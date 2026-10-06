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
    /// Compute 3-layer signal lights + 8 indicators + tension analysis
    Score {
        /// View observations since date (YYYY-MM-DD) without updating canonical history
        #[arg(long)]
        since: Option<String>,
        /// View all session observations without updating canonical history
        #[arg(long)]
        all: bool,
        /// Require current local portfolio advice to be available (no LLM needed).
        #[arg(long, conflicts_with_all = ["since", "all"])]
        require_advice: bool,
    },
    /// One-line briefing (add to .zshrc)
    Motd,
    /// Full ASCII dashboard
    Dashboard {
        /// View observations since date (YYYY-MM-DD) without updating canonical history
        #[arg(long)]
        since: Option<String>,
        /// View all session observations without updating canonical history
        #[arg(long)]
        all: bool,
    },
    /// Weekly local metrics-delta report with deterministic action cards
    Weekly,
    /// Generate cognitive portrait narrative (requires LLM)
    Profile,
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::CommandFactory;

    #[test]
    fn weekly_help_and_readme_describe_local_generation() {
        let mut command = Cli::command();
        let help = command
            .find_subcommand_mut("weekly")
            .unwrap()
            .render_long_help()
            .to_string();
        assert!(help.contains("local"), "{help}");
        assert!(!help.contains("requires LLM"), "{help}");

        let readme = include_str!("../../../README.md");
        let weekly_lines: Vec<_> = readme
            .lines()
            .filter(|line| line.contains("mirror weekly "))
            .collect();
        assert_eq!(weekly_lines.len(), 2);
        for line in weekly_lines {
            assert!(line.contains("local"), "{line}");
            assert!(
                !line.contains("via LLM") && !line.contains("requires LLM"),
                "{line}"
            );
        }
    }
}
