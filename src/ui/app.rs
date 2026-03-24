use std::{
    cmp::Ordering,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout},
    prelude::*,
    Terminal,
};
use crate::{
    config::Config,
    domain::{Bibliography, Entry, EntryId},
    search::{Query, QueryTerm, SearchEngine},
    services::notes::NotesService,
};

use super::{
    events::{EventHandler, UiEvent},
    widgets::{
        color_from_name, render_entry_list, render_status_bar,
        EntryListView, StatusBarView,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Searching,
    Sorting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Year,
    Author,
    Journal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiEffect {
    Continue,
    Quit,
    Select(EntryId),
    EditEntry(EntryId),
    OpenNote(EntryId),
}

impl Default for TuiEffect {
    fn default() -> Self {
        TuiEffect::Continue
    }
}

pub trait ActionHandler: Send {
    fn edit_entry(&mut self, entry: &Entry, config: &Config) -> Result<String>;
    fn open_note(&mut self, entry: &Entry, config: &Config) -> Result<String>;
    fn open_pdf(&mut self, entry: &Entry, config: &Config) -> Result<String>;
    fn copy_citekey(&mut self, citekey: &str) -> Result<String>;
}

#[derive(Debug, Default)]
pub struct SystemActionHandler;

pub struct TuiApp {
    bib: Bibliography,
    config: Config,
    search_engine: SearchEngine,
    pub search_query: String,
    pub selected: usize,
    pub filtered: Vec<EntryId>,
    pub mode: Mode,
    pub sort_field: Option<SortField>,
    status_message: String,
    scroll_offset: usize,
    list_height: usize,
    pending_g: bool,
    action_handler: Box<dyn ActionHandler>,
    cursor_position: usize,
    show_preview: Option<usize>,
    pub last_effect: Option<TuiEffect>,
}

impl TuiApp {
    pub fn new(bib: Bibliography, config: Config) -> Self {
        Self::with_action_handler(bib, config, Box::<SystemActionHandler>::default())
    }

    pub fn with_action_handler(
        bib: Bibliography,
        config: Config,
        action_handler: Box<dyn ActionHandler>,
    ) -> Self {
        let search_engine = SearchEngine::new(config.search.clone());
        let mut app = Self {
            bib,
            config,
            search_engine,
            search_query: String::new(),
            selected: 0,
            filtered: Vec::new(),
            mode: Mode::Normal,
            sort_field: None,
            status_message: "Ready".to_string(),
            scroll_offset: 0,
            list_height: 10,
            pending_g: false,
            action_handler,
            cursor_position: 0,
            show_preview: None,
            last_effect: None,
        };
        app.refresh_filtered();
        app
    }

    pub fn get_bibliography(&self) -> &Bibliography {
        &self.bib
    }

    pub async fn run<B>(&mut self, terminal: &mut Terminal<B>, mouse_enabled: bool) -> Result<Option<EntryId>>
    where
        B: Backend,
        <B as Backend>::Error: std::error::Error + Send + Sync + 'static,
    {
        let mut events = EventHandler::new(Duration::from_millis(200));

        loop {
            terminal.draw(|frame| self.draw(frame))?;

            match events.next().await? {
                UiEvent::Input(key) => {
                    let effect = self.handle_key_event(key)?;
                    match effect {
                        TuiEffect::Continue => {}
                        TuiEffect::Quit => {
                            events.shutdown().await;
                            return Ok(None);
                        }
                        TuiEffect::Select(id) => {
                            events.shutdown().await;
                            return Ok(Some(id));
                        }
                        TuiEffect::EditEntry(_) | TuiEffect::OpenNote(_) => {
                            self.last_effect = Some(effect.clone());
                            events.shutdown().await;
                            return Ok(None);
                        }
                    }
                }
                UiEvent::Mouse(mouse) if mouse_enabled => {
                    self.handle_mouse_event(mouse)?;
                }
                UiEvent::Resize(_, _) | UiEvent::Tick | UiEvent::Mouse(_) => {}
            }
        }
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(frame.area());

        let list_area = main_layout[0];
        let status_area = main_layout[1];

        let preview_height: u16 = if self.show_preview.is_some() { 8 } else { 0 };
        self.list_height = list_area.height.saturating_sub(2).max(1) as usize;
        self.ensure_visible_with_context(3);

        let visible_items = self
            .visible_entry_ids_with_preview(preview_height as usize)
            .iter()
            .filter_map(|id| self.bib.get(id))
            .map(|entry| self.format_entry_line(entry, list_area.width as usize))
            .collect::<Vec<_>>();

        let preview_content = if let Some(preview_idx) = self.show_preview {
            self.selected_entry()
                .map(|entry| format_preview_with_template(entry, &self.config.preview.patterns, preview_idx))
        } else {
            None
        };

        let preview_ref = preview_content.as_deref();

        render_entry_list(
            frame,
            list_area,
            EntryListView {
                items: &visible_items,
                selected: self.relative_selected(),
                total_items: self.filtered.len(),
                scroll_offset: self.scroll_offset,
                theme: &self.config.theme,
                search_query: if self.mode == Mode::Searching { &self.search_query } else { "" },
                show_preview: self.show_preview,
                preview_content: preview_ref,
                preview_height,
            },
        );

        let status_text = self.status_line();
        render_status_bar(
            frame,
            status_area,
            StatusBarView {
                text: if self.mode == Mode::Searching { &self.search_query } else { &status_text },
                is_search_mode: self.mode == Mode::Searching,
                cursor_position: if self.mode == Mode::Searching { Some(self.cursor_position) } else { None },
                theme: &self.config.theme,
            },
        );
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) -> Result<TuiEffect> {
        match self.mode {
            Mode::Normal => self.handle_normal_mode(key),
            Mode::Searching => self.handle_search_mode(key),
            Mode::Sorting => self.handle_sort_mode(key),
        }
    }

    fn handle_normal_mode(&mut self, key: KeyEvent) -> Result<TuiEffect> {
        let key_name = key_event_to_binding_name(key);
        let mapped_action = self.get_keybinding_action(&key_name).cloned();

        if self.pending_g && key.code != KeyCode::Char('g') {
            self.pending_g = false;
        }

        if let Some(action) = mapped_action {
            return self.execute_action_binding(&action);
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('d') => {
                    self.page_down();
                    return Ok(TuiEffect::Continue);
                }
                KeyCode::Char('u') => {
                    self.page_up();
                    return Ok(TuiEffect::Continue);
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('q') => Ok(TuiEffect::Quit),
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_down(1);
                Ok(TuiEffect::Continue)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_up(1);
                Ok(TuiEffect::Continue)
            }
            KeyCode::Char('g') => {
                if self.pending_g {
                    self.pending_g = false;
                    self.move_to_top();
                } else {
                    self.pending_g = true;
                }
                Ok(TuiEffect::Continue)
            }
            KeyCode::Char('G') => {
                self.move_to_bottom();
                Ok(TuiEffect::Continue)
            }
            KeyCode::Char('/') => {
                self.mode = Mode::Searching;
                self.status_message = "Search mode".to_string();
                Ok(TuiEffect::Continue)
            }
            KeyCode::Enter => match self.selected_entry_id() {
                Some(id) => Ok(TuiEffect::Select(id.clone())),
                None => Ok(TuiEffect::Continue),
            },
            KeyCode::Char('e') => match self.selected_entry_id() {
                Some(id) => Ok(TuiEffect::EditEntry(id.clone())),
                None => {
                    self.status_message = "No entry selected".to_string();
                    Ok(TuiEffect::Continue)
                }
            },
            KeyCode::Char('n') => match self.selected_entry_id() {
                Some(id) => Ok(TuiEffect::OpenNote(id.clone())),
                None => {
                    self.status_message = "No entry selected".to_string();
                    Ok(TuiEffect::Continue)
                }
            },
            KeyCode::Char('p') => self.run_entry_action(|handler, entry, config| handler.open_pdf(entry, config)),
            KeyCode::Char('y') => self.copy_selected_citekey(),
            KeyCode::Char('s') => {
                self.mode = Mode::Sorting;
                self.status_message = "Sort mode".to_string();
                Ok(TuiEffect::Continue)
            }
            KeyCode::Char('i') => {
                self.toggle_preview(None);
                Ok(TuiEffect::Continue)
            }
            _ => Ok(TuiEffect::Continue),
        }
    }

    fn execute_action_binding(&mut self, action: &str) -> Result<TuiEffect> {
        let (base, params) = parse_action_binding(action);
        match base {
            "quit" => Ok(TuiEffect::Quit),
            "up" => {
                self.move_up(1);
                Ok(TuiEffect::Continue)
            }
            "down" => {
                self.move_down(1);
                Ok(TuiEffect::Continue)
            }
            "page_up" => {
                self.page_up();
                Ok(TuiEffect::Continue)
            }
            "page_down" => {
                self.page_down();
                Ok(TuiEffect::Continue)
            }
            "goto_top" => {
                self.move_to_top();
                Ok(TuiEffect::Continue)
            }
            "goto_bottom" => {
                self.move_to_bottom();
                Ok(TuiEffect::Continue)
            }
            "search" => {
                self.mode = Mode::Searching;
                self.status_message = "Search mode".to_string();
                Ok(TuiEffect::Continue)
            }
            "sort_year" => {
                self.apply_sort_choice(SortField::Year);
                Ok(TuiEffect::Continue)
            }
            "sort_author" => {
                self.apply_sort_choice(SortField::Author);
                Ok(TuiEffect::Continue)
            }
            "preview" => {
                self.toggle_preview(params);
                Ok(TuiEffect::Continue)
            }
            "copy" => self.copy_selected_citekey(),
            "pdf" => self.run_entry_action(|handler, entry, config| handler.open_pdf(entry, config)),
            "edit" => match self.selected_entry_id() {
                Some(id) => Ok(TuiEffect::EditEntry(id.clone())),
                None => {
                    self.status_message = "No entry selected".to_string();
                    Ok(TuiEffect::Continue)
                }
            },
            "note" => match self.selected_entry_id() {
                Some(id) => Ok(TuiEffect::OpenNote(id.clone())),
                None => {
                    self.status_message = "No entry selected".to_string();
                    Ok(TuiEffect::Continue)
                }
            },
            _ => Ok(TuiEffect::Continue),
        }
    }

    fn handle_search_mode(&mut self, key: KeyEvent) -> Result<TuiEffect> {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.mode = Mode::Normal;
                self.cursor_position = 0;
                self.status_message = if self.search_query.is_empty() {
                    "Search cleared".to_string()
                } else {
                    format!("{} result(s)", self.filtered.len())
                };
            }
            KeyCode::Backspace => {
                if self.cursor_position > 0 {
                    self.cursor_position -= 1;
                    self.search_query.remove(self.cursor_position);
                    self.refresh_filtered();
                }
            }
            KeyCode::Delete => {
                if self.cursor_position < self.search_query.len() {
                    self.search_query.remove(self.cursor_position);
                    self.refresh_filtered();
                }
            }
            KeyCode::Left => {
                if self.cursor_position > 0 {
                    self.cursor_position -= 1;
                }
            }
            KeyCode::Right => {
                if self.cursor_position < self.search_query.len() {
                    self.cursor_position += 1;
                }
            }
            KeyCode::Home => {
                self.cursor_position = 0;
            }
            KeyCode::End => {
                self.cursor_position = self.search_query.len();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search_query.clear();
                self.cursor_position = 0;
                self.refresh_filtered();
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search_query.insert(self.cursor_position, ch);
                self.cursor_position += 1;
                self.refresh_filtered();
            }
            _ => {}
        }

        Ok(TuiEffect::Continue)
    }

    fn handle_sort_mode(&mut self, key: KeyEvent) -> Result<TuiEffect> {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.status_message = "Sort cancelled".to_string();
            }
            KeyCode::Char('y') => self.apply_sort_choice(SortField::Year),
            KeyCode::Char('a') => self.apply_sort_choice(SortField::Author),
            KeyCode::Char('j') => self.apply_sort_choice(SortField::Journal),
            _ => {}
        }

        Ok(TuiEffect::Continue)
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<TuiEffect> {
        match mouse.kind {
            MouseEventKind::ScrollDown => {
                self.move_down(3);
            }
            MouseEventKind::ScrollUp => {
                self.move_up(3);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                // Calculate which item was clicked based on row position
                let clicked_row = mouse.row as usize;
                // Account for borders (1 row at top)
                if clicked_row > 0 {
                    let clicked_index = clicked_row.saturating_sub(1) + self.scroll_offset;
                    if clicked_index < self.filtered.len() {
                        self.selected = clicked_index;
                        self.ensure_visible();
                    }
                }
            }
            _ => {}
        }
        Ok(TuiEffect::Continue)
    }

    fn apply_sort_choice(&mut self, sort_field: SortField) {
        self.sort_field = Some(sort_field);
        self.mode = Mode::Normal;
        self.refresh_filtered();
        self.status_message = format!("Sorted by {}", sort_field.label());
    }

    fn copy_selected_citekey(&mut self) -> Result<TuiEffect> {
        let Some(id) = self.selected_entry_id().cloned() else {
            self.status_message = "No entry selected".to_string();
            return Ok(TuiEffect::Continue);
        };

        let message = self.action_handler.copy_citekey(&id.0)?;
        self.status_message = message;
        Ok(TuiEffect::Continue)
    }

    fn run_entry_action<F>(&mut self, action: F) -> Result<TuiEffect>
    where
        F: FnOnce(&mut dyn ActionHandler, &Entry, &Config) -> Result<String>,
    {
        let Some(entry) = self.selected_entry().cloned() else {
            self.status_message = "No entry selected".to_string();
            return Ok(TuiEffect::Continue);
        };

        let message = action(self.action_handler.as_mut(), &entry, &self.config)?;
        self.status_message = message;
        Ok(TuiEffect::Continue)
    }

    fn refresh_filtered(&mut self) {
        let selected_before = self.selected_entry_id().cloned();
        let query = Query::parse(&self.search_query);
        self.filtered = self
            .search_engine
            .search(&self.bib, &query)
            .into_iter()
            .map(|result| result.entry_id)
            .collect();

        if let Some(sort_field) = self.sort_field {
            let bib = &self.bib;
            self.filtered
                .sort_by(|left, right| compare_entries(bib, left, right, sort_field));
        }

        self.selected = selected_before
            .and_then(|id| self.filtered.iter().position(|candidate| candidate == &id))
            .unwrap_or(0);

        if self.filtered.is_empty() {
            self.selected = 0;
            self.scroll_offset = 0;
        } else {
            self.selected = self.selected.min(self.filtered.len().saturating_sub(1));
            self.ensure_visible();
        }
    }

    fn active_filters(&self) -> Vec<String> {
        Query::parse(&self.search_query)
            .terms
            .into_iter()
            .filter_map(|term| match term {
                QueryTerm::Field { field, .. } => Some(field),
                QueryTerm::Plain(_) => None,
            })
            .fold(Vec::<String>::new(), |mut filters, field| {
                if !filters.iter().any(|existing| existing == &field) {
                    filters.push(field);
                }
                filters
            })
    }

    fn format_entry_line(&self, entry: &Entry, available_width: usize) -> String {
        let citekey = &entry.id.0;
        let title = entry.title().unwrap_or("Untitled");
        let author = entry.get_field("author").unwrap_or("Unknown");

        let first_author = author.split(" and ").next().unwrap_or(author).trim();

        let note_indicator = if self.note_exists(entry) { "✓" } else { " " };
        let citekey_max = 15;
        let author_max = 20;
        let padding = 4;
        let min_title_width = 10;

        let citekey_display = truncate_with_ellipsis(citekey, citekey_max);
        let author_display = truncate_with_ellipsis(first_author, author_max);

        let used_width = 2 + citekey_max + 1 + author_max + 1 + padding;
        let title_width = available_width.saturating_sub(used_width).max(min_title_width);

        let title_display = truncate_with_ellipsis(title, title_width);

        // Pad manually based on display width for proper Unicode (Chinese) handling
        let citekey_padded = pad_to_display_width(&citekey_display, citekey_max);
        let author_padded = pad_to_display_width(&author_display, author_max);

        format!("{} {} {} {}", note_indicator, citekey_padded, author_padded, title_display)
    }

    fn note_exists(&self, entry: &Entry) -> bool {
        let notes_service = NotesService::new(self.config.notes.clone());
        notes_service.note_path(entry).exists()
    }

    fn placeholder_value(&self, entry: &Entry, field: &str) -> String {
        match field {
            "citekey" => entry.id.0.clone(),
            "author" => entry
                .get_field("author")
                .map(ToOwned::to_owned)
                .filter(|author| !author.is_empty())
                .unwrap_or_else(|| "Unknown author".to_string()),
            "title" => entry
                .title()
                .map(ToOwned::to_owned)
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| "Untitled".to_string()),
            other => entry.get_field(other).unwrap_or_default().to_string(),
        }
    }

    fn selected_style(&self) -> Style {
        let fg = color_from_name(&self.config.theme.selected_fg);
        let bg = color_from_name(&self.config.theme.selected_bg);
        let mut style = Style::default();

        if fg == Color::Reset && bg == Color::Reset {
            style = style.add_modifier(Modifier::REVERSED);
        } else {
            if fg != Color::Reset {
                style = style.fg(fg);
            }
            if bg != Color::Reset {
                style = style.bg(bg);
            }
        }

        style.add_modifier(Modifier::BOLD)
    }

    fn status_line(&self) -> String {
        match self.mode {
            Mode::Normal => {
                let help = if self.show_preview.is_some() {
                    "i next | / search | enter select | e edit | n note | p pdf | y copy | s sort | q quit"
                } else {
                    "i preview | / search | enter select | e edit | n note | p pdf | y copy | s sort | q quit"
                };
                format!("{} | {}", self.status_message, help)
            }
            Mode::Searching => "type to filter | @field: scoped | backspace delete | ctrl+u clear | enter/esc close".to_string(),
            Mode::Sorting => "y year | a author | j journal | esc cancel".to_string(),
        }
    }

    fn mode_label(&self) -> &'static str {
        match self.mode {
            Mode::Normal => "normal",
            Mode::Searching => "search",
            Mode::Sorting => "sort",
        }
    }

    fn visible_entry_ids_with_preview(&self, preview_height: usize) -> &[EntryId] {
        let available_lines = self.list_height.saturating_sub(if self.show_preview.is_some() { preview_height } else { 0 });
        let end = (self.scroll_offset + available_lines).min(self.filtered.len());
        &self.filtered[self.scroll_offset..end]
    }

    fn relative_selected(&self) -> Option<usize> {
        if self.filtered.is_empty() || self.selected < self.scroll_offset {
            return None;
        }

        let relative = self.selected - self.scroll_offset;
        (relative < self.filtered.len() - self.scroll_offset).then_some(relative)
    }

    fn selected_entry_id(&self) -> Option<&EntryId> {
        self.filtered.get(self.selected)
    }

    fn selected_entry(&self) -> Option<&Entry> {
        self.selected_entry_id().and_then(|id| self.bib.get(id))
    }

    fn move_down(&mut self, amount: usize) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = (self.selected + amount).min(self.filtered.len().saturating_sub(1));
        self.ensure_visible();
    }

    fn move_up(&mut self, amount: usize) {
        self.selected = self.selected.saturating_sub(amount);
        self.ensure_visible();
    }

    fn page_down(&mut self) {
        let step = self.list_height.saturating_sub(1).max(1);
        self.move_down(step);
    }

    fn page_up(&mut self) {
        let step = self.list_height.saturating_sub(1).max(1);
        self.move_up(step);
    }

    fn move_to_top(&mut self) {
        self.selected = 0;
        self.ensure_visible();
    }

    fn move_to_bottom(&mut self) {
        if self.filtered.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.filtered.len() - 1;
        }
        self.ensure_visible();
    }

    fn ensure_visible(&mut self) {
        if self.filtered.is_empty() {
            self.scroll_offset = 0;
            return;
        }

        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }

        let window = self.list_height.max(1);
        let bottom = self.scroll_offset + window;
        if self.selected >= bottom {
            self.scroll_offset = self.selected + 1 - window;
        }
    }

    fn ensure_visible_with_context(&mut self, context_lines: usize) {
        if self.filtered.is_empty() {
            self.scroll_offset = 0;
            return;
        }

        let effective_list_height = if self.show_preview.is_some() {
            self.list_height.saturating_sub(8)
        } else {
            self.list_height
        };

        let window = effective_list_height.max(1);

        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else {
            let max_scroll = self.filtered.len().saturating_sub(window).max(0);
            
            if self.selected >= self.scroll_offset + window.saturating_sub(context_lines) {
                let target_offset = self.selected.saturating_sub(window.saturating_sub(context_lines).saturating_sub(1));
                self.scroll_offset = target_offset.min(max_scroll);
            }
            
            if self.selected == self.filtered.len().saturating_sub(1) {
                self.scroll_offset = max_scroll;
            }
        }
    }

    fn toggle_preview(&mut self, params: Option<&str>) {
        let pattern_count = self.config.preview.patterns.len();
        if pattern_count == 0 {
            self.show_preview = None;
            self.status_message = "No preview patterns configured".to_string();
            return;
        }

        match params {
            None => {
                self.show_preview = match self.show_preview {
                    None => Some(0),
                    Some(idx) if idx + 1 < pattern_count => Some(idx + 1),
                    Some(_) => None,
                };
            }
            Some(p) if p.contains(',') => {
                let parts: Vec<&str> = p.split(',').collect();
                if parts.len() == 2 {
                    let start: usize = parts[0].trim().parse().unwrap_or(0);
                    let end: usize = parts[1].trim().parse().unwrap_or(pattern_count - 1);
                    self.show_preview = match self.show_preview {
                        None => Some(start),
                        Some(idx) if idx < end => Some((idx + 1).min(end)),
                        Some(_) => None,
                    };
                }
            }
            Some(p) => {
                if let Ok(idx) = p.trim().parse::<usize>() {
                    if self.show_preview == Some(idx) {
                        self.show_preview = None;
                    } else {
                        self.show_preview = Some(idx.min(pattern_count - 1));
                    }
                }
            }
        }

        self.status_message = match self.show_preview {
            None => "Preview off".to_string(),
            Some(idx) => {
                let name = self.config.preview.patterns[idx]
                    .name
                    .as_deref()
                    .unwrap_or("Preview");
                format!("{} {}/{}", name, idx + 1, pattern_count)
            }
        };
    }

    fn get_keybinding_action(&self, key_str: &str) -> Option<&String> {
        self.config.keybindings.iter()
            .find(|(_, v)| v.as_str() == key_str)
            .map(|(k, _)| k)
    }
}

