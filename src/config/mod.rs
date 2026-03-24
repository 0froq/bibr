use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

const DEFAULT_CONFIG: &str = include_str!("default.toml");

pub const UP: &str = "up";
pub const DOWN: &str = "down";
pub const SEARCH: &str = "search";
pub const QUIT: &str = "quit";
pub const EDIT: &str = "edit";
pub const NOTE: &str = "note";
pub const PDF: &str = "pdf";
pub const COPY: &str = "copy";
pub const SORT_YEAR: &str = "sort_year";
pub const SORT_AUTHOR: &str = "sort_author";
pub const PREVIEW: &str = "preview";
pub const PAGE_UP: &str = "page_up";
pub const PAGE_DOWN: &str = "page_down";
pub const GOTO_TOP: &str = "goto_top";
pub const GOTO_BOTTOM: &str = "goto_bottom";

const DEFAULT_FORMAT: &str = "{citekey:<15} {author:<20} {title}";
const VALID_KEYBINDING_ACTIONS: &[&str] = &[
    UP,
    DOWN,
    SEARCH,
    QUIT,
    EDIT,
    NOTE,
    PDF,
    COPY,
    SORT_YEAR,
    SORT_AUTHOR,
    PREVIEW,
    PAGE_UP,
    PAGE_DOWN,
    GOTO_TOP,
    GOTO_BOTTOM,
];
const VALID_THEME_COLORS: &[&str] = &[
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "dark_gray",
    "light_red",
    "light_green",
    "light_yellow",
    "light_blue",
    "light_magenta",
    "light_cyan",
    "gray",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub bibtex_files: Vec<PathBuf>,
    pub search: SearchConfig,
    pub display: DisplayConfig,
    pub preview: PreviewConfig,
    #[serde(
        default = "default_keybindings",
        deserialize_with = "deserialize_keybindings"
    )]
    pub keybindings: HashMap<String, String>,
    pub theme: ThemeConfig,
    #[serde(default = "default_editor")]
    pub editor: Option<String>,
    pub notes: NotesConfig,
    pub pdf_reader: Option<String>,
    #[serde(default)]
    pub mouse_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SearchConfig {
    pub smart_case: bool,
    pub fuzzy: bool,
    pub search_all_fields: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DisplayConfig {
    pub format: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewPattern {
    pub name: Option<String>,
    pub template: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PreviewConfig {
    pub patterns: Vec<PreviewPattern>,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            patterns: vec![PreviewPattern {
                name: Some("Default".to_string()),
                template: concat!(
                    "Citekey: {citekey}\n",
                    "Type: {entry_type}\n",
                    "\n",
                    "Title: {title}\n",
                    "Author(s): {author}\n",
                    "Year: {year}\n",
                    "Journal: {journal}\n",
                    "DOI: {doi}\n",
                    "\n",
                    "Abstract:\n",
                    "{abstract}"
                )
                .to_string(),
            }],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeConfig {
    pub selected_fg: String,
    pub selected_bg: String,
    pub highlight_fg: String,
    pub status_bg: String,
    pub status_fg: String,
    pub status_search_bg: String,
    pub status_search_fg: String,
    pub help_key_fg: String,
    pub help_key_bg: String,
    pub help_desc_fg: String,
    pub help_desc_bg: String,
    pub entry_normal_fg: String,
    pub entry_normal_bg: String,
    pub entry_selected_fg: String,
    pub entry_selected_bg: String,
    pub entry_focused_fg: String,
    pub entry_focused_bg: String,
    pub list_title_fg: String,
    pub list_title_bg: String,
    pub list_border_fg: String,
    pub list_border_bg: String,
    pub cursor_bg: String,
    pub search_match_fg: String,
    pub search_match_bg: String,
    pub preview_label_fg: String,
    pub preview_label_bg: String,
    pub preview_value_fg: String,
    pub preview_value_bg: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NotesConfig {
    #[serde(default = "default_notes_dir", alias = "dir")]
    pub notes_dir: PathBuf,
    pub filename_pattern: String,
    pub template_file: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bibtex_files: Vec::new(),
            search: SearchConfig::default(),
            display: DisplayConfig::default(),
            preview: PreviewConfig::default(),
            keybindings: default_keybindings(),
            theme: ThemeConfig::default(),
            editor: default_editor(),
            notes: NotesConfig::default(),
            pdf_reader: None,
            mouse_enabled: false,
        }
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            smart_case: true,
            fuzzy: true,
            search_all_fields: true,
        }
    }
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            format: DEFAULT_FORMAT.to_string(),
        }
    }
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            selected_fg: "white".to_string(),
            selected_bg: "blue".to_string(),
            highlight_fg: "yellow".to_string(),
            status_bg: "dark_gray".to_string(),
            status_fg: "white".to_string(),
            status_search_bg: "dark_gray".to_string(),
            status_search_fg: "white".to_string(),
            help_key_fg: "yellow".to_string(),
            help_key_bg: "dark_gray".to_string(),
            help_desc_fg: "white".to_string(),
            help_desc_bg: "dark_gray".to_string(),
            entry_normal_fg: "white".to_string(),
            entry_normal_bg: "black".to_string(),
            entry_selected_fg: "white".to_string(),
            entry_selected_bg: "dark_gray".to_string(),
            entry_focused_fg: "black".to_string(),
            entry_focused_bg: "cyan".to_string(),
            list_title_fg: "yellow".to_string(),
            list_title_bg: "black".to_string(),
            list_border_fg: "gray".to_string(),
            list_border_bg: "black".to_string(),
            cursor_bg: "white".to_string(),
            search_match_fg: "black".to_string(),
            search_match_bg: "yellow".to_string(),
            preview_label_fg: "cyan".to_string(),
            preview_label_bg: "black".to_string(),
            preview_value_fg: "white".to_string(),
            preview_value_bg: "black".to_string(),
        }
    }
}

