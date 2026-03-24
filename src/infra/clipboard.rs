use std::env;
use std::fs;
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

use anyhow::{bail, Context, Result};

use crate::domain::Entry;

const CLIPBOARD_TEST_FILE_ENV: &str = "BIBR_CLIPBOARD_TEST_FILE";

pub struct ClipboardService;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Continue,
    Quit,
}

impl ClipboardService {
    pub fn copy(text: &str) -> Result<()> {
        if let Some(path) = env::var_os(CLIPBOARD_TEST_FILE_ENV) {
            fs::write(&path, text).with_context(|| {
                format!("failed to write clipboard mock file `{}`", path.to_string_lossy())
            })?;
            return Ok(());
        }

        copy_with_system_clipboard(text)
    }

    pub fn copy_citekey(entry: &Entry, auto_close: bool) -> Result<Action> {
        Self::copy(&entry.id.0)?;
        Ok(if auto_close { Action::Quit } else { Action::Continue })
    }
}

#[cfg(feature = "arboard")]
fn copy_with_system_clipboard(text: &str) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new().context("failed to access system clipboard")?;
    clipboard
        .set_text(text.to_string())
        .context("failed to copy text to clipboard")
}

#[cfg(not(feature = "arboard"))]
fn copy_with_system_clipboard(_text: &str) -> Result<()> {
    bail!("clipboard support requires the `arboard` feature")
}

#[cfg(test)]
fn clipboard_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tempfile::tempdir;

    use super::*;
    use crate::domain::{Entry, EntryId, Provenance};

    fn entry(citekey: &str) -> Entry {
        Entry {
            id: EntryId::from(citekey),
            entry_type: "article".to_string(),
            fields: HashMap::new(),
            provenance: Provenance {
                file_path: "library.bib".into(),
                line_start: 1,
                line_end: 1,
                byte_start: 0,
                byte_end: 1,
            },
        }
    }

    #[test]
    fn copy_writes_to_mock_clipboard_file() {
        let _guard = clipboard_test_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let clipboard_file = dir.path().join("clipboard.txt");

        unsafe {
            env::set_var(CLIPBOARD_TEST_FILE_ENV, &clipboard_file);
        }
        ClipboardService::copy("knuth1984").unwrap();
        unsafe {
            env::remove_var(CLIPBOARD_TEST_FILE_ENV);
        }

        assert_eq!(fs::read_to_string(clipboard_file).unwrap(), "knuth1984");
    }

    #[test]
    fn copy_citekey_returns_action_for_auto_close() {
        let _guard = clipboard_test_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let clipboard_file = dir.path().join("clipboard.txt");

        unsafe {
            env::set_var(CLIPBOARD_TEST_FILE_ENV, &clipboard_file);
        }
        let quit_action = ClipboardService::copy_citekey(&entry("knuth1984"), true).unwrap();
        let continue_action = ClipboardService::copy_citekey(&entry("lamport1994"), false).unwrap();
        unsafe {
            env::remove_var(CLIPBOARD_TEST_FILE_ENV);
        }

        assert_eq!(quit_action, Action::Quit);
        assert_eq!(continue_action, Action::Continue);
        assert_eq!(fs::read_to_string(clipboard_file).unwrap(), "lamport1994");
    }
}
