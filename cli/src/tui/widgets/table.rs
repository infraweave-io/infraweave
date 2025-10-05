use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Row, Table, TableState},
    Frame,
};

pub struct TableWidget<'a> {
    pub header: Vec<&'a str>,
    pub rows: Vec<Vec<String>>,
    pub selected_index: usize,
    pub title: &'a str,
    pub title_icon: &'a str,
    pub widths: Vec<u16>,
}

impl<'a> TableWidget<'a> {
    pub fn new(
        title: &'a str,
        title_icon: &'a str,
        header: Vec<&'a str>,
        widths: Vec<u16>,
    ) -> Self {
        Self {
            header,
            rows: Vec::new(),
            selected_index: 0,
            title,
            title_icon,
            widths,
        }
    }

    pub fn with_rows(mut self, rows: Vec<Vec<String>>) -> Self {
        self.rows = rows;
        self
    }

    pub fn with_selected(mut self, selected: usize) -> Self {
        self.selected_index = selected;
        self
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let rows: Vec<Row> = self
            .rows
            .iter()
            .enumerate()
            .map(|(idx, row)| {
                let style = if idx == self.selected_index {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                Row::new(row.clone()).style(style)
            })
            .collect();

        let header = Row::new(
            self.header
                .iter()
                .map(|h| h.to_string())
                .collect::<Vec<_>>(),
        )
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )
        .bottom_margin(1);

        let constraints: Vec<ratatui::layout::Constraint> = self
            .widths
            .iter()
            .map(|&w| ratatui::layout::Constraint::Percentage(w))
            .collect();

        let table = Table::new(rows, constraints)
            .header(header)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(Span::styled(
                        format!(" {} {} ", self.title_icon, self.title),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )),
            )
            .highlight_symbol("▶ ");

        let mut state = TableState::default();
        state.select(Some(self.selected_index));

        frame.render_stateful_widget(table, area, &mut state);
    }
}
