use std::env;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, Context, Result};
use tokio::process::Command;
use which::which;

use crate::config::Config;
use crate::domain::{load_from_file, Entry};
#[cfg(test)]
use crate::domain::{EntryId, Provenance};

const PDF_DIR_ENV: &str = "BIBR_PDF_DIR";

pub struct EditorLauncher;

impl EditorLauncher {
    pub async fn open_at_entry(entry: &Entry, editor: &str) -> Result<()> {
        let (program, mut args) = split_command(editor)?;
        let jump_args = jump_arguments(&program, &entry.provenance.file_path, entry.provenance.line_start);
        args.extend(jump_args);

        run_editor(&program, &args).await?;
        let _ = load_from_file(&entry.provenance.file_path).with_context(|| {
            format!(
                "failed to reload bibliography after editing `{}`",
                entry.provenance.file_path.display()
            )
        })?;

        Ok(())
    }

    pub fn get_editor(config: &Config) -> String {
        config
            .editor
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                env::var("EDITOR")
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            })
            .unwrap_or_else(|| "vi".to_string())
    }
}

pub struct PdfLauncher;

impl PdfLauncher {
    pub async fn open_pdf(entry: &Entry, config: &Config) -> Result<()> {
        let pdf_path = Self::find_pdf(entry)
            .with_context(|| format!("no PDF found for `{}`", entry.id.0))?;
        let reader = Self::get_reader(config)
            .with_context(|| format!("no PDF reader configured or detected for `{}`", entry.id.0))?;
        let (program, mut args) = split_command(&reader)?;

        if cfg!(target_os = "windows")
            && program.eq_ignore_ascii_case("cmd")
            && args.first().is_some_and(|arg| arg.eq_ignore_ascii_case("/C"))
            && args.get(1).is_some_and(|arg| arg.eq_ignore_ascii_case("start"))
        {
            args.push(String::new());
        }

        args.push(pdf_path.display().to_string());
        launch_reader(&program, &args).await
    }

    pub fn find_pdf(entry: &Entry) -> Option<PathBuf> {
        let bib_dir = entry.provenance.file_path.parent().unwrap_or_else(|| Path::new("."));

        if let Some(path) = entry
            .get_field("file")
            .and_then(|value| resolve_pdf_from_file_field(value, bib_dir))
        {
            return Some(path);
        }

        let sibling_pdf = bib_dir.join(format!("{}.pdf", entry.id.0));
        if sibling_pdf.is_file() {
            return Some(sibling_pdf);
        }

        let configured_pdf = configured_pdf_dir()?.join(format!("{}.pdf", entry.id.0));
        configured_pdf.is_file().then_some(configured_pdf)
    }

    pub fn get_reader(config: &Config) -> Option<String> {
        config
            .pdf_reader
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(system_default_reader)
    }
}

async fn run_editor(program: &str, args: &[String]) -> Result<()> {
    #[cfg(test)]
    if let Some(result) = maybe_run_mock(program, args)? {
        return result;
    }

    if !program.contains(std::path::MAIN_SEPARATOR) {
        which(program).with_context(|| format!("editor `{program}` is not available"))?;
    }

    let status = Command::new(program)
        .args(args)
        .status()
        .await
        .with_context(|| format!("failed to launch editor `{program}`"))?;

    if !status.success() {
        return Err(anyhow!("editor `{program}` exited with status {status}"));
    }

    Ok(())
}

async fn launch_reader(program: &str, args: &[String]) -> Result<()> {
    #[cfg(test)]
    if let Some(result) = maybe_run_mock(program, args)? {
        return result;
    }

    if !program.contains(std::path::MAIN_SEPARATOR) {
        which(program).with_context(|| format!("PDF reader `{program}` is not available"))?;
    }

    Command::new(program)
        .args(args)
        .spawn()
        .with_context(|| format!("failed to launch PDF reader `{program}`"))?;

    Ok(())
}

fn jump_arguments(program: &str, file_path: &Path, line: usize) -> Vec<String> {
    let editor = executable_name(program);
    let file = file_path.display().to_string();

    match editor.as_str() {
        "vim" | "nvim" | "vi" | "hx" | "helix" | "kak" | "nano" => {
            let jump = if editor == "nano" {
                format!("+{line},1")
            } else {
                format!("+{line}")
            };
            vec![jump, file]
        }
        "emacs" | "emacsclient" => vec![format!("+{line}:1"), file],
        "code" | "codium" | "code-insiders" => {
            vec!["--goto".to_string(), format!("{file}:{line}:1"), "--wait".to_string()]
        }
        "subl" | "sublime_text" => vec!["--wait".to_string(), format!("{file}:{line}:1")],
        "mate" => vec!["-w".to_string(), "-l".to_string(), line.to_string(), file],
        _ => vec![file],
    }
}

fn executable_name(program: &str) -> String {
    Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase()
}

