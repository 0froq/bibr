use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

use crate::config::ThemeConfig;

#[derive(Debug, Clone)]
pub struct EntryListView<'a> {
    pub items: &'a [String],
    pub selected: Option<usize>,
    pub total_items: usize,
    pub scroll_offset: usize,
    pub theme: &'a ThemeConfig,
    pub search_query: &'a str,
    pub show_preview: Option<usize>,
    pub preview_content: Option<&'a str>,
    pub preview_height: u16,
}

#[derive(Debug, Clone)]
pub struct StatusBarView<'a> {
    pub text: &'a str,
    pub is_search_mode: bool,
    pub cursor_position: Option<usize>,
    pub theme: &'a ThemeConfig,
}

#[derive(Debug, Clone)]
pub struct PreviewView<'a> {
    pub content: &'a str,
    pub title: &'a str,
    pub theme: &'a ThemeConfig,
}

pub fn color_from_name(name: &str) -> Color {
    match name {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "dark_gray" => Color::DarkGray,
        "light_red" => Color::LightRed,
        "light_green" => Color::LightGreen,
        "light_yellow" => Color::LightYellow,
        "light_blue" => Color::LightBlue,
        "light_magenta" => Color::LightMagenta,
        "light_cyan" => Color::LightCyan,
        "gray" => Color::Gray,
        _ => Color::Reset,
    }
}

pub fn style_from_config(fg: &str, bg: &str) -> Style {
    Style::default()
        .fg(color_from_name(fg))
        .bg(color_from_name(bg))
}

pub fn render_entry_list(frame: &mut Frame, area: Rect, view: EntryListView<'_>) {
    let title_fg = color_from_name(&view.theme.list_title_fg);
    let title_bg = color_from_name(&view.theme.list_title_bg);
    let border_fg = color_from_name(&view.theme.list_border_fg);
    let border_bg = color_from_name(&view.theme.list_border_bg);
    let normal_fg = color_from_name(&view.theme.entry_normal_fg);
    let normal_bg = color_from_name(&view.theme.entry_normal_bg);
    let selected_fg = color_from_name(&view.theme.entry_selected_fg);
    let selected_bg = color_from_name(&view.theme.entry_selected_bg);
    let search_match_fg = color_from_name(&view.theme.search_match_fg);
    let search_match_bg = color_from_name(&view.theme.search_match_bg);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            "Entries ({}/{})",
            view.selected.map(|s| view.scroll_offset + s + 1).unwrap_or(0),
            view.total_items
        ))
        .title_style(Style::default().fg(title_fg).bg(title_bg).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(border_fg).bg(border_bg));

    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    let content_area = Rect::new(
        inner_area.x + 1,
        inner_area.y,
        inner_area.width.saturating_sub(2),
        inner_area.height,
    );

    let preview_height = if view.show_preview.is_some() { view.preview_height } else { 0 };
    let available_height = inner_area.height.saturating_sub(preview_height);
    let max_items = (available_height as usize).min(view.items.len());

    let mut y_offset = inner_area.y;
    let mut rendered_preview = false;

    for (i, item) in view.items.iter().enumerate().take(max_items) {
        let is_selected = view.selected == Some(i);
        let row_area = Rect::new(content_area.x, y_offset, content_area.width, 1);

        let line = if !view.search_query.is_empty() && !is_selected {
            highlight_search_matches(item, view.search_query, normal_fg, normal_bg, search_match_fg, search_match_bg)
        } else {
            Line::from(item.as_str())
        };

        let style = if is_selected {
            Style::default().fg(selected_fg).bg(selected_bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(normal_fg).bg(normal_bg)
        };

        let paragraph = Paragraph::new(line).style(style);
        frame.render_widget(paragraph, row_area);

        y_offset += 1;

        if is_selected && view.show_preview.is_some() && !rendered_preview {
            if let Some(content) = view.preview_content {
                let preview_area = Rect::new(content_area.x, y_offset, content_area.width.saturating_sub(2), preview_height);
                if preview_area.height > 0 {
                    render_inline_preview(frame, preview_area, content, view.theme);
                    y_offset += preview_height;
                    rendered_preview = true;
                }
            }
        }
    }

    if view.total_items > 0 {
        let scrollbar_area = Rect::new(
            area.x + area.width - 1,
            inner_area.y,
            1,
            inner_area.height,
        );
        let mut scrollbar_state = ScrollbarState::new(view.total_items)
            .position(view.scroll_offset)
            .viewport_content_length(view.items.len().min(inner_area.height as usize));
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .track_style(Style::default().fg(Color::DarkGray))
            .thumb_style(Style::default().fg(Color::Gray))
            .begin_symbol(None)
            .end_symbol(None);
        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

fn highlight_search_matches<'a>(
    text: &'a str,
    query: &str,
    normal_fg: Color,
    normal_bg: Color,
    match_fg: Color,
    match_bg: Color,
) -> Line<'a> {
    let mut spans = Vec::new();
    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();

    let mut last_end = 0;
    for (start, end) in find_all_matches(&text_lower, &query_lower) {
        if start > last_end {
            spans.push(Span::styled(
                &text[last_end..start],
                Style::default().fg(normal_fg).bg(normal_bg),
            ));
        }
        spans.push(Span::styled(
            &text[start..end],
            Style::default().fg(match_fg).bg(match_bg).add_modifier(Modifier::BOLD),
        ));
        last_end = end;
    }

    if last_end < text.len() {
        spans.push(Span::styled(
            &text[last_end..],
            Style::default().fg(normal_fg).bg(normal_bg),
        ));
    }

    Line::from(spans)
}

