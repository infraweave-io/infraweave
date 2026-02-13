use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::tui::app::View;

pub struct NavigationBar<'a> {
    pub current_view: &'a View,
    pub project_id: &'a str,
    pub region: &'a str,
}

impl<'a> NavigationBar<'a> {
    pub fn new(current_view: &'a View, project_id: &'a str, region: &'a str) -> Self {
        Self {
            current_view,
            project_id,
            region,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let menu_items = [
            ("1", "Modules", View::Modules),
            ("2", "Stacks", View::Stacks),
            ("3", "Policies", View::Policies),
            ("4", "Deployments", View::Deployments),
        ];

        let mut spans: Vec<Span> = menu_items
            .iter()
            .flat_map(|(key, label, view)| {
                let is_active = self.current_view == view;
                let (label_style, bracket_style) = if is_active {
                    (
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                        Style::default().fg(Color::Cyan),
                    )
                } else {
                    (
                        Style::default().fg(Color::DarkGray),
                        Style::default().fg(Color::DarkGray),
                    )
                };

                vec![
                    Span::raw("  "),
                    Span::styled("[", bracket_style),
                    Span::styled(
                        key.to_string(),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("]", bracket_style),
                    Span::raw(" "),
                    Span::styled(label.to_string(), label_style),
                    Span::raw("  "),
                ]
            })
            .collect();

        // Add project info on the right side
        // Calculate padding to push the info to the right
        let menu_text_len: usize = menu_items
            .iter()
            .map(|(_key, label, _)| 2 + 3 + 1 + label.len() + 2) // "  [X] Label  "
            .sum();

        let project_info = format!("Project: {} | Region: {}", self.project_id, self.region);
        let project_info_len = project_info.len();

        // Calculate available width (subtract borders and title)
        let available_width = area.width.saturating_sub(4) as usize; // 2 for borders, 2 for padding

        // Add spacing to push project info to the right
        if menu_text_len + project_info_len + 3 < available_width {
            let padding_len = available_width.saturating_sub(menu_text_len + project_info_len);
            spans.push(Span::raw(" ".repeat(padding_len)));
        } else {
            spans.push(Span::raw("   "));
        }

        // Add project info spans
        spans.push(Span::styled(
            "Project: ",
            Style::default().fg(Color::DarkGray),
        ));
        spans.push(Span::styled(
            self.project_id,
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            " | Region: ",
            Style::default().fg(Color::DarkGray),
        ));
        spans.push(Span::styled(
            self.region,
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));

        let navigation = Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue))
                .title(Span::styled(
                    " 🧭 Navigation ",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                )),
        );

        frame.render_widget(navigation, area);
    }
}
