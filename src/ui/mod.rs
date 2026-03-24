pub mod app;
pub mod events;
pub mod widgets;

use std::io::{self, Stdout};

use anyhow::Result;
use crossterm::{
    execute,
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::infra::launcher::EditorLauncher;

fn drain_pending_events() {
    use crossterm::event;
    use std::time::Duration;

    let timeout = Duration::from_millis(0);
    let mut drained = 0;
    const MAX_DRAIN: usize = 100;

    while drained < MAX_DRAIN {
        match event::poll(timeout) {
            Ok(true) => {
                if let Ok(_event) = event::read() {
                    drained += 1;
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
}

pub use app::{
    ActionHandler, Mode, SortField, SystemActionHandler, TuiApp, TuiEffect,
};
pub use events::{map_crossterm_event, EventHandler, UiEvent};
pub use widgets::color_from_name;

pub async fn run_tui(mut app: TuiApp, config: &crate::config::Config) -> Result<Option<crate::domain::EntryId>> {
    use crate::services::notes::NotesService;

    let mut terminal = init_terminal(config.mouse_enabled)?;
    let mut result;

    loop {
        result = app.run(&mut terminal, config.mouse_enabled).await;

        match result {
            Ok(None) if matches!(app.last_effect, Some(crate::ui::app::TuiEffect::EditEntry(_))) => {
                let entry_id = match &app.last_effect {
                    Some(crate::ui::app::TuiEffect::EditEntry(id)) => id.clone(),
                    _ => break,
                };

                if let Err(e) = restore_terminal(&mut terminal, config.mouse_enabled) {
                    eprintln!("Failed to restore terminal: {}", e);
                    return Ok(Some(entry_id));
                }

                if let Some(entry) = app.get_bibliography().get(&entry_id) {
                    let editor = EditorLauncher::get_editor(config);
                    if let Err(e) = EditorLauncher::open_at_entry(entry, &editor).await {
                        eprintln!("Editor error: {}", e);
                    }
                }

                drain_pending_events();

                terminal = match init_terminal(config.mouse_enabled) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("Failed to reinitialize terminal: {}", e);
                        return Ok(Some(entry_id));
                    }
                };

                app.last_effect = None;
                continue;
            }
            Ok(None) if matches!(app.last_effect, Some(crate::ui::app::TuiEffect::OpenNote(_))) => {
                let entry_id = match &app.last_effect {
                    Some(crate::ui::app::TuiEffect::OpenNote(id)) => id.clone(),
                    _ => break,
                };

                if let Err(e) = restore_terminal(&mut terminal, config.mouse_enabled) {
                    eprintln!("Failed to restore terminal: {}", e);
                    return Ok(Some(entry_id));
                }

                if let Some(entry) = app.get_bibliography().get(&entry_id) {
                    let notes_service = NotesService::new(config.notes.clone());

                    // create_or_open_note already opens the editor internally
                    if let Err(e) = notes_service.create_or_open_note(entry).await {
                        eprintln!("Note error: {}", e);
                    }
                }

                drain_pending_events();

                terminal = match init_terminal(config.mouse_enabled) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("Failed to reinitialize terminal: {}", e);
                        return Ok(Some(entry_id));
                    }
                };

                app.last_effect = None;
                continue;
            }
            _ => break,
        }
    }

    let _ = restore_terminal(&mut terminal, config.mouse_enabled);
    result
}

fn init_terminal(mouse_enabled: bool) -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;

    let mut stdout = io::stdout();
    if mouse_enabled {
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    } else {
        execute!(stdout, EnterAlternateScreen)?;
    }

    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>, mouse_enabled: bool) -> Result<()> {
    disable_raw_mode()?;
    if mouse_enabled {
        execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    } else {
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    }
    terminal.show_cursor()?;
    Ok(())
}