impl Default for NotesConfig {
    fn default() -> Self {
        Self {
            notes_dir: default_notes_dir(),
            filename_pattern: "{citekey}.md".to_string(),
            template_file: None,
        }
    }
}

/// Partial config for deep merging - all fields are Optional to allow partial overrides
#[derive(Debug, Default, Deserialize)]
struct PartialConfig {
    bibtex_files: Option<Vec<PathBuf>>,
    search: Option<PartialSearchConfig>,
    display: Option<PartialDisplayConfig>,
    preview: Option<PartialPreviewConfig>,
    keybindings: Option<HashMap<String, String>>,
    theme: Option<PartialThemeConfig>,
    editor: Option<Option<String>>,
    notes: Option<PartialNotesConfig>,
    pdf_reader: Option<Option<String>>,
    mouse_enabled: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct PartialSearchConfig {
    smart_case: Option<bool>,
    fuzzy: Option<bool>,
    search_all_fields: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct PartialDisplayConfig {
    format: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct PartialPreviewConfig {
    patterns: Option<Vec<PreviewPattern>>,
}

#[derive(Debug, Default, Deserialize)]
struct PartialThemeConfig {
    selected_fg: Option<String>,
    selected_bg: Option<String>,
    highlight_fg: Option<String>,
    status_bg: Option<String>,
    status_fg: Option<String>,
    status_search_bg: Option<String>,
    status_search_fg: Option<String>,
    help_key_fg: Option<String>,
    help_key_bg: Option<String>,
    help_desc_fg: Option<String>,
    help_desc_bg: Option<String>,
    entry_normal_fg: Option<String>,
    entry_normal_bg: Option<String>,
    entry_selected_fg: Option<String>,
    entry_selected_bg: Option<String>,
    entry_focused_fg: Option<String>,
    entry_focused_bg: Option<String>,
    list_title_fg: Option<String>,
    list_title_bg: Option<String>,
    list_border_fg: Option<String>,
    list_border_bg: Option<String>,
    cursor_bg: Option<String>,
    search_match_fg: Option<String>,
    search_match_bg: Option<String>,
    preview_label_fg: Option<String>,
    preview_label_bg: Option<String>,
    preview_value_fg: Option<String>,
    preview_value_bg: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct PartialNotesConfig {
    dir: Option<PathBuf>,
    notes_dir: Option<PathBuf>,
    filename_pattern: Option<String>,
    template_file: Option<Option<PathBuf>>,
}

impl Config {
    fn merge_partial(&mut self, partial: PartialConfig) {
        if let Some(files) = partial.bibtex_files {
            self.bibtex_files = files;
        }
        if let Some(editor) = partial.editor {
            self.editor = editor;
        }
        if let Some(pdf_reader) = partial.pdf_reader {
            self.pdf_reader = pdf_reader;
        }
        if let Some(mouse) = partial.mouse_enabled {
            self.mouse_enabled = mouse;
        }
        if let Some(search) = partial.search {
            self.search.merge_partial(search);
        }
        if let Some(display) = partial.display {
            self.display.merge_partial(display);
        }
        if let Some(preview) = partial.preview {
            self.preview.merge_partial(preview);
        }
        if let Some(theme) = partial.theme {
            self.theme.merge_partial(theme);
        }
        if let Some(notes) = partial.notes {
            self.notes.merge_partial(notes);
        }
        if let Some(keybindings) = partial.keybindings {
            self.keybindings.extend(keybindings);
        }
    }
}

impl SearchConfig {
    fn merge_partial(&mut self, partial: PartialSearchConfig) {
        if let Some(v) = partial.smart_case {
            self.smart_case = v;
        }
        if let Some(v) = partial.fuzzy {
            self.fuzzy = v;
        }
        if let Some(v) = partial.search_all_fields {
            self.search_all_fields = v;
        }
    }
}

impl DisplayConfig {
    fn merge_partial(&mut self, partial: PartialDisplayConfig) {
        if let Some(v) = partial.format {
            self.format = v;
        }
    }
}

impl PreviewConfig {
    fn merge_partial(&mut self, partial: PartialPreviewConfig) {
        if let Some(v) = partial.patterns {
            self.patterns = v;
        }
    }
}

impl ThemeConfig {
    fn merge_partial(&mut self, partial: PartialThemeConfig) {
        macro_rules! merge_field {
            ($field:ident) => {
                if let Some(v) = partial.$field {
                    self.$field = v;
                }
            };
        }
        merge_field!(selected_fg);
        merge_field!(selected_bg);
        merge_field!(highlight_fg);
        merge_field!(status_bg);
        merge_field!(status_fg);
        merge_field!(status_search_bg);
        merge_field!(status_search_fg);
        merge_field!(help_key_fg);
        merge_field!(help_key_bg);
        merge_field!(help_desc_fg);
        merge_field!(help_desc_bg);
        merge_field!(entry_normal_fg);
        merge_field!(entry_normal_bg);
        merge_field!(entry_selected_fg);
        merge_field!(entry_selected_bg);
        merge_field!(entry_focused_fg);
        merge_field!(entry_focused_bg);
        merge_field!(list_title_fg);
        merge_field!(list_title_bg);
        merge_field!(list_border_fg);
        merge_field!(list_border_bg);
        merge_field!(cursor_bg);
        merge_field!(search_match_fg);
        merge_field!(search_match_bg);
        merge_field!(preview_label_fg);
        merge_field!(preview_label_bg);
        merge_field!(preview_value_fg);
        merge_field!(preview_value_bg);
    }
}

impl NotesConfig {
    fn merge_partial(&mut self, partial: PartialNotesConfig) {
        if let Some(v) = partial.notes_dir.or(partial.dir) {
            self.notes_dir = v;
        }
        if let Some(v) = partial.filename_pattern {
            self.filename_pattern = v;
        }
        if let Some(v) = partial.template_file {
            self.template_file = v;
        }
    }
}

pub fn load(path: Option<PathBuf>) -> Result<Config> {
    let explicit_path = path.is_some();
    let config_path = match path {
        Some(path) => path,
        None => default_config_path()?,
    };

    let config_exists = config_path.exists();

    // Start with embedded default config
    let mut config: Config =
        toml::from_str(DEFAULT_CONFIG).context("Failed to parse embedded default config")?;

    // Apply user config overrides if file exists
    if config_exists {
        let content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config file '{}'", config_path.display()))?;

        let partial: PartialConfig = toml::from_str(&content).with_context(|| {
            format!(
                "Failed to parse TOML config at '{}'. For parameterized keybindings, quote the key name, e.g. \"preview[0]\" = \"1\", \"preview[1,2]\" = \"2\"",
                config_path.display()
            )
        })?;

        config.merge_partial(partial);

        if let Some(base_dir) = config_path.parent() {
            config.resolve_relative_paths(base_dir);
        }
    } else if explicit_path {
        bail!("Config file '{}' does not exist", config_path.display());
    }

    config.ensure_notes_dir()?;
    config.validate()?;

    Ok(config)
}

impl Config {
    fn resolve_relative_paths(&mut self, base_dir: &Path) {
        for path in &mut self.bibtex_files {
            resolve_relative_path(base_dir, path);
        }

        resolve_relative_path(base_dir, &mut self.notes.notes_dir);

        if let Some(template_file) = &mut self.notes.template_file {
            resolve_relative_path(base_dir, template_file);
        }
    }

    fn ensure_notes_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.notes.notes_dir).with_context(|| {
            format!(
                "Failed to create notes directory '{}'",
                self.notes.notes_dir.display()
            )
        })
    }

    fn validate(&self) -> Result<()> {
        for action in self.keybindings.keys() {
            let (base_action, params) = parse_action_with_params(action);
            if !VALID_KEYBINDING_ACTIONS.contains(&base_action.as_str()) {
                bail!("Unknown keybinding action '{action}' in config");
            }
            if base_action == PREVIEW {
                validate_preview_params(action, params, self.preview.patterns.len())?;
            }
        }

        validate_theme_color("theme.selected_fg", &self.theme.selected_fg)?;
        validate_theme_color("theme.selected_bg", &self.theme.selected_bg)?;
        validate_theme_color("theme.highlight_fg", &self.theme.highlight_fg)?;
        validate_theme_color("theme.status_bg", &self.theme.status_bg)?;
        validate_theme_color("theme.status_fg", &self.theme.status_fg)?;
        validate_theme_color("theme.status_search_bg", &self.theme.status_search_bg)?;
        validate_theme_color("theme.status_search_fg", &self.theme.status_search_fg)?;
        validate_theme_color("theme.help_key_fg", &self.theme.help_key_fg)?;
        validate_theme_color("theme.help_key_bg", &self.theme.help_key_bg)?;
        validate_theme_color("theme.help_desc_fg", &self.theme.help_desc_fg)?;
        validate_theme_color("theme.help_desc_bg", &self.theme.help_desc_bg)?;
        validate_theme_color("theme.entry_normal_fg", &self.theme.entry_normal_fg)?;
        validate_theme_color("theme.entry_normal_bg", &self.theme.entry_normal_bg)?;
        validate_theme_color("theme.entry_selected_fg", &self.theme.entry_selected_fg)?;
        validate_theme_color("theme.entry_selected_bg", &self.theme.entry_selected_bg)?;
        validate_theme_color("theme.entry_focused_fg", &self.theme.entry_focused_fg)?;
        validate_theme_color("theme.entry_focused_bg", &self.theme.entry_focused_bg)?;
        validate_theme_color("theme.list_title_fg", &self.theme.list_title_fg)?;
        validate_theme_color("theme.list_title_bg", &self.theme.list_title_bg)?;
        validate_theme_color("theme.list_border_fg", &self.theme.list_border_fg)?;
        validate_theme_color("theme.list_border_bg", &self.theme.list_border_bg)?;
        validate_theme_color("theme.cursor_bg", &self.theme.cursor_bg)?;
        validate_theme_color("theme.search_match_fg", &self.theme.search_match_fg)?;
        validate_theme_color("theme.search_match_bg", &self.theme.search_match_bg)?;
        validate_theme_color("theme.preview_label_fg", &self.theme.preview_label_fg)?;
        validate_theme_color("theme.preview_label_bg", &self.theme.preview_label_bg)?;
        validate_theme_color("theme.preview_value_fg", &self.theme.preview_value_fg)?;
        validate_theme_color("theme.preview_value_bg", &self.theme.preview_value_bg)?;

        for path in &self.bibtex_files {
            if !path.exists() {
                bail!("BibTeX file '{}' does not exist", path.display());
            }

            if !path.is_file() {
                bail!("BibTeX path '{}' is not a file", path.display());
            }

            match path.extension().and_then(|ext| ext.to_str()) {
                Some("bib") => {}
                _ => bail!(
                    "BibTeX file '{}' must have a .bib extension",
                    path.display()
                ),
            }
        }

        if !self.notes.notes_dir.exists() {
            bail!(
                "Notes directory '{}' does not exist",
                self.notes.notes_dir.display()
            );
        }

        if !self.notes.notes_dir.is_dir() {
            bail!(
                "Notes path '{}' is not a directory",
                self.notes.notes_dir.display()
            );
        }

        if let Some(template_file) = &self.notes.template_file {
            if !template_file.exists() {
                bail!("Template file '{}' does not exist", template_file.display());
            }

            if !template_file.is_file() {
                bail!("Template path '{}' is not a file", template_file.display());
            }
        }

        Ok(())
    }
}

fn default_keybindings() -> HashMap<String, String> {
    HashMap::from([
        (UP.to_string(), "k".to_string()),
        (DOWN.to_string(), "j".to_string()),
        (SEARCH.to_string(), "/".to_string()),
        (QUIT.to_string(), "q".to_string()),
        (EDIT.to_string(), "e".to_string()),
        (NOTE.to_string(), "n".to_string()),
        (PDF.to_string(), "p".to_string()),
        (COPY.to_string(), "y".to_string()),
        (PREVIEW.to_string(), "i".to_string()),
        (PAGE_UP.to_string(), "ctrl+u".to_string()),
        (PAGE_DOWN.to_string(), "ctrl+d".to_string()),
        (GOTO_TOP.to_string(), "gg".to_string()),
        (GOTO_BOTTOM.to_string(), "G".to_string()),
    ])
}

fn deserialize_keybindings<'de, D>(
    deserializer: D,
) -> std::result::Result<HashMap<String, String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let overrides = Option::<HashMap<String, String>>::deserialize(deserializer)?;
    let mut keybindings = default_keybindings();