fn format_preview_content(entry: &Entry) -> String {
    let mut lines = vec![
        format!("Citekey: {}", entry.id.0),
        format!("Type: {}", entry.entry_type),
        String::new(),
    ];

    if let Some(title) = entry.title() {
        lines.push(format!("Title: {}", title));
    }

    if let Some(author) = entry.get_field("author") {
        lines.push(format!("Author(s): {}", author));
    }

    if let Some(year) = entry.year() {
        lines.push(format!("Year: {}", year));
    }

    if let Some(journal) = entry.get_field("journal") {
        lines.push(format!("Journal: {}", journal));
    }

    if let Some(doi) = entry.get_field("doi") {
        lines.push(format!("DOI: {}", doi));
    }

    if let Some(abstract_text) = entry.get_field("abstract") {
        lines.push(String::new());
        lines.push("Abstract:".to_string());
        lines.push(abstract_text.to_string());
    }

    lines.join("\n")
}

fn format_preview_with_template(entry: &Entry, patterns: &[crate::config::PreviewPattern], pattern_idx: usize) -> String {
    if patterns.is_empty() || pattern_idx >= patterns.len() {
        return format_preview_content(entry);
    }
    
    let template = &patterns[pattern_idx].template;
    render_template(template, entry)
}

fn render_template(template: &str, entry: &Entry) -> String {
    let mut result = template.to_string();
    
    let replacements = [
        ("citekey", entry.id.0.clone()),
        ("entry_type", entry.entry_type.clone()),
        ("title", entry.title().unwrap_or("N/A").to_string()),
        ("author", entry.get_field("author").unwrap_or("N/A").to_string()),
        ("year", entry.year().map(|y| y.to_string()).unwrap_or_default()),
        ("journal", entry.get_field("journal").unwrap_or_default().to_string()),
        ("doi", entry.get_field("doi").unwrap_or_default().to_string()),
        ("abstract", entry.get_field("abstract").unwrap_or_default().to_string()),
        ("url", entry.get_field("url").unwrap_or_default().to_string()),
        ("publisher", entry.get_field("publisher").unwrap_or_default().to_string()),
        ("volume", entry.get_field("volume").unwrap_or_default().to_string()),
        ("number", entry.get_field("number").unwrap_or_default().to_string()),
        ("pages", entry.get_field("pages").unwrap_or_default().to_string()),
        ("booktitle", entry.get_field("booktitle").unwrap_or_default().to_string()),
        ("school", entry.get_field("school").unwrap_or_default().to_string()),
        ("institution", entry.get_field("institution").unwrap_or_default().to_string()),
        ("organization", entry.get_field("organization").unwrap_or_default().to_string()),
        ("edition", entry.get_field("edition").unwrap_or_default().to_string()),
        ("series", entry.get_field("series").unwrap_or_default().to_string()),
        ("address", entry.get_field("address").unwrap_or_default().to_string()),
        ("month", entry.get_field("month").unwrap_or_default().to_string()),
        ("note", entry.get_field("note").unwrap_or_default().to_string()),
        ("howpublished", entry.get_field("howpublished").unwrap_or_default().to_string()),
        ("chapter", entry.get_field("chapter").unwrap_or_default().to_string()),
    ];
    
    for (key, value) in &replacements {
        let placeholder = format!("{{{}}}", key);
        let marked_value = format!("\u{001E}{}\u{001F}", value);
        result = result.replace(&placeholder, &marked_value);
    }
    
    result
}

