use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::tui::app::App;

/// Helper function to truncate strings
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    } else {
        s.to_string()
    }
}

/// Render the deployments list view
pub fn render_deployments(frame: &mut Frame, area: Rect, app: &App) {
    let filtered_deployments = app.get_filtered_deployments();

    if filtered_deployments.is_empty() {
        render_empty_state(frame, area, app);
        return;
    }

    let items: Vec<ListItem> = filtered_deployments
        .iter()
        .map(|deployment| {
            let (status_icon, status_color) = match deployment.status.as_str() {
                "DEPLOYED" => ("✓", Color::Green),
                "FAILED" => ("✗", Color::Red),
                "IN_PROGRESS" => ("⏳", Color::Yellow),
                _ => ("•", Color::White),
            };

            let content = vec![
                Span::styled(
                    status_icon,
                    Style::default()
                        .fg(status_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    format!("{:<22}", truncate(&deployment.timestamp, 21)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:<14}", truncate(&deployment.status, 13)),
                    Style::default().fg(status_color),
                ),
                Span::styled(
                    format!("{:<40}", truncate(&deployment.deployment_id, 39)),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("{:<30}", truncate(&deployment.module, 29)),
                    Style::default().fg(Color::Magenta),
                ),
                Span::styled(
                    truncate(&deployment.environment, 25),
                    Style::default().fg(Color::Blue),
                ),
            ];

            ListItem::new(Line::from(content))
        })
        .collect();

    let header = vec![
        Span::raw("  "),
        Span::styled(
            format!("{:<22}", "Timestamp"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ),
        Span::styled(
            format!("{:<14}", "Status"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ),
        Span::styled(
            format!("{:<40}", "Deployment ID"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ),
        Span::styled(
            format!("{:<30}", "Module"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ),
        Span::styled(
            "Environment",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ),
    ];

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green))
                .title(Line::from(header)),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    let mut state = ListState::default();
    state.select(Some(app.selected_index));

    frame.render_stateful_widget(list, area, &mut state);
}

fn render_empty_state(frame: &mut Frame, area: Rect, app: &App) {
    let message_text = if app.search_state.search_mode && !app.search_state.search_query.is_empty()
    {
        format!(
            "🔍 No deployments match '{}'",
            app.search_state.search_query
        )
    } else if app.deployments.is_empty() {
        "📭 No deployments found".to_string()
    } else {
        format!(
            "🔍 No deployments match '{}'",
            app.search_state.search_query
        )
    };

    let message = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            message_text,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Press 'r' to reload or ESC to clear search",
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .alignment(ratatui::layout::Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );
    frame.render_widget(message, area);
}
