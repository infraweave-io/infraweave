use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::tui::app::{App, View};
use crate::tui::widgets::footer::FooterBar;
use crate::tui::widgets::loading::LoadingWidget;
use crate::tui::widgets::navigation::NavigationBar;

/// Render loading screen
pub fn render_loading(frame: &mut Frame, area: Rect, app: &App) {
    let widget = LoadingWidget::new(&app.loading_message);
    widget.render(frame, area);
}

/// Render navigation menu bar
pub fn render_navigation(frame: &mut Frame, area: Rect, app: &App) {
    let widget = NavigationBar::new(&app.current_view);
    widget.render(frame, area);
}

/// Render search bar (when in search mode)
pub fn render_search_bar(frame: &mut Frame, area: Rect, app: &App) {
    let search_text = format!("/{}", app.search_state.search_query);
    let search_bar = Paragraph::new(Line::from(vec![
        Span::styled("🔍 ", Style::default().fg(Color::Yellow)),
        Span::styled(search_text, Style::default().fg(Color::White)),
        Span::styled(
            "_",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::SLOW_BLINK),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(Span::styled(
                " 🔍 Search ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
    );

    frame.render_widget(search_bar, area);
}

/// Render header with current view info
pub fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let (icon, title) = match app.current_view {
        View::Modules => ("📦", format!("Modules (Track: {})", app.current_track)),
        View::Stacks => ("📚", "Stacks".to_string()),
        View::Policies => ("📋", "Policies".to_string()),
        View::Deployments => ("🚀", "Deployments".to_string()),
    };

    let count = match app.current_view {
        View::Modules => format!(" • {} items", app.modules.len()),
        View::Deployments => format!(" • {} items", app.deployments.len()),
        _ => String::new(),
    };

    let content = vec![
        Span::styled(icon, Style::default().fg(Color::Cyan)),
        Span::raw(" "),
        Span::styled(
            title,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(count, Style::default().fg(Color::DarkGray)),
    ];

    let header = Paragraph::new(Line::from(content)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta))
            .title(Span::styled(
                " ⚡ InfraWeave ",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            )),
    );

    frame.render_widget(header, area);
}

/// Render footer with keyboard shortcuts
pub fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let actions = get_footer_actions(app);
    let widget = FooterBar::new(actions);
    widget.render(frame, area);
}

/// Helper to determine which footer actions to show
fn get_footer_actions(app: &App) -> Vec<(&'static str, &'static str)> {
    if app.search_state.search_mode && app.detail_state.showing_detail {
        vec![
            ("↑↓/PgUp/PgDn", "Navigate"),
            ("ESC/q", "Close Details"),
            ("Ctrl+C", "Quit"),
        ]
    } else if app.search_state.search_mode {
        vec![
            ("Type", "Search"),
            ("↑↓/PgUp/PgDn", "Navigate"),
            ("Enter", "Details"),
            ("ESC/q", "Exit Search"),
            ("Ctrl+C", "Quit"),
        ]
    } else if app.events_state.showing_events {
        let mut shortcuts = vec![
            ("1/2/3", "Events/Logs/Changelog"),
            ("Tab", "Next View"),
            ("←→/hl", "Switch Pane"),
            (
                "↑↓/jk",
                if app.events_focus_right {
                    "Scroll"
                } else {
                    "Select Job"
                },
            ),
            ("PgUp/PgDn", "Page Scroll"),
        ];

        // Add auto-refresh indicator when viewing logs of an in-progress deployment
        if app.events_log_view == crate::tui::app::EventsLogView::Logs && app.auto_refresh_logs {
            // Check if the current deployment is in progress
            if let Some(deployment_id) = app.events_deployment_id.as_str().split("::").last() {
                for event in &app.events_data {
                    if event.deployment_id == deployment_id {
                        let status = event.status.to_lowercase();
                        if status == "requested" || status == "initiated" {
                            shortcuts.push(("🔄", "Auto-refresh (5s)"));
                            break;
                        }
                    }
                }
            }
        }

        shortcuts.push(("ESC/q", "Close"));
        shortcuts
    } else if app.detail_state.showing_detail {
        // Show different shortcuts for structured detail view (module/stack/deployment) vs simple text view
        if app.detail_state.detail_module.is_some()
            || app.detail_state.detail_stack.is_some()
            || app.detail_state.detail_deployment.is_some()
        {
            vec![
                ("←→/hl", "Switch Pane"),
                (
                    "↑↓/jk",
                    if app.detail_state.detail_focus_right {
                        "Scroll"
                    } else {
                        "Browse"
                    },
                ),
                ("PgUp/PgDn", "Page Scroll"),
                (
                    "w",
                    if app.detail_wrap_text {
                        "Wrap: ON"
                    } else {
                        "Wrap: OFF"
                    },
                ),
                ("ESC/q", "Close"),
                ("Ctrl+C", "Quit"),
            ]
        } else {
            vec![
                ("↑↓/jk/PgUp/PgDn", "Navigate"),
                ("ESC/q", "Close"),
                ("Ctrl+C", "Quit"),
            ]
        }
    } else if matches!(app.current_view, View::Modules) {
        vec![
            ("←→", "Switch Track"),
            ("↑↓/jk/PgUp/PgDn", "Navigate"),
            ("/", "Search"),
            ("Enter", "Details"),
            ("r", "Reload"),
            ("Ctrl+C", "Quit"),
        ]
    } else if matches!(app.current_view, View::Stacks) {
        vec![
            ("←→", "Switch Track"),
            ("↑↓/jk/PgUp/PgDn", "Navigate"),
            ("/", "Search"),
            ("Enter", "Details"),
            ("r", "Reload"),
            ("Ctrl+C", "Quit"),
        ]
    } else if matches!(app.current_view, View::Deployments) {
        vec![
            ("↑↓/jk/PgUp/PgDn", "Navigate"),
            ("/", "Search"),
            ("Enter", "Details"),
            ("e", "Events"),
            ("r", "Reload"),
            ("Ctrl+R", "Reapply"),
            ("Ctrl+D", "Destroy"),
            ("Ctrl+C", "Quit"),
        ]
    } else {
        vec![
            ("↑↓/jk/PgUp/PgDn", "Navigate"),
            ("/", "Search"),
            ("Enter", "Details"),
            ("r", "Reload"),
            ("Ctrl+C", "Quit"),
        ]
    }
}
