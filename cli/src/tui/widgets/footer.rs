use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub struct FooterBar<'a> {
    pub actions: Vec<(&'a str, &'a str)>,
}

impl<'a> FooterBar<'a> {
    pub fn new(actions: Vec<(&'a str, &'a str)>) -> Self {
        Self { actions }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let spans: Vec<Span> = self
            .actions
            .iter()
            .enumerate()
            .flat_map(|(i, (key, action))| {
                let mut result = vec![
                    Span::raw("  "),
                    Span::styled(
                        key.to_string(),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" → ", Style::default().fg(Color::DarkGray)),
                    Span::styled(action.to_string(), Style::default().fg(Color::White)),
                ];
                if i < self.actions.len() - 1 {
                    result.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
                }
                result
            })
            .collect();

        let footer = Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green))
                .title(Span::styled(
                    " ⌨️  Keyboard Shortcuts ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )),
        );

        frame.render_widget(footer, area);
    }
}
