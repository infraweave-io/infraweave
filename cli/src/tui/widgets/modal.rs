use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

pub struct ConfirmationModal<'a> {
    pub message: &'a str,
}

impl<'a> ConfirmationModal<'a> {
    pub fn new(message: &'a str) -> Self {
        Self { message }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let modal_width = std::cmp::max(area.width * 6 / 10, 50);
        let modal_height = std::cmp::max(area.height * 4 / 10, 15);
        let modal_area = Rect {
            x: (area.width.saturating_sub(modal_width)) / 2,
            y: (area.height.saturating_sub(modal_height)) / 2,
            width: modal_width,
            height: modal_height,
        };

        frame.render_widget(Clear, area);
        let overlay = Block::default().style(Style::default().bg(Color::Rgb(20, 20, 20)));
        frame.render_widget(overlay, area);

        let modal_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)])
            .split(modal_area);

        let message_lines: Vec<Line> = self
            .message
            .lines()
            .map(|line| Line::from(Span::styled(line, Style::default().fg(Color::White))))
            .collect();

        let message = Paragraph::new(message_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow))
                    .title(Span::styled(
                        " ⚠️  Confirmation ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )),
            )
            .wrap(Wrap { trim: true });
        frame.render_widget(message, modal_chunks[0]);

        let buttons = vec![
            Span::styled(
                "[Y]",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Yes  "),
            Span::styled(
                "[N]",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" No  "),
            Span::styled(
                "[ESC]",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Cancel"),
        ];

        let button_bar = Paragraph::new(Line::from(buttons))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            )
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(button_bar, modal_chunks[1]);
    }
}

pub struct VersionsModal<'a, T> {
    pub module_name: &'a str,
    pub versions: &'a [T],
    pub selected_index: usize,
    pub current_track: &'a str,
    pub available_tracks: &'a [String],
    pub all_tracks: &'a [String],
    pub track_index: usize,
}

impl<'a, T> VersionsModal<'a, T>
where
    T: VersionItem,
{
    pub fn new(
        module_name: &'a str,
        versions: &'a [T],
        selected_index: usize,
        current_track: &'a str,
        available_tracks: &'a [String],
        all_tracks: &'a [String],
        track_index: usize,
    ) -> Self {
        Self {
            module_name,
            versions,
            selected_index,
            current_track,
            available_tracks,
            all_tracks,
            track_index,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let modal_area = Rect {
            x: area.width / 10,
            y: area.height / 10,
            width: area.width * 8 / 10,
            height: area.height * 8 / 10,
        };

        frame.render_widget(Clear, area);
        let overlay = Block::default().style(Style::default().bg(Color::Rgb(20, 20, 20)));
        frame.render_widget(overlay, area);

        let modal_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
            ])
            .split(modal_area);

        let header_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(modal_chunks[0]);

        let module_header = Paragraph::new(Line::from(vec![
            Span::styled("📦 ", Style::default().fg(Color::Cyan)),
            Span::styled(
                self.module_name,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(Span::styled(
                    " Module ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
        );
        frame.render_widget(module_header, header_chunks[0]);

        let track_tabs = self.build_track_tabs();
        let track_bar = Paragraph::new(Line::from(track_tabs)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(Span::styled(
                    " 🎯 Track ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
        );
        frame.render_widget(track_bar, header_chunks[1]);

        if self.versions.is_empty() {
            let message = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "📭 No versions found",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
            ])
            .alignment(ratatui::layout::Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );
            frame.render_widget(message, modal_chunks[1]);
        } else {
            let items: Vec<ListItem> = self
                .versions
                .iter()
                .map(|version| {
                    let content = vec![
                        Span::styled(
                            format!("{:<40}", truncate(version.get_version(), 39)),
                            Style::default().fg(Color::Green),
                        ),
                        Span::styled(
                            version.get_timestamp().to_string(),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ];
                    ListItem::new(Line::from(content))
                })
                .collect();

            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan))
                        .title(Span::styled(
                            " Versions ",
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        )),
                )
                .highlight_style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▶ ");

            let mut state = ListState::default();
            state.select(Some(self.selected_index));

            frame.render_stateful_widget(list, modal_chunks[1], &mut state);
        }

        let instructions = vec![
            Span::styled(
                "←→",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Track  "),
            Span::styled(
                "↑↓",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Navigate  "),
            Span::styled(
                "r",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Reload  "),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Details  "),
            Span::styled(
                "ESC/q",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Close"),
        ];

        let footer = Paragraph::new(Line::from(instructions)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
        );
        frame.render_widget(footer, modal_chunks[2]);
    }

    fn build_track_tabs(&self) -> Vec<Span<'a>> {
        self.all_tracks
            .iter()
            .enumerate()
            .filter(|(_, track)| track.as_str() != "all")
            .flat_map(|(idx, track)| {
                let is_selected = idx == self.track_index;
                let is_available = self.available_tracks.contains(track);

                let (label_style, bracket_style) = if !is_available {
                    (
                        Style::default().fg(Color::Rgb(60, 60, 60)),
                        Style::default().fg(Color::Rgb(60, 60, 60)),
                    )
                } else if is_selected {
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
                    Span::styled(track.to_string(), label_style),
                    Span::styled("]", bracket_style),
                ]
            })
            .collect()
    }
}

pub trait VersionItem {
    fn get_version(&self) -> &str;
    fn get_timestamp(&self) -> &str;
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}…", &s[..max_len - 1])
    } else {
        s.to_string()
    }
}