fn find_all_matches(text: &str, query: &str) -> Vec<(usize, usize)> {
    let mut matches = Vec::new();
    let query_len = query.len();

    if query_len == 0 {
        return matches;
    }

    let mut start = 0;
    while let Some(pos) = text[start..].find(query) {
        let match_start = start + pos;
        matches.push((match_start, match_start + query_len));
        start = match_start + query_len;
    }

    matches
}

fn render_inline_preview(frame: &mut Frame, area: Rect, content: &str, theme: &ThemeConfig) {
    let border_fg = Color::DarkGray;
    let label_fg = color_from_name(&theme.preview_label_fg);
    let label_bg = color_from_name(&theme.preview_label_bg);
    let value_fg = color_from_name(&theme.preview_value_fg);
    let value_bg = color_from_name(&theme.preview_value_bg);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_fg));

    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    if inner_area.height == 0 || inner_area.width == 0 {
        return;
    }

    let content_lines: Vec<&str> = content.lines().collect();
    let mut styled_lines: Vec<Line> = Vec::new();

    for line in content_lines.iter().take(inner_area.height as usize) {
        let mut spans: Vec<Span> = Vec::new();
        let mut buffer = String::new();
        let mut in_value = false;

        for ch in line.chars() {
            if ch == '\u{001E}' || ch == '\u{001F}' {
                if !buffer.is_empty() {
                    let style = if in_value {
                        Style::default().fg(value_fg).bg(value_bg)
                    } else {
                        Style::default().fg(label_fg).bg(label_bg)
                    };
                    spans.push(Span::styled(buffer.clone(), style));
                    buffer.clear();
                }
                in_value = ch == '\u{001E}';
            } else {
                buffer.push(ch);
            }
        }

        if !buffer.is_empty() {
            let style = if in_value {
                Style::default().fg(value_fg).bg(value_bg)
            } else {
                Style::default().fg(label_fg).bg(label_bg)
            };
            spans.push(Span::styled(buffer, style));
        }

        styled_lines.push(Line::from(spans));
    }

    let paragraph = Paragraph::new(styled_lines)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, inner_area);
}

