use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub struct LoadingWidget {
    pub message: String,
}

impl LoadingWidget {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let loading_text = vec![
            Line::from(""),
            Line::from(""),
            Line::from(vec![
                Span::styled("⏳ ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    &self.message,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Please wait...",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let loading = Paragraph::new(loading_text)
            .style(Style::default())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(Span::styled(
                        " Loading ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )),
            )
            .alignment(ratatui::layout::Alignment::Center);

        frame.render_widget(loading, area);
    }
}