fn key_event_to_binding_name(key: KeyEvent) -> String {
    match key.code {
        KeyCode::Char(ch) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                format!("ctrl+{}", ch.to_ascii_lowercase())
            } else {
                ch.to_string()
            }
        }
        KeyCode::Up => "<Up>".to_string(),
        KeyCode::Down => "<Down>".to_string(),
        KeyCode::Left => "<Left>".to_string(),
        KeyCode::Right => "<Right>".to_string(),
        KeyCode::Enter => "<Enter>".to_string(),
        KeyCode::Esc => "<Esc>".to_string(),
        _ => String::new(),
    }
}

fn parse_action_binding(action: &str) -> (&str, Option<&str>) {
    if let Some(start) = action.find('[') {
        if let Some(end) = action.find(']') {
            return (&action[..start], Some(&action[start + 1..end]));
        }
    }
    (action, None)
}

fn truncate_with_ellipsis(text: &str, max_len: usize) -> String {
    let display_len = display_width(text);
    if display_len <= max_len {
        return text.to_string();
    }

    let ellipsis = "…";
    let ellipsis_width = display_width(ellipsis);
    let available = max_len.saturating_sub(ellipsis_width);

    if available == 0 {
        return ellipsis.to_string();
    }

    // Truncate based on display width, not byte count
    let mut result = String::new();
    let mut current_width = 0;
    for ch in text.chars() {
        let ch_width = char_display_width(ch);
        if current_width + ch_width > available {
            break;
        }
        result.push(ch);
        current_width += ch_width;
    }
    format!("{}{}", result, ellipsis)
}

