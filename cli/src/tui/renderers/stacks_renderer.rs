use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::tui::app::App;
use crate::tui::widgets::table::TableWidget;

/// Render the stacks list view
pub fn render_stacks(frame: &mut Frame, area: Rect, app: &App) {
    let filtered_stacks = app.get_filtered_stacks();

    if filtered_stacks.is_empty() {
        render_empty_state(frame, area, app);
        return;
    }

    // Create table rows
    let rows: Vec<Vec<String>> = filtered_stacks
        .iter()
        .map(|stack| {
            vec![
                stack.module_name.clone(),
                stack
                    .stable_version
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                stack.rc_version.clone().unwrap_or_else(|| "-".to_string()),
                stack
                    .beta_version
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                stack
                    .alpha_version
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                stack.dev_version.clone().unwrap_or_else(|| "-".to_string()),
            ]
        })
        .collect();

    // Create header
    let headers = vec!["Stack Name", "Stable", "RC", "Beta", "Alpha", "Dev"];
    let widths = vec![30, 10, 10, 15, 15, 20];

    let widget = TableWidget::new("📚 Stacks", "📚", headers, widths)
        .with_rows(rows)
        .with_selected(app.selected_index);

    widget.render(frame, area);
}

fn render_empty_state(frame: &mut Frame, area: Rect, app: &App) {
    let message_text = if app.search_state.search_mode && !app.search_state.search_query.is_empty()
    {
        format!("🔍 No stacks match '{}'", app.search_state.search_query)
    } else if app.stacks.is_empty() {
        "📭 No stacks found".to_string()
    } else {
        format!("🔍 No stacks match '{}'", app.search_state.search_query)
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
