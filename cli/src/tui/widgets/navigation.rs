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
}

impl<'a> NavigationBar<'a> {
    pub fn new(current_view: &'a View) -> Self {
        Self { current_view }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let menu_items = vec![
            ("1", "Modules", View::Modules),
            ("2", "Stacks", View::Stacks),
            ("3", "Policies", View::Policies),
            ("4", "Deployments", View::Deployments),
        ];

        let spans: Vec<Span> = menu_items
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