fn split_command(command: &str) -> Result<(String, Vec<String>)> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;

    for ch in command.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            '\'' | '"' => {
                if quote == Some(ch) {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(ch);
                } else {
                    current.push(ch);
                }
            }
            _ if ch.is_whitespace() && quote.is_none() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if escaped || quote.is_some() {
        return Err(anyhow!("invalid command `{command}`"));
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    let program = tokens.first().cloned().ok_or_else(|| anyhow!("command is empty"))?;
    Ok((program, tokens.into_iter().skip(1).collect()))
}

fn resolve_pdf_from_file_field(raw: &str, bib_dir: &Path) -> Option<PathBuf> {
    for segment in raw.split(';') {
        let segment = segment.trim().trim_matches('{').trim_matches('}').trim_matches('"');
        if segment.is_empty() {
            continue;
        }

        let mut candidates = Vec::new();
        candidates.push(segment.to_string());

        let lower = segment.to_ascii_lowercase();
        if let Some(pdf_index) = lower.find(".pdf") {
            let prefix = &segment[..pdf_index + 4];
            candidates.push(prefix.to_string());
            if let Some(colon_index) = prefix.rfind(':') {
                if !(colon_index == 1 && prefix.chars().next().is_some_and(|ch| ch.is_ascii_alphabetic())) {
                    candidates.push(prefix[colon_index + 1..].to_string());
                }
            }
        }

        for candidate in candidates {
            let candidate = candidate.trim().trim_matches('{').trim_matches('}').trim_matches('"');
            if !looks_like_pdf(candidate) {
                continue;
            }

            let path = PathBuf::from(candidate);
            let resolved = if path.is_absolute() { path } else { bib_dir.join(path) };
            if resolved.is_file() {
                return Some(resolved);
            }
        }
    }

    None
}

fn looks_like_pdf(value: &str) -> bool {
    value.to_ascii_lowercase().ends_with(".pdf")
}

fn configured_pdf_dir() -> Option<PathBuf> {
    env::var_os(PDF_DIR_ENV)
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

fn system_default_reader() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        return Some("open".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        return Some("cmd /C start".to_string());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for candidate in ["xdg-open", "gio", "gnome-open", "kde-open"] {
            if which(candidate).is_ok() {
                return Some(candidate.to_string());
            }
        }

        None
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct Invocation {
    program: String,
    args: Vec<String>,
}

#[cfg(test)]
#[derive(Default)]
struct MockLauncherState {
    invocations: Vec<Invocation>,
}

#[cfg(test)]
fn mock_launcher_state() -> &'static Mutex<MockLauncherState> {
    static STATE: OnceLock<Mutex<MockLauncherState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(MockLauncherState::default()))
}

#[cfg(test)]
fn maybe_run_mock(program: &str, args: &[String]) -> Result<Option<Result<()>>> {
    let mut state = mock_launcher_state().lock().unwrap();
    state.invocations.push(Invocation { program: program.to_string(), args: args.to_vec() });
    Ok(Some(Ok(())))
}

#[cfg(test)]
fn take_invocations() -> Vec<Invocation> {
    let mut state = mock_launcher_state().lock().unwrap();
    std::mem::take(&mut state.invocations)
}

