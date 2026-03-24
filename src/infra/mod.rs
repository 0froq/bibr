pub mod bib_store;
pub mod clipboard;
pub mod launcher;
pub mod watcher;

pub use bib_store::BibStore;
pub use clipboard::{Action, ClipboardService};
pub use launcher::{EditorLauncher, PdfLauncher};
pub use watcher::FileWatcher;

pub struct Infra;

impl Infra {
    pub fn new() -> Self {
        Infra
    }
}