/// Calculate display width of a string (accounts for wide characters like Chinese)
fn display_width(s: &str) -> usize {
    s.chars().map(char_display_width).sum()
}

/// Calculate display width of a single character
fn char_display_width(c: char) -> usize {
    // CJK characters and other wide characters have width 2
    if c as u32 >= 0x1100 && (c as u32 <= 0x115F
        || c as u32 >= 0x2329 && c as u32 <= 0x232A
        || c as u32 >= 0x2E80 && c as u32 <= 0x303E
        || c as u32 >= 0x3040 && c as u32 <= 0xA4CF
        || c as u32 >= 0xAC00 && c as u32 <= 0xD7A3
        || c as u32 >= 0xF900 && c as u32 <= 0xFAFF
        || c as u32 >= 0xFE10 && c as u32 <= 0xFE19
        || c as u32 >= 0xFE30 && c as u32 <= 0xFE6F
        || c as u32 >= 0xFF00 && c as u32 <= 0xFF60
        || c as u32 >= 0xFFE0 && c as u32 <= 0xFFE6
        || c as u32 >= 0x20000 && c as u32 <= 0x2FFFD
        || c as u32 >= 0x30000 && c as u32 <= 0x3FFFD)
    {
        2
    } else {
        1
    }
}

/// Pad string to target display width (handles Unicode properly)
fn pad_to_display_width(s: &str, target_width: usize) -> String {
    let current_width = display_width(s);
    if current_width >= target_width {
        return s.to_string();
    }
    let padding = target_width - current_width;
    format!("{}{}", s, " ".repeat(padding))
}

