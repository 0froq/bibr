use bibr::cli::{Cli, Commands, OutputFormat};
use bibr::config::{self, Config};
use bibr::domain::{load_from_file, Bibliography, EntryId};
use bibr::search::{Query, SearchEngine};
use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(command) => dispatch_command(command, cli.config).await,
        None => run_tui(cli.config).await,
    }
}

async fn dispatch_command(
    command: Commands,
    config_path: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    use Commands::*;

    match command {
        List { format, query, sort } => cmd_list(format, query, sort, config_path).await,
        Show { citekey, format } => cmd_show(citekey, format, config_path).await,
        Edit { citekey } => cmd_edit(citekey, config_path).await,
        Search { query, format } => cmd_search(query, format, config_path).await,
        Note { citekey, no_open } => cmd_note(citekey, no_open, config_path).await,
        Copy { citekey } => cmd_copy(citekey, config_path).await,
        Pdf { citekey } => cmd_pdf(citekey, config_path).await,
        Init { output } => cmd_init(output).await,
        Doctor { file } => cmd_doctor(file, config_path).await,
    }
}

async fn run_tui(config_path: Option<std::path::PathBuf>) -> anyhow::Result<()> {
    eprintln!("DEBUG: Starting TUI mode...");
    
    let config = match config::load(config_path.clone()) {
        Ok(c) => {
            eprintln!("DEBUG: Config loaded, bib files: {:?}", c.bibtex_files);
            c
        }
        Err(e) => {
            eprintln!("Error loading config: {}", e);
            eprintln!("\nTip: Run 'bibr init' to create a default config file.");
            std::process::exit(1);
        }
    };
    
    let bib = match load_bibliography(&config) {
        Ok(b) => {
            eprintln!("DEBUG: Loaded {} entries", b.entries.len());
            b
        }
        Err(e) => {
            eprintln!("Error loading bibliography: {}", e);
            std::process::exit(1);
        }
    };
    
    use bibr::ui::{TuiApp, run_tui};
    let app = TuiApp::new(bib, config.clone());
    run_tui(app, &config).await.map(|_| ())
}

async fn cmd_list(
    format: OutputFormat,
    query: Option<String>,
    sort: Option<String>,
    config_path: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let config = config::load(config_path)?;
    let bib = load_bibliography(&config)?;

    let entries = if let Some(q) = query {
        let search_engine = SearchEngine::new(config.search.clone());
        let query = Query::parse(&q);
        let results = search_engine.search(&bib, &query);
        results
            .into_iter()
            .filter_map(|r| bib.get(&r.entry_id))
            .collect::<Vec<_>>()
    } else {
        bib.iter().collect::<Vec<_>>()
    };

    let mut entries = entries;
    if let Some(sort_field) = sort {
        match sort_field.as_str() {
            "year" => entries.sort_by(|a, b| a.year().cmp(&b.year())),
            "author" => entries.sort_by(|a, b| {
                let a_authors = a.authors().join(", ");
                let b_authors = b.authors().join(", ");
                a_authors.cmp(&b_authors)
            }),
            "title" => entries.sort_by(|a, b| a.title().cmp(&b.title())),
            _ => {}
        }
    }

    output_entries(&entries, format, &config.display.format)?;
    Ok(())
}

async fn cmd_show(
    citekey: String,
    format: OutputFormat,
    config_path: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let config = config::load(config_path)?;
    let bib = load_bibliography(&config)?;

    let entry_id = EntryId(citekey.clone());
    let entry = bib
        .get(&entry_id)
        .ok_or_else(|| anyhow::anyhow!("Entry not found: {}", citekey))?;

    match format {
        OutputFormat::Json => {
            println!("Citekey: {}", entry.id);
            println!("Type: {}", entry.entry_type);
            println!("Title: {}", entry.title().unwrap_or("N/A"));
            println!("Authors: {}", entry.authors().join("; "));
            println!("Year: {}", entry.year().map(|y| y.to_string()).unwrap_or_else(|| "N/A".to_string()));
            for (key, value) in &entry.fields {
                println!("{}: {}", key, value);
            }
        }
        OutputFormat::Yaml => {
            println!("Citekey: {}", entry.id);
            println!("Type: {}", entry.entry_type);
            println!("Title: {}", entry.title().unwrap_or("N/A"));
            println!("Authors: {}", entry.authors().join("; "));
            println!("Year: {}", entry.year().map(|y| y.to_string()).unwrap_or_else(|| "N/A".to_string()));
            for (key, value) in &entry.fields {
                println!("{}: {}", key, value);
            }
        }
        OutputFormat::Plain => {
            println!("Citekey: {}", entry.id);
            println!("Type: {}", entry.entry_type);
            println!("Title: {}", entry.title().unwrap_or("N/A"));
            println!("Authors: {}", entry.authors().join("; "));
            println!("Year: {}", entry.year().map(|y| y.to_string()).unwrap_or_else(|| "N/A".to_string()));
            for (key, value) in &entry.fields {
                println!("{}: {}", key, value);
            }
        }
        OutputFormat::Table => {
            println!("Citekey: {}", entry.id);
            println!("Type: {}", entry.entry_type);
            println!("Title: {}", entry.title().unwrap_or("N/A"));
            println!("Authors: {}", entry.authors().join("; "));
            println!("Year: {}", entry.year().map(|y| y.to_string()).unwrap_or_else(|| "N/A".to_string()));
        }
    }

    Ok(())
}