#[cfg(test)]
fn launcher_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
fn clear_invocations() {
    let _ = take_invocations();
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn pdf_entry(citekey: &str, bib_path: PathBuf) -> Entry {
        Entry {
            id: EntryId::from(citekey),
            entry_type: "article".to_string(),
            fields: Default::default(),
            provenance: Provenance {
                file_path: bib_path,
                line_start: 1,
                line_end: 1,
                byte_start: 0,
                byte_end: 1,
            },
        }
    }

    #[tokio::test]
    async fn open_at_entry_builds_vscode_wait_command() {
        let _guard = launcher_test_lock().lock().unwrap();
        clear_invocations();
        let dir = tempdir().unwrap();
        let path = dir.path().join("library.bib");
        fs::write(&path, "@article{alpha,\n  title = {Alpha}\n}\n").unwrap();
        let bibliography = load_from_file(&path).unwrap();
        let entry = bibliography.get(&EntryId::from("alpha")).unwrap();

        EditorLauncher::open_at_entry(entry, "code --reuse-window").await.unwrap();

        let invocations = take_invocations();
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].program, "code");
        assert_eq!(
            invocations[0].args,
            vec![
                "--reuse-window".to_string(),
                "--goto".to_string(),
                format!("{}:1:1", path.display()),
                "--wait".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn open_at_entry_builds_terminal_editor_jump_command() {
        let _guard = launcher_test_lock().lock().unwrap();
        clear_invocations();
        let dir = tempdir().unwrap();
        let path = dir.path().join("library.bib");
        fs::write(&path, "\n@article{alpha,\n  title = {Alpha}\n}\n").unwrap();
        let bibliography = load_from_file(&path).unwrap();
        let entry = bibliography.get(&EntryId::from("alpha")).unwrap();

        EditorLauncher::open_at_entry(entry, "nvim").await.unwrap();

        let invocations = take_invocations();
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].program, "nvim");
        assert_eq!(
            invocations[0].args,
            vec![format!("+{}", entry.provenance.line_start), path.display().to_string()]
        );
    }

    #[test]
    fn get_editor_prefers_config_then_environment_then_default() {
        let _guard = launcher_test_lock().lock().unwrap();
        let config = Config { editor: Some("nvim".to_string()), ..Config::default() };
        assert_eq!(EditorLauncher::get_editor(&config), "nvim");

        let config = Config { editor: None, ..Config::default() };
        unsafe {
            env::set_var("EDITOR", "hx");
        }
        assert_eq!(EditorLauncher::get_editor(&config), "hx");
        unsafe {
            env::remove_var("EDITOR");
        }
        assert_eq!(EditorLauncher::get_editor(&config), "vi");
    }

    #[test]
    fn split_command_preserves_quoted_arguments() {
        let _guard = launcher_test_lock().lock().unwrap();
        let (program, args) = split_command("code --profile 'Work Profile'").unwrap();
        assert_eq!(program, "code");
        assert_eq!(args, vec!["--profile".to_string(), "Work Profile".to_string()]);
    }

    #[test]
    fn jump_arguments_support_known_editors() {
        let _guard = launcher_test_lock().lock().unwrap();
        let path = PathBuf::from("/tmp/library.bib");
        assert_eq!(
            jump_arguments("emacs", &path, 42),
            vec!["+42:1".to_string(), path.display().to_string()]
        );
        assert_eq!(
            jump_arguments("mate", &path, 42),
            vec![
                "-w".to_string(),
                "-l".to_string(),
                "42".to_string(),
                path.display().to_string(),
            ]
        );
    }

    #[test]
    fn find_pdf_prefers_file_field() {
        let _guard = launcher_test_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let bib_path = dir.path().join("library.bib");
        let pdf_path = dir.path().join("files").join("paper.pdf");

        fs::create_dir_all(pdf_path.parent().unwrap()).unwrap();
        fs::write(&bib_path, "@article{paper,}\n").unwrap();
        fs::write(&pdf_path, b"pdf").unwrap();

        let mut entry = pdf_entry("paper", bib_path);
        entry.fields.insert("file".to_string(), "files/paper.pdf".to_string());

        assert_eq!(PdfLauncher::find_pdf(&entry), Some(pdf_path));
    }

    #[test]
    fn find_pdf_falls_back_to_sibling_pdf() {
        let _guard = launcher_test_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let bib_path = dir.path().join("library.bib");
        let pdf_path = dir.path().join("paper.pdf");

        fs::write(&bib_path, "@article{paper,}\n").unwrap();
        fs::write(&pdf_path, b"pdf").unwrap();

        let entry = pdf_entry("paper", bib_path);

        assert_eq!(PdfLauncher::find_pdf(&entry), Some(pdf_path));
    }

    #[test]
    fn find_pdf_uses_configured_pdf_directory() {
        let _guard = launcher_test_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let bib_path = dir.path().join("library.bib");
        let pdf_dir = dir.path().join("pdfs");
        let pdf_path = pdf_dir.join("paper.pdf");

        fs::write(&bib_path, "@article{paper,}\n").unwrap();
        fs::create_dir_all(&pdf_dir).unwrap();
        fs::write(&pdf_path, b"pdf").unwrap();

        unsafe {
            env::set_var(PDF_DIR_ENV, &pdf_dir);
        }

        let entry = pdf_entry("paper", bib_path);
        assert_eq!(PdfLauncher::find_pdf(&entry), Some(pdf_path));

        unsafe {
            env::remove_var(PDF_DIR_ENV);
        }
    }

    #[test]
    fn get_reader_prefers_configured_reader() {
        let _guard = launcher_test_lock().lock().unwrap();
        let config = Config { pdf_reader: Some("zathura --fork".to_string()), ..Config::default() };

        assert_eq!(PdfLauncher::get_reader(&config).as_deref(), Some("zathura --fork"));
    }

    #[tokio::test]
    async fn open_pdf_launches_configured_reader() {
        let _guard = launcher_test_lock().lock().unwrap();
        clear_invocations();
        let dir = tempdir().unwrap();
        let bib_path = dir.path().join("library.bib");
        let pdf_path = dir.path().join("paper.pdf");

        fs::write(&bib_path, "@article{paper,}\n").unwrap();
        fs::write(&pdf_path, b"pdf").unwrap();

        let entry = pdf_entry("paper", bib_path);
        let config = Config { pdf_reader: Some("zathura --fork".to_string()), ..Config::default() };

        PdfLauncher::open_pdf(&entry, &config).await.unwrap();

        let invocations = take_invocations();
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].program, "zathura");
        assert_eq!(invocations[0].args, vec!["--fork".to_string(), pdf_path.display().to_string()]);
    }
}