    if let Some(overrides) = overrides {
        keybindings.extend(overrides);
    }

    Ok(keybindings)
}

fn default_editor() -> Option<String> {
    env::var("EDITOR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn default_notes_dir() -> PathBuf {
    project_dirs()
        .map(|dirs| dirs.data_local_dir().join("notes"))
        .unwrap_or_else(|_| PathBuf::from(".local/share/bibr/notes"))
}

fn default_config_path() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME environment variable not set")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("bibr")
        .join("bibr.toml"))
}

fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("", "", "bibr").context("Failed to determine BIBR config directory")
}

fn resolve_relative_path(base_dir: &Path, path: &mut PathBuf) {
    *path = expand_tilde(path);

    if path.is_relative() {
        *path = base_dir.join(&*path);
    }
}

fn expand_tilde(path: &Path) -> PathBuf {
    let path_str = match path.to_str() {
        Some(path_str) => path_str,
        None => return path.to_path_buf(),
    };

    if path_str == "~" {
        return home_dir().unwrap_or_else(|| path.to_path_buf());
    }

    if let Some(stripped) = path_str.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(stripped);
        }
    }

    path.to_path_buf()
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn validate_theme_color(field: &str, color: &str) -> Result<()> {
    if VALID_THEME_COLORS.contains(&color) {
        return Ok(());
    }

    if is_hex_color(color) {
        return Ok(());
    }

    bail!(
        "Invalid color '{}' for {}. Expected one of: {} or a hex color like #RRGGBB",
        color,
        field,
        VALID_THEME_COLORS.join(", ")
    )
}