async fn cmd_edit(
    citekey: String,
    config_path: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let config = config::load(config_path)?;
    let bib = load_bibliography(&config)?;

    let entry_id = EntryId(citekey.clone());
    let entry = bib
        .get(&entry_id)
        .ok_or_else(|| anyhow::anyhow!("Entry not found: {}", citekey))?;

    let editor = config.editor.clone().unwrap_or_else(|| {
        std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string())
    });

    println!("Opening {} in {}...", citekey, editor);
    
    use bibr::infra::launcher::EditorLauncher;
    EditorLauncher::open_at_entry(entry, &editor).await?;
    
    println!("Editor closed. Entry updated.");
    Ok(())
}

async fn cmd_search(
    query: String,
    format: OutputFormat,
    config_path: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    cmd_list(format, Some(query), None, config_path).await
}

async fn cmd_note(
    citekey: String,
    no_open: bool,
    config_path: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let config = config::load(config_path)?;
    let bib = load_bibliography(&config)?;

    let entry_id = EntryId(citekey.clone());
    let entry = bib
        .get(&entry_id)
        .ok_or_else(|| anyhow::anyhow!("Entry not found: {}", citekey))?;

    use bibr::services::notes::NotesService;
    let notes_service = NotesService::new(config.notes.clone());

    if no_open {
        let note_path = notes_service.ensure_note_exists(entry).await?;
        println!("Note created at: {:?}", note_path);
    } else {
        let note_path = notes_service.create_or_open_note(entry).await?;
        println!("Opening note at: {:?}", note_path);
    }
    
    Ok(())
}

async fn cmd_copy(
    citekey: String,
    config_path: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let _config = config::load(config_path)?;
    
    use bibr::infra::clipboard::ClipboardService;
    ClipboardService::copy(&citekey)?;
    
    println!("Copied citekey '{}' to clipboard", citekey);
    Ok(())
}

async fn cmd_pdf(
    citekey: String,
    config_path: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let config = config::load(config_path)?;
    let bib = load_bibliography(&config)?;

    let entry_id = EntryId(citekey.clone());
    let entry = bib
        .get(&entry_id)
        .ok_or_else(|| anyhow::anyhow!("Entry not found: {}", citekey))?;

    use bibr::infra::launcher::PdfLauncher;
    PdfLauncher::open_pdf(entry, &config).await?;
    
    println!("Opening PDF for {}...", citekey);
    Ok(())
}

async fn cmd_init(output: Option<std::path::PathBuf>) -> anyhow::Result<()> {
    use std::fs;
    
    let config_path = output.unwrap_or_else(|| {
        std::env::var("HOME")
            .map(|home| std::path::PathBuf::from(home).join(".config").join("bibr").join("bibr.toml"))
            .expect("Could not determine config directory: HOME not set")
    });

    if config_path.exists() {
        println!("Config already exists at: {:?}", config_path);
        return Ok(());
    }

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let default_config = r#"# BIBR Configuration
# Add your BibTeX files below

bibtex_files = [
    # "~/path/to/your.bib",
]

[search]
smart_case = true
fuzzy = true

[keybindings]
up = "k"
down = "j"
search = "/"
quit = "q"
edit = "e"
note = "n"
pdf = "p"
copy = "y"

[notes]
dir = "~/.local/share/bibr/notes"
filename_pattern = "{citekey}.md"
"#;

    fs::write(&config_path, default_config)?;
    println!("Created default config at: {:?}", config_path);
    println!("Edit this file to add your BibTeX files.");
    
    Ok(())
}