impl SortField {
    fn label(self) -> &'static str {
        match self {
            SortField::Year => "year",
            SortField::Author => "author",
            SortField::Journal => "journal",
        }
    }
}

impl ActionHandler for SystemActionHandler {
    fn edit_entry(&mut self, _entry: &Entry, _config: &Config) -> Result<String> {
        bail!("edit_entry is not used; editor launch is handled via TuiEffect in mod.rs")
    }

    fn open_note(&mut self, _entry: &Entry, _config: &Config) -> Result<String> {
        bail!("open_note is not used; editor launch is handled via TuiEffect in mod.rs")
    }

    fn open_pdf(&mut self, entry: &Entry, config: &Config) -> Result<String> {
        let target = pdf_target_for(entry)?;
        open_with_program(&target, config.pdf_reader.as_deref())?;
        Ok(format!("Opened PDF {}", target))
    }

    fn copy_citekey(&mut self, citekey: &str) -> Result<String> {
        copy_to_clipboard(citekey)?;
        Ok(format!("Copied {}", citekey))
    }
}

fn pdf_target_for(entry: &Entry) -> Result<String> {
    for field in ["pdf", "file", "url"] {
        let Some(value) = entry.get_field(field) else {
            continue;
        };

        if field == "url" {
            return Ok(value.to_string());
        }

        for candidate in pdf_candidates(value) {
            let resolved = resolve_target_path(&entry.provenance.file_path, candidate);
            if resolved.exists() || candidate.ends_with(".pdf") {
                return Ok(resolved.to_string_lossy().to_string());
            }
        }
    }

    bail!("No PDF path found for {}", entry.id)
}