fn is_hex_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color
            .as_bytes()
            .iter()
            .skip(1)
            .all(|byte| byte.is_ascii_hexdigit())
}

fn parse_action_with_params(action: &str) -> (String, Option<&str>) {
    if let Some(start) = action.find('[') {
        if let Some(end) = action.find(']') {
            let base = &action[..start];
            let params = &action[start + 1..end];
            return (base.to_string(), Some(params));
        }
    }
    (action.to_string(), None)
}

fn validate_preview_params(action: &str, params: Option<&str>, pattern_count: usize) -> Result<()> {
    let params = match params {
        None => return Ok(()),
        Some(p) if p.is_empty() => return Ok(()),
        Some(p) => p,
    };

    if params.contains(',') {
        let parts: Vec<&str> = params.split(',').collect();
        if parts.len() != 2 {
            bail!("Invalid preview range '{action}'. Expected format: preview[start,end]");
        }
        let start: usize = parts[0]
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid preview start index in '{action}'"))?;
        let end: usize = parts[1]
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid preview end index in '{action}'"))?;
        if start >= pattern_count {
            bail!("Preview start index {start} out of bounds (max: {pattern_count})");
        }
        if end >= pattern_count {
            bail!("Preview end index {end} out of bounds (max: {pattern_count})");
        }
        if start > end {
            bail!("Preview range start ({start}) must be <= end ({end})");
        }
    } else {
        let idx: usize = params
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid preview index in '{action}'"))?;
        if idx >= pattern_count {
            bail!("Preview index {idx} out of bounds (max: {pattern_count})");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn default_config_creation() {
        let config = Config::default();

        assert!(config.search.smart_case);
        assert!(config.search.fuzzy);
        assert!(config.search.search_all_fields);
        assert_eq!(config.display.format, DEFAULT_FORMAT);
        assert_eq!(config.keybindings.get(UP).map(String::as_str), Some("k"));
        assert_eq!(config.keybindings.get(DOWN).map(String::as_str), Some("j"));
        assert_eq!(config.notes.filename_pattern, "{citekey}.md");
        assert_eq!(config.notes.notes_dir, default_notes_dir());

        let toml = toml::to_string(&config).expect("default config should serialize");
        let roundtrip: Config =
            toml::from_str(&toml).expect("serialized config should deserialize");

        assert_eq!(roundtrip, config);
    }

    #[test]
    fn loading_from_toml_string() {
        let temp = tempdir().expect("tempdir should exist");
        let bib_file = temp.path().join("library.bib");
        let notes_dir = temp.path().join("notes");
        let template_file = temp.path().join("template.md");

        fs::write(&bib_file, "% test bib").expect("bib file should be written");
        fs::create_dir(&notes_dir).expect("notes dir should exist");
        fs::write(&template_file, "# Note").expect("template file should be written");

        let input = format!(
            r#"
bibtex_files = ["{}"]
editor = "nvim"
pdf_reader = "zathura"

[search]
smart_case = false
fuzzy = false
search_all_fields = true

[display]
format = "{{title}} - {{citekey}}"

[keybindings]
up = "K"
down = "J"
sort_year = "Y"
sort_author = "A"

[theme]
selected_fg = "black"
selected_bg = "green"
highlight_fg = "red"

[notes]
notes_dir = "{}"
filename_pattern = "notes-{{citekey}}.md"
template_file = "{}"
"#,
            bib_file.display(),
            notes_dir.display(),
            template_file.display(),
        );

        let config: Config = toml::from_str(&input).expect("config should deserialize");
        config.validate().expect("config should validate");

        assert_eq!(config.bibtex_files, vec![bib_file]);
        assert!(!config.search.smart_case);
        assert!(!config.search.fuzzy);
        assert_eq!(config.display.format, "{title} - {citekey}");
        assert_eq!(config.keybindings.get(UP).map(String::as_str), Some("K"));
        assert_eq!(
            config.keybindings.get(SORT_YEAR).map(String::as_str),
            Some("Y")
        );
        assert_eq!(
            config.keybindings.get(SORT_AUTHOR).map(String::as_str),
            Some("A")
        );
        assert_eq!(config.theme.selected_bg, "green");
        assert_eq!(config.editor.as_deref(), Some("nvim"));
        assert_eq!(config.pdf_reader.as_deref(), Some("zathura"));
        assert_eq!(config.notes.filename_pattern, "notes-{citekey}.md");
        assert_eq!(config.notes.template_file, Some(template_file));
    }

    #[test]
    fn validation_of_invalid_paths() {
        let temp = tempdir().expect("tempdir should exist");
        let missing_bib = temp.path().join("missing.bib");
        let notes_dir = temp.path().join("notes");

        fs::create_dir(&notes_dir).expect("notes dir should exist");

        let config = Config {
            bibtex_files: vec![missing_bib.clone()],
            notes: NotesConfig {
                notes_dir,
                ..NotesConfig::default()
            },
            ..Config::default()
        };

        let error = config
            .validate()
            .expect_err("missing bib file should fail validation");
        assert!(
            error
                .to_string()
                .contains(missing_bib.to_string_lossy().as_ref()),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn keybinding_parsing() {
        let input = r#"
[keybindings]
up = "<Up>"
down = "<Down>"
quit = "ZZ"
sort_year = "Y"
sort_author = "A"
"#;

        let config: Config = toml::from_str(input).expect("config should deserialize");

        assert_eq!(config.keybindings.get(UP).map(String::as_str), Some("<Up>"));
        assert_eq!(
            config.keybindings.get(DOWN).map(String::as_str),
            Some("<Down>")
        );
        assert_eq!(config.keybindings.get(QUIT).map(String::as_str), Some("ZZ"));
        assert_eq!(
            config.keybindings.get(SORT_YEAR).map(String::as_str),
            Some("Y")
        );
        assert_eq!(
            config.keybindings.get(SORT_AUTHOR).map(String::as_str),
            Some("A")
        );
    }

    #[test]
    fn parameterized_preview_keybindings_require_quoted_toml_keys() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let notes_dir = temp.path().join("notes");
        fs::create_dir(&notes_dir).expect("notes dir should exist");

        let input = format!(
            r#"
bibtex_files = []

[preview]
patterns = [
  {{ name = "A", template = "Title: {{title}}" }},
  {{ name = "B", template = "Abstract: {{abstract}}" }},
  {{ name = "C", template = "Year: {{year}}" }}
]

[notes]
notes_dir = "{}"

[keybindings]
preview = "i"
"preview[0]" = "1"
"preview[1,2]" = "2"
"#,
            notes_dir.display()
        );

        let config: Config =
            toml::from_str(&input).expect("config with quoted preview keys should deserialize");
        config
            .validate()
            .expect("config with in-range preview params should validate");
        assert_eq!(
            config.keybindings.get("preview[0]").map(String::as_str),
            Some("1")
        );
        assert_eq!(
            config.keybindings.get("preview[1,2]").map(String::as_str),
            Some("2")
        );
    }

    #[test]
    fn theme_color_validation_accepts_hex_rgb() {
        validate_theme_color("theme.selected_bg", "#1A2B3C")
            .expect("#RRGGBB theme colors should validate");
    }

    #[test]
    fn theme_color_validation_rejects_invalid_hex() {
        let error = validate_theme_color("theme.selected_bg", "#12GG99")
            .expect_err("invalid hex should fail validation");
        assert!(
            error.to_string().contains("or a hex color like #RRGGBB"),
            "unexpected error: {error}"
        );
    }
}