pub fn render_status_bar(frame: &mut Frame, area: Rect, view: StatusBarView<'_>) {
    if view.is_search_mode {
        let bg = color_from_name(&view.theme.status_search_bg);
        let fg = color_from_name(&view.theme.status_search_fg);
        let cursor_bg = color_from_name(&view.theme.cursor_bg);

        let mut spans = vec![
            Span::styled("/", Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
        ];

        if view.text.is_empty() {
            spans.push(Span::styled(
                "search...",
                Style::default().fg(Color::Gray).bg(bg),
            ));
        } else {
            if let Some(cursor_pos) = view.cursor_position {
                let before = &view.text[..cursor_pos];
                let at_cursor = view.text.chars().nth(cursor_pos);
                let after = &view.text[cursor_pos + at_cursor.map(|c| c.len_utf8()).unwrap_or(0)..];

                if !before.is_empty() {
                    spans.push(Span::styled(before, Style::default().fg(fg).bg(bg)));
                }

                if let Some(ch) = at_cursor {
                    spans.push(Span::styled(
                        ch.to_string(),
                        Style::default().fg(bg).bg(cursor_bg).add_modifier(Modifier::BOLD),
                    ));
                } else {
                    spans.push(Span::styled(
                        " ",
                        Style::default().bg(cursor_bg),
                    ));
                }

                if !after.is_empty() {
                    spans.push(Span::styled(after, Style::default().fg(fg).bg(bg)));
                }
            } else {
                spans.push(Span::styled(view.text, Style::default().fg(fg).bg(bg)));
            }
        }

        let widget = Paragraph::new(Line::from(spans))
            .style(Style::default().bg(bg));
        frame.render_widget(widget, area);
    } else {
        let bg = color_from_name(&view.theme.status_bg);
        let fg = color_from_name(&view.theme.status_fg);
        let help_key_fg = color_from_name(&view.theme.help_key_fg);
        let help_key_bg = color_from_name(&view.theme.help_key_bg);
        let help_desc_fg = color_from_name(&view.theme.help_desc_fg);
        let help_desc_bg = color_from_name(&view.theme.help_desc_bg);

        let mut spans = vec![];
        let parts: Vec<&str> = view.text.split('|').collect();

        for (i, part) in parts.iter().enumerate() {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }

            if let Some(space_idx) = part.find(' ') {
                let key = &part[..space_idx];
                let desc = &part[space_idx..];

                spans.push(Span::styled(
                    key,
                    Style::default().fg(help_key_fg).bg(help_key_bg).add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(
                    desc,
                    Style::default().fg(help_desc_fg).bg(help_desc_bg),
                ));
            } else {
                spans.push(Span::styled(part.to_string(), Style::default().fg(fg).bg(bg)));
            }

            if i < parts.len() - 1 {
                spans.push(Span::styled(" | ", Style::default().fg(fg).bg(bg)));
            }
        }

        if spans.is_empty() {
            spans.push(Span::styled(view.text, Style::default().fg(fg).bg(bg)));
        }

        let widget = Paragraph::new(Line::from(spans))
            .style(Style::default().bg(bg));
        frame.render_widget(widget, area);
    }
}

pub fn render_preview(frame: &mut Frame, area: Rect, view: PreviewView<'_>) {
    let title_fg = color_from_name(&view.theme.list_title_fg);
    let title_bg = color_from_name(&view.theme.list_title_bg);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(view.title)
        .title_style(Style::default().fg(title_fg).bg(title_bg).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(title_fg));

    let paragraph = Paragraph::new(view.content)
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    use ratatui::{backend::TestBackend, Terminal};

    fn buffer_lines(terminal: &Terminal<TestBackend>) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn renders_entry_list_with_selection() {
        let backend = TestBackend::new(28, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        let items = vec!["Alpha".to_string(), "Beta".to_string()];
        let theme = ThemeConfig::default();

        terminal
            .draw(|frame| {
                render_entry_list(
                    frame,
                    frame.area(),
                    EntryListView {
                        items: &items,
                        selected: Some(1),
                        total_items: 2,
                        scroll_offset: 0,
                        theme: &theme,
                        search_query: "",
                        show_preview: None,
                        preview_content: None,
                        preview_height: 0,
                    },
                );
            })
            .unwrap();

        let lines = buffer_lines(&terminal);
        // Find which lines contain the entries (accounting for borders)
        let alpha_line = lines.iter().position(|l| l.contains("Alpha")).expect("Alpha should be rendered");
        let beta_line = lines.iter().position(|l| l.contains("Beta")).expect("Beta should be rendered");
        assert!(alpha_line < beta_line, "Alpha should come before Beta");
        assert!(!lines[beta_line].contains("▶"), "indicator should be removed");
    }

    #[test]
    fn renders_status_bar_in_normal_mode() {
        let backend = TestBackend::new(40, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = ThemeConfig::default();

        terminal
            .draw(|frame| {
                render_status_bar(
                    frame,
                    frame.area(),
                    StatusBarView {
                        text: "/ search | e edit",
                        is_search_mode: false,
                        cursor_position: None,
                        theme: &theme,
                    },
                );
            })
            .unwrap();

        let lines = buffer_lines(&terminal);
        assert!(lines[0].contains("/"));
        assert!(lines[0].contains("search"));
    }

    #[test]
    fn renders_status_bar_in_search_mode() {
        let backend = TestBackend::new(40, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = ThemeConfig::default();

        terminal
            .draw(|frame| {
                render_status_bar(
                    frame,
                    frame.area(),
                    StatusBarView {
                        text: "drought",
                        is_search_mode: true,
                        cursor_position: Some(3),
                        theme: &theme,
                    },
                );
            })
            .unwrap();

        let lines = buffer_lines(&terminal);
        assert!(lines[0].contains("/"));
        assert!(lines[0].contains("drought"));
    }
}
