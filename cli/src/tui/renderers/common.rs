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
    // If a project filter is selected, show that instead of the context project ID
    let display_project_id = app
        .selected_project_filter
        .as_deref()
        .unwrap_or(&app.project_id);

    // If a region filter is selected, show that. Otherwise fall back to the configured region,
    // or "All Regions" if no specific region is configured.
    let display_region = app.selected_region_filter.as_deref().unwrap_or_else(|| {
        if app.region != "unknown" && !app.region.is_empty() {
            &app.region
        } else {
            "All Regions"
        }
    });

    let widget = NavigationBar::new(
        &app.current_view,
        display_project_id,
        display_region,
        app.available_projects.len(),
    );
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
        // View::Deployments => format!(" • {} items", app.deployments.len()),
        View::Deployments => format!(" • {} items", app.get_filtered_deployments().len()),
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
        vec![("ESC/q", "Close Details"), ("Ctrl+C", "Quit")]
    } else if app.search_state.search_mode {
        vec![
            ("Type", "Search"),
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
        ];

        // Add reload shortcut when viewing logs
        if app.events_log_view == crate::tui::app::EventsLogView::Logs {
            shortcuts.push(("r", "Reload Logs"));
        }

        shortcuts.push(("ESC/q", "Close"));
        shortcuts
    } else if app.detail_state.showing_detail {
        // Show different shortcuts for structured detail view (module/stack/deployment) vs simple text view
        if app.detail_state.detail_module.is_some()
            || app.detail_state.detail_stack.is_some()
            || app.detail_state.detail_deployment.is_some()
        {
            let mut shortcuts = vec![
                ("←→/hl", "Switch Pane"),
                (
                    "↑↓/jk",
                    if app.detail_state.detail_focus_right {
                        "Scroll"
                    } else {
                        "Browse"
                    },
                ),
                (
                    "w",
                    if app.detail_wrap_text {
                        "Wrap: ON"
                    } else {
                        "Wrap: OFF"
                    },
                ),
            ];

            // Add reload shortcut when viewing logs section in deployment details
            if app.detail_deployment.is_some()
                && app.detail_browser_index == app.calculate_logs_section_index()
                && !app.events_current_job_id.is_empty()
            {
                shortcuts.push(("r", "Reload Logs"));
            }
            // Add reload shortcut when viewing General section in deployment details
            else if app.detail_deployment.is_some() && app.detail_browser_index == 0 {
                shortcuts.push(("r", "Reload Details"));
            }

            shortcuts.push(("c", "Build Claim"));
            shortcuts.push(("ESC/q", "Close"));
            shortcuts.push(("Ctrl+C", "Quit"));

            shortcuts
        } else {
            vec![("ESC/q", "Close"), ("Ctrl+C", "Quit")]
        }
    } else if matches!(app.current_view, View::Modules) {
        vec![
            ("←→", "Switch Track"),
            ("/", "Search"),
            ("Enter", "Details"),
            ("r", "Reload"),
            ("Ctrl+C", "Quit"),
        ]
    } else if matches!(app.current_view, View::Stacks) {
        vec![
            ("←→", "Switch Track"),
            ("/", "Search"),
            ("Ctrl+N", "New Stack"),
            ("Enter", "Details"),
            ("r", "Reload"),
            ("Ctrl+C", "Quit"),
        ]
    } else if matches!(app.current_view, View::Deployments) {
        vec![
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
            ("/", "Search"),
            ("Enter", "Details"),
            ("r", "Reload"),
            ("Ctrl+C", "Quit"),
        ]
    }
}