fn pdf_candidates(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(';')
        .flat_map(|segment| segment.split(':'))
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
}

fn resolve_target_path(base_file: &Path, target: &str) -> PathBuf {
    let path = PathBuf::from(target);
    if path.is_absolute() {
        path
    } else {
        base_file.parent().unwrap_or_else(|| Path::new(".")).join(path)
    }
}

fn open_with_program(target: &str, program: Option<&str>) -> Result<()> {
    if let Some(program) = program.filter(|program| !program.trim().is_empty()) {
        return spawn_shell_command("exec \"$BIBR_PROGRAM\" \"$BIBR_TARGET\"", &[("BIBR_PROGRAM", program), ("BIBR_TARGET", target)]);
    }

    let opener = ["open", "xdg-open", "gio"]
        .into_iter()
        .find(|candidate| which::which(candidate).is_ok())
        .ok_or_else(|| anyhow!("No system opener found"))?;

    let mut command = Command::new(opener);
    if opener == "gio" {
        command.arg("open");
    }
    command.arg(target).spawn()?;
    Ok(())
}

fn spawn_shell_command(script: &str, envs: &[(&str, &str)]) -> Result<()> {
    let mut command = Command::new("/bin/sh");
    command.arg("-lc").arg(script);
    for (key, value) in envs {
        command.env(key, value);
    }
    command.spawn()?;
    Ok(())
}