async fn cmd_doctor(
    file: Option<std::path::PathBuf>,
    config_path: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    use bibr::domain::load_from_file_with_diagnostics;
    
    let bib_file = match file {
        Some(f) => f,
        None => {
            let config = config::load(config_path)?;
            config.bibtex_files.first()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("No BibTeX file specified and no files in config. Use --file or add files to config."))?
        }
    };
    
    let bib_file = std::path::PathBuf::from(shellexpand::tilde(&bib_file.to_string_lossy()).to_string());
    
    println!("🔍 Diagnosing BibTeX file: {:?}\n", bib_file);
    
    if !bib_file.exists() {
        println!("❌ ERROR: File not found!");
        return Ok(());
    }
    
    let result = load_from_file_with_diagnostics(&bib_file)?;
    
    println!("📊 Statistics:");
    println!("   Total blocks found: {}", result.total_blocks);
    println!("   Entries parsed: {}", result.parsed_entries);
    println!("   Warnings: {}", result.warnings.len());
    
    if result.warnings.is_empty() {
        println!("\n✅ All entries loaded successfully!");
    } else {
        println!("\n⚠️  Warnings ({}):", result.warnings.len());
        for (i, warning) in result.warnings.iter().enumerate() {
            println!("\n   Warning #{}:", i + 1);
            println!("   Lines: {}-{}", warning.line_start, warning.line_end);
            if let Some(citekey) = &warning.citekey {
                println!("   Citekey: {}", citekey);
            }
            println!("   Reason: {}", warning.message);
        }
    }
    
    let missing_count = result.total_blocks.saturating_sub(result.parsed_entries).saturating_sub(result.warnings.len());
    if missing_count > 0 {
        println!("\n⚠️  Note: {} entries were silently skipped (possibly comments, @string, or @preamble)", missing_count);
    }
    
    println!("\n💡 Tip: Run 'bibr list' to see all loaded entries.");
    
    Ok(())
}

fn load_bibliography(config: &Config) -> anyhow::Result<Bibliography> {
    if config.bibtex_files.is_empty() {
        return Err(anyhow::anyhow!(
            "No BibTeX files configured. Add files to your config or run 'bibr init'"
        ));
    }

    let mut bib = Bibliography::new();
    for path in &config.bibtex_files {
        let path_str = path.to_string_lossy().to_string();
        let expanded = shellexpand::tilde(&path_str);
        let path = std::path::PathBuf::from(expanded.as_ref());
        
        if !path.exists() {
            eprintln!("Warning: BibTeX file not found: {:?}", path);
            continue;
        }
        
        let file_bib = load_from_file(&path)?;
        
        if let Err(e) = bib.merge(file_bib) {
            eprintln!("Warning: {}", e);
        }
    }

    if bib.entries.is_empty() {
        return Err(anyhow::anyhow!(
            "No entries found in configured BibTeX files"
        ));
    }

    Ok(bib)
}

fn output_entries(
    entries: &[&bibr::domain::Entry],
    format: OutputFormat,
    display_format: &str,
) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => {
            for entry in entries {
                println!("{{");
                println!("  \"citekey\": \"{}\",", entry.id);
                println!("  \"type\": \"{}\",", entry.entry_type);
                println!("  \"title\": \"{}\",", entry.title().unwrap_or("N/A"));
                println!("  \"authors\": \"{}\",", entry.authors().join("; "));
                println!("  \"year\": {}", entry.year().map(|y| y.to_string()).unwrap_or_else(|| "null".to_string()));
                println!("}}");
            }
        }
        OutputFormat::Yaml => {
            for entry in entries {
                println!("- citekey: {}", entry.id);
                println!("  type: {}", entry.entry_type);
                println!("  title: {}", entry.title().unwrap_or("N/A"));
                println!("  authors: {}", entry.authors().join("; "));
                println!("  year: {}", entry.year().map(|y| y.to_string()).unwrap_or_else(|| "null".to_string()));
            }
        }
        OutputFormat::Plain => {
            for entry in entries {
                println!("{}", format_entry(entry, display_format));
            }
        }
        OutputFormat::Table => {
            println!("{:<20} {:<40} {:<30}", "Citekey", "Title", "Authors");
            println!("{}", "-".repeat(90));
            for entry in entries {
                let title = entry.title().unwrap_or("N/A");
                let title = if title.chars().count() > 37 {
                    let truncated: String = title.chars().take(34).collect();
                    format!("{}...", truncated)
                } else {
                    title.to_string()
                };
                
                let authors = entry.authors().join("; ");
                let authors = if authors.chars().count() > 27 {
                    let truncated: String = authors.chars().take(24).collect();
                    format!("{}...", truncated)
                } else {
                    authors
                };
                
                println!("{:<20} {:<40} {:<30}", entry.id, title, authors);
            }
            println!("\n{} entries found", entries.len());
        }
    }
    Ok(())
}

fn format_entry(entry: &bibr::domain::Entry, format: &str) -> String {
    format
        .replace("{citekey}", &entry.id.to_string())
        .replace("{author}", &entry.authors().first().map(|s| s.as_str()).unwrap_or("N/A"))
        .replace("{title}", entry.title().unwrap_or("N/A"))
        .replace("{year}", &entry.year().map(|y| y.to_string()).unwrap_or_default())
}
