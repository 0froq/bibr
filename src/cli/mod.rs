use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "bibr")]
#[command(about = "BibTeX Reference Manager - TUI and CLI")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
    
    /// Config file path
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,
    
    /// Run in TUI mode (default if no subcommand)
    #[arg(short, long)]
    pub tui: bool,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// List all entries
    #[command(name = "list")]
    List {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
        
        /// Search query
        #[arg(short, long)]
        query: Option<String>,
        
        /// Sort by field
        #[arg(short, long)]
        sort: Option<String>,
    },
    
    /// Show entry details
    #[command(name = "show")]
    Show {
        /// Citekey
        citekey: String,
        
        /// Output format
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,
    },
    
    /// Edit entry in external editor
    #[command(name = "edit")]
    Edit {
        /// Citekey
        citekey: String,
    },
    
    /// Search entries
    #[command(name = "search")]
    Search {
        /// Search query
        query: String,
        
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
    
    /// Create or open note for entry
    #[command(name = "note")]
    Note {
        /// Citekey
        citekey: String,
        
        /// Don't open editor, just create
        #[arg(short, long)]
        no_open: bool,
    },
    
    /// Copy citekey to clipboard
    #[command(name = "copy")]
    Copy {
        /// Citekey
        citekey: String,
    },
    
    /// Open PDF for entry
    #[command(name = "pdf")]
    Pdf {
        /// Citekey
        citekey: String,
    },
    
    /// Initialize default config
    #[command(name = "init")]
    Init {
        /// Output path
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Diagnose BibTeX file for issues
    #[command(name = "doctor")]
    Doctor {
        /// BibTeX file to check (uses first file from config if not specified)
        #[arg(short, long)]
        file: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
    Yaml,
    Plain,
}

impl OutputFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            OutputFormat::Table => "table",
            OutputFormat::Json => "json",
            OutputFormat::Yaml => "yaml",
            OutputFormat::Plain => "plain",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    
    #[test]
    fn test_cli_command_factory() {
        Cli::command().debug_assert();
    }
    
    #[test]
    fn test_list_command_parsing() {
        let cli = Cli::parse_from([
            "bibr", "list", "--format", "json", "--query", "test"
        ]);
        match cli.command {
            Some(Commands::List { format, query, sort }) => {
                assert!(matches!(format, OutputFormat::Json));
                assert_eq!(query, Some("test".to_string()));
                assert_eq!(sort, None);
            }
            _ => panic!("Expected list command"),
        }
    }
    
    #[test]
    fn test_show_command_parsing() {
        let cli = Cli::parse_from([
            "bibr", "show", "knuth1984", "--format", "yaml"
        ]);
        match cli.command {
            Some(Commands::Show { citekey, format }) => {
                assert_eq!(citekey, "knuth1984");
                assert!(matches!(format, OutputFormat::Yaml));
            }
            _ => panic!("Expected show command"),
        }
    }
}