fn copy_to_clipboard(text: &str) -> Result<()> {
    for (program, args) in [
        ("pbcopy", Vec::<&str>::new()),
        ("wl-copy", Vec::<&str>::new()),
        ("xclip", vec!["-selection", "clipboard"]),
        ("xsel", vec!["--clipboard", "--input"]),
    ] {
        if which::which(program).is_err() {
            continue;
        }

        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed to start clipboard helper '{program}'"))?;

        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(text.as_bytes())?;
        }

        child.wait()?;
        return Ok(());
    }

    bail!("No supported clipboard helper found")
}

fn compare_year_desc(left: Option<&Entry>, right: Option<&Entry>) -> Ordering {
    let left_year = left.and_then(Entry::year).unwrap_or_default();
    let right_year = right.and_then(Entry::year).unwrap_or_default();
    right_year.cmp(&left_year)
}

fn compare_text(left: Option<&str>, right: Option<&str>) -> Ordering {
    left.unwrap_or_default()
        .to_ascii_lowercase()
        .cmp(&right.unwrap_or_default().to_ascii_lowercase())
}

fn field_value<'a>(entry: Option<&'a Entry>, field: &str) -> Option<&'a str> {
    entry.and_then(|entry| entry.get_field(field))
}

fn compare_entries(
    bib: &Bibliography,
    left: &EntryId,
    right: &EntryId,
    sort_field: SortField,
) -> Ordering {
    let left_entry = bib.get(left);
    let right_entry = bib.get(right);

    match sort_field {
        SortField::Year => compare_year_desc(left_entry, right_entry)
            .then_with(|| compare_text(field_value(left_entry, "author"), field_value(right_entry, "author")))
            .then_with(|| left.0.cmp(&right.0)),
        SortField::Author => compare_text(field_value(left_entry, "author"), field_value(right_entry, "author"))
            .then_with(|| compare_text(field_value(left_entry, "title"), field_value(right_entry, "title")))
            .then_with(|| left.0.cmp(&right.0)),
        SortField::Journal => compare_text(field_value(left_entry, "journal"), field_value(right_entry, "journal"))
            .then_with(|| compare_text(field_value(left_entry, "author"), field_value(right_entry, "author")))
            .then_with(|| left.0.cmp(&right.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{collections::HashMap, sync::{Arc, Mutex}};

    use crossterm::event::KeyEvent;

    use crate::{
        config::{Config, DisplayConfig, NotesConfig, SearchConfig, ThemeConfig},
        domain::{Entry, Provenance},
    };

    #[derive(Clone)]
    struct MockActionHandler {
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl MockActionHandler {
        fn new() -> (Box<dyn ActionHandler>, Arc<Mutex<Vec<String>>>) {
            let calls = Arc::new(Mutex::new(Vec::new()));
            (
                Box::new(Self {
                    calls: calls.clone(),
                }),
                calls,
            )
        }
    }

    impl ActionHandler for MockActionHandler {
        fn edit_entry(&mut self, entry: &Entry, _config: &Config) -> Result<String> {
            self.calls.lock().unwrap().push(format!("edit:{}", entry.id));
            Ok("edited".to_string())
        }

        fn open_note(&mut self, entry: &Entry, _config: &Config) -> Result<String> {
            self.calls.lock().unwrap().push(format!("note:{}", entry.id));
            Ok("noted".to_string())
        }

        fn open_pdf(&mut self, entry: &Entry, _config: &Config) -> Result<String> {
            self.calls.lock().unwrap().push(format!("pdf:{}", entry.id));
            Ok("pdf".to_string())
        }

        fn copy_citekey(&mut self, citekey: &str) -> Result<String> {
            self.calls.lock().unwrap().push(format!("copy:{}", citekey));
            Ok("copied".to_string())
        }
    }

    fn sample_entry(id: &str, author: &str, title: &str, year: &str, journal: &str) -> Entry {
        Entry {
            id: EntryId::from(id),
            entry_type: "article".to_string(),
            fields: HashMap::from([
                ("author".to_string(), author.to_string()),
                ("title".to_string(), title.to_string()),
                ("year".to_string(), year.to_string()),
                ("journal".to_string(), journal.to_string()),
            ]),
            provenance: Provenance {
                file_path: PathBuf::from("library.bib"),
                line_start: 1,
                line_end: 1,
                byte_start: 0,
                byte_end: 0,
            },
        }
    }

    fn sample_app() -> (TuiApp, Arc<Mutex<Vec<String>>>) {
        let mut bib = Bibliography::new();
        bib.add_entry(sample_entry("knuth1984", "Knuth", "Literate Programming", "1984", "The Computer Journal")).unwrap();
        bib.add_entry(sample_entry("hopper1952", "Hopper", "Compiler Design", "1952", "ACM Journal")).unwrap();
        bib.add_entry(sample_entry("turing1936", "Turing", "Computable Numbers", "1936", "Proceedings")).unwrap();

        let (handler, calls) = MockActionHandler::new();
        let config = Config {
            search: SearchConfig::default(),
            display: DisplayConfig {
                format: "{author} - {title} ({citekey})".to_string(),
            },
            theme: ThemeConfig::default(),
            notes: NotesConfig::default(),
            ..Config::default()
        };

        (TuiApp::with_action_handler(bib, config, handler), calls)
    }

    #[test]
    fn search_updates_results_as_user_types() {
        let (mut app, _) = sample_app();

        app.handle_key_event(KeyEvent::from(KeyCode::Char('/'))).unwrap();
        for ch in "compiler".chars() {
            app.handle_key_event(KeyEvent::from(KeyCode::Char(ch))).unwrap();
        }

        assert_eq!(app.mode, Mode::Searching);
        assert_eq!(app.search_query, "compiler");
        assert_eq!(app.filtered, vec![EntryId::from("hopper1952")]);
    }

    #[test]
    fn navigation_supports_vim_and_jump_keys() {
        let (mut app, _) = sample_app();
        app.list_height = 2;

        app.handle_key_event(KeyEvent::from(KeyCode::Char('j'))).unwrap();
        assert_eq!(app.selected_entry_id(), Some(&EntryId::from("knuth1984")));

        app.handle_key_event(KeyEvent::from(KeyCode::Char('G'))).unwrap();
        assert_eq!(app.selected_entry_id(), Some(&EntryId::from("turing1936")));

        app.handle_key_event(KeyEvent::from(KeyCode::Char('g'))).unwrap();
        app.handle_key_event(KeyEvent::from(KeyCode::Char('g'))).unwrap();
        assert_eq!(app.selected_entry_id(), Some(&EntryId::from("hopper1952")));
    }

    #[test]
    fn ctrl_keys_page_through_results() {
        let (mut app, _) = sample_app();
        app.list_height = 2;

        app.handle_key_event(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)).unwrap();
        assert_eq!(app.selected_entry_id(), Some(&EntryId::from("knuth1984")));

        app.handle_key_event(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)).unwrap();
        assert_eq!(app.selected_entry_id(), Some(&EntryId::from("hopper1952")));
    }

    #[test]
    fn sort_mode_reorders_entries() {
        let (mut app, _) = sample_app();

        app.handle_key_event(KeyEvent::from(KeyCode::Char('s'))).unwrap();
        app.handle_key_event(KeyEvent::from(KeyCode::Char('y'))).unwrap();

        assert_eq!(app.sort_field, Some(SortField::Year));
        assert_eq!(app.filtered.first(), Some(&EntryId::from("knuth1984")));
        assert_eq!(app.filtered.last(), Some(&EntryId::from("turing1936")));
    }

    #[test]
    fn actions_delegate_to_handler() {
        let (mut app, calls) = sample_app();

        app.handle_key_event(KeyEvent::from(KeyCode::Char('p'))).unwrap();
        app.handle_key_event(KeyEvent::from(KeyCode::Char('y'))).unwrap();

        assert_eq!(
            calls.lock().unwrap().clone(),
            vec![
                "pdf:hopper1952".to_string(),
                "copy:hopper1952".to_string(),
            ]
        );
    }

    #[test]
    fn edit_and_note_return_effects() {
        let (mut app, _) = sample_app();

        let effect = app.handle_key_event(KeyEvent::from(KeyCode::Char('e'))).unwrap();
        assert!(matches!(effect, TuiEffect::EditEntry(_)));

        let effect = app.handle_key_event(KeyEvent::from(KeyCode::Char('n'))).unwrap();
        assert!(matches!(effect, TuiEffect::OpenNote(_)));
    }

    #[test]
    fn format_uses_display_template() {
        let (app, _) = sample_app();
        let entry = app.bib.get(&EntryId::from("knuth1984")).unwrap();

        let formatted = app.format_entry_line(entry, 100);
        assert!(formatted.contains("knuth1984"));
        assert!(formatted.contains("Knuth"));
        assert!(formatted.contains("Literate"));
        assert!(formatted.starts_with(' ') || formatted.starts_with('✓'));
    }
}
