use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::tui::app::App;
use crate::tui::widgets::table::TableWidget;

/// Render the modules list view
pub fn render_modules(frame: &mut Frame, area: Rect, app: &App) {
    let filtered_modules = app.get_filtered_modules();

    if filtered_modules.is_empty() {
        render_empty_state(frame, area, app);
        return;
    }

    // Create table rows
    let rows: Vec<Vec<String>> = filtered_modules
        .iter()
        .map(|module| {
            let module_name = if module.has_deprecated {
                format!("⚠️  {}", module.module_name)
            } else {
                module.module_name.clone()
            };
            vec![
                module_name,
                module
                    .stable_version
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                module.rc_version.clone().unwrap_or_else(|| "-".to_string()),
                module
                    .beta_version
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                module
                    .alpha_version
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                module
                    .dev_version
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
            ]
        })
        .collect();

    // Create header
    let headers = vec!["Module Name", "Stable", "RC", "Beta", "Alpha", "Dev"];
    let widths = vec![30, 10, 10, 15, 15, 20];

    let widget = TableWidget::new("Modules", "📦", headers, widths)
        .with_rows(rows)
        .with_selected(app.selected_index);

    widget.render(frame, area);
}

fn render_empty_state(frame: &mut Frame, area: Rect, app: &App) {
    let message_text = if app.search_state.search_mode && !app.search_state.search_query.is_empty()
    {
        format!("🔍 No modules match '{}'", app.search_state.search_query)
    } else if app.modules.is_empty() {
        "No modules available".to_string()
    } else {
        format!("🔍 No modules match '{}'", app.search_state.search_query)
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
