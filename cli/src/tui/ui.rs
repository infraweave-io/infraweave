use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use super::app::{App, View};
use env_defs::EventData;

fn render_loading(frame: &mut Frame, area: Rect, app: &App) {
    let loading_text = vec![
        Line::from(""),
        Line::from(""),
        Line::from(vec![
            Span::styled("⏳ ", Style::default().fg(Color::Yellow)),
            Span::styled(
                &app.loading_message,
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

fn render_navigation(frame: &mut Frame, area: Rect, app: &App) {
    let menu_items = vec![
        ("1", "Modules", View::Modules),
        ("2", "Stacks", View::Stacks),
        ("3", "Policies", View::Policies),
        ("4", "Deployments", View::Deployments),
    ];

    let spans: Vec<Span> = menu_items
        .iter()
        .flat_map(|(key, label, view)| {
            let is_active = &app.current_view == view;
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

fn render_search_bar(frame: &mut Frame, area: Rect, app: &App) {
    let search_text = format!("/{}", app.search_query);
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

#[allow(dead_code)]
fn render_track_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let tabs: Vec<Span> = app
        .available_tracks
        .iter()
        .enumerate()
        .flat_map(|(idx, track)| {
            let is_selected = idx == app.selected_track_index;
            let (label_style, bracket_style) = if is_selected {
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
                Span::raw("  "),
            ]
        })
        .collect();

    let track_bar = Paragraph::new(Line::from(tabs)).block(
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

    frame.render_widget(track_bar, area);
}

pub fn render(frame: &mut Frame, app: &mut App) {
    let size = frame.area();

    // If showing events view, use simplified layout without navigation/header
    if app.showing_events {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Min(0),    // Content (events view)
                Constraint::Length(3), // Actions footer
            ])
            .split(size);

        render_events(frame, chunks[0], app);
        render_footer(frame, chunks[1], app);
        return;
    }

    // If showing detail view, use simplified layout without navigation/header
    if app.showing_detail {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Min(0),    // Content (detail view)
                Constraint::Length(3), // Actions footer
            ])
            .split(size);

        render_detail(frame, chunks[0], app);
        render_footer(frame, chunks[1], app);
        return;
    }

    // Create main layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if app.search_mode && !app.is_loading {
            vec![
                Constraint::Length(3), // Navigation menu
                Constraint::Length(3), // Header
                Constraint::Length(3), // Search bar
                Constraint::Min(0),    // Content
                Constraint::Length(3), // Actions footer
            ]
        } else {
            vec![
                Constraint::Length(3), // Navigation menu
                Constraint::Length(3), // Header (with track tabs side by side for modules)
                Constraint::Min(0),    // Content
                Constraint::Length(3), // Actions footer
            ]
        })
        .split(size);

    // Render navigation menu
    render_navigation(frame, chunks[0], app);

    // Render header
    render_header(frame, chunks[1], app);

    // Determine content chunk index based on what's shown
    let content_chunk_idx = if app.search_mode && !app.is_loading {
        render_search_bar(frame, chunks[2], app);
        3
    } else {
        2
    };

    // Render content based on current view or loading screen
    if app.is_loading {
        render_loading(frame, chunks[content_chunk_idx], app);
    } else {
        match app.current_view {
            View::Modules => render_modules(frame, chunks[content_chunk_idx], app),
            View::Stacks => render_stacks(frame, chunks[content_chunk_idx], app),
            View::Policies => render_policies(frame, chunks[content_chunk_idx], app),
            View::Deployments => render_deployments(frame, chunks[content_chunk_idx], app),
        }
    }

    // Render actions footer
    render_footer(frame, chunks[content_chunk_idx + 1], app);

    // Render modals on top if active
    if app.showing_versions_modal {
        render_versions_modal(frame, size, app);
    }

    if app.showing_confirmation {
        render_confirmation_modal(frame, size, app);
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
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

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let actions = if app.search_mode && app.showing_detail {
        vec![
            ("↑↓/PgUp/PgDn", "Navigate"),
            ("ESC/q", "Close Details"),
            ("Ctrl+C", "Quit"),
        ]
    } else if app.search_mode {
        vec![
            ("Type", "Search"),
            ("↑↓/PgUp/PgDn", "Navigate"),
            ("Enter", "Details"),
            ("ESC/q", "Exit Search"),
            ("Ctrl+C", "Quit"),
        ]
    } else if app.showing_events {
        vec![
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
            ("ESC/q", "Close"),
        ]
    } else if app.showing_detail {
        vec![
            ("↑↓/jk/PgUp/PgDn", "Navigate"),
            ("ESC/q", "Close"),
            ("Ctrl+C", "Quit"),
        ]
    } else if matches!(app.current_view, View::Modules) {
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
    };

    let spans: Vec<Span> = actions
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
            if i < actions.len() - 1 {
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

fn render_modules(frame: &mut Frame, area: Rect, app: &App) {
    let filtered_modules = app.get_filtered_modules();

    if filtered_modules.is_empty() {
        let message_text = if app.search_mode && !app.search_query.is_empty() {
            format!("🔍 No modules match '{}'", app.search_query)
        } else if app.modules.is_empty() {
            "📭 No modules found".to_string()
        } else {
            format!("🔍 No modules match '{}'", app.search_query)
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
        return;
    }

    let items: Vec<ListItem> = filtered_modules
        .iter()
        .map(|module| {
            let content = vec![Span::styled(
                module.module_name.clone(),
                Style::default().fg(Color::Cyan),
            )];
            ListItem::new(Line::from(content))
        })
        .collect();

    let header = vec![Span::styled(
        "Module Name",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    )];

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
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

fn render_stacks(frame: &mut Frame, area: Rect, _app: &App) {
    let message = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            "🚧 Coming Soon!",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Stacks view is under development",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Press [1] for Modules or [4] for Deployments",
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

fn render_policies(frame: &mut Frame, area: Rect, _app: &App) {
    let message = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            "🚧 Coming Soon!",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Policies view is under development",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Press [1] for Modules or [4] for Deployments",
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

fn render_deployments(frame: &mut Frame, area: Rect, app: &App) {
    let filtered_deployments = app.get_filtered_deployments();

    if filtered_deployments.is_empty() {
        let message_text = if app.search_mode && !app.search_query.is_empty() {
            format!("🔍 No deployments match '{}'", app.search_query)
        } else if app.deployments.is_empty() {
            "📭 No deployments found".to_string()
        } else {
            format!("🔍 No deployments match '{}'", app.search_query)
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

fn render_detail(frame: &mut Frame, area: Rect, app: &mut App) {
    // Update the visible lines based on the actual rendered area
    // Subtract 2 for borders
    app.detail_visible_lines = area.height.saturating_sub(2);

    // If we have structured module data, render it nicely
    if let Some(module) = app.detail_module.clone() {
        render_module_detail(frame, area, app, &module);
    } else {
        // Fallback to simple text rendering for deployments or when module data is missing
        let (icon, title) = match app.current_view {
            View::Modules => ("📦", "Module Details"),
            View::Deployments => ("🚀", "Deployment Details"),
            _ => ("📄", "Details"),
        };

        // Update total lines count
        app.detail_total_lines = app.detail_content.lines().count() as u16;

        let lines: Vec<Line> = app
            .detail_content
            .lines()
            .skip(app.detail_scroll as usize)
            .map(|line| Line::from(line.to_string()))
            .collect();

        let title_line = Line::from(vec![
            Span::styled(icon, Style::default().fg(Color::Cyan)),
            Span::raw(" "),
            Span::styled(
                title,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);

        let paragraph = Paragraph::new(lines)
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Magenta))
                    .title(title_line),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, area);
    }
}

fn render_events(frame: &mut Frame, area: Rect, app: &mut App) {
    // Create two-pane layout: left for job list, right for logs
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(35), // Left: Job list
            Constraint::Percentage(65), // Right: Logs
        ])
        .split(area);

    // Get grouped events - clone the data to avoid borrow issues
    let grouped_events: Vec<(String, Vec<EventData>)> = app
        .get_grouped_events()
        .into_iter()
        .map(|(job_id, events)| {
            (
                job_id.clone(),
                events.iter().map(|e| (*e).clone()).collect(),
            )
        })
        .collect();

    if grouped_events.is_empty() {
        let message = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "📭 No events found",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
        ])
        .alignment(ratatui::layout::Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(Span::styled(
                    " Events ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
        );
        frame.render_widget(message, area);
        return;
    }

    // Build job list items
    let job_items: Vec<ListItem> = grouped_events
        .iter()
        .map(|(job_id, events)| {
            // Get the last event for this job to show current status
            let last_event = events.last().unwrap();
            let status = &last_event.status;

            // Extract action from event data
            // Try to get command from first event's metadata or event name
            let (action, action_color) = if let Some(first_event) = events.first() {
                // Check metadata for command field
                if let Some(command) = first_event.metadata.get("command").and_then(|v| v.as_str())
                {
                    match command.to_lowercase().as_str() {
                        "plan" => ("plan", Color::Cyan),
                        "apply" => ("apply", Color::Green),
                        "destroy" => ("destroy", Color::Red),
                        _ => {
                            // Fallback to checking job_id or event name
                            if job_id.contains("plan")
                                || first_event.event.to_lowercase().contains("plan")
                            {
                                ("plan", Color::Cyan)
                            } else if job_id.contains("apply")
                                || first_event.event.to_lowercase().contains("apply")
                            {
                                ("apply", Color::Green)
                            } else if job_id.contains("destroy")
                                || first_event.event.to_lowercase().contains("destroy")
                            {
                                ("destroy", Color::Red)
                            } else {
                                (command, Color::White)
                            }
                        }
                    }
                } else {
                    // Fallback to checking job_id or event name
                    if job_id.contains("plan") || first_event.event.to_lowercase().contains("plan")
                    {
                        ("plan", Color::Cyan)
                    } else if job_id.contains("apply")
                        || first_event.event.to_lowercase().contains("apply")
                    {
                        ("apply", Color::Green)
                    } else if job_id.contains("destroy")
                        || first_event.event.to_lowercase().contains("destroy")
                    {
                        ("destroy", Color::Red)
                    } else {
                        ("job", Color::White)
                    }
                }
            } else {
                ("job", Color::White)
            };

            // Color code based on status
            let (status_icon, status_color) = match status.as_str() {
                "completed" | "success" => ("✓", Color::Green),
                "failed" | "error" => ("✗", Color::Red),
                "in_progress" | "running" => ("⏳", Color::Yellow),
                _ => ("•", Color::White),
            };

            // Get timestamp from last event
            let timestamp = &last_event.timestamp;

            let lines = vec![
                Line::from(vec![
                    Span::styled(
                        format!("{:<8}", action),
                        Style::default()
                            .fg(action_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        status_icon,
                        Style::default()
                            .fg(status_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(truncate(&status, 15), Style::default().fg(status_color)),
                ]),
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        truncate(timestamp, 25),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
            ];
            ListItem::new(lines)
        })
        .collect();

    let job_border_color = if !app.events_focus_right {
        Color::White
    } else {
        Color::Yellow
    };
    let job_list = List::new(job_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(job_border_color))
                .title(Span::styled(
                    " 📅 Jobs ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    let mut job_state = ListState::default();
    job_state.select(Some(app.events_browser_index));

    frame.render_stateful_widget(job_list, chunks[0], &mut job_state);

    // Render content for selected job (right pane)
    if let Some((job_id, events)) = grouped_events.get(app.events_browser_index) {
        app.detail_visible_lines = chunks[1].height.saturating_sub(2);

        let mut log_lines: Vec<Line> = Vec::new();

        // Render different content based on selected view
        match app.events_log_view {
            super::app::EventsLogView::Events => {
                // Render events view (original content)
                let is_loading = app.is_loading;
                let current_job_id = &app.events_current_job_id;
                let logs = &app.events_logs;
                render_events_content(
                    &mut log_lines,
                    job_id,
                    events,
                    is_loading,
                    current_job_id,
                    logs,
                );
            }
            super::app::EventsLogView::Logs => {
                // Render logs view
                let is_loading = app.is_loading;
                let current_job_id = &app.events_current_job_id;
                let logs = &app.events_logs;
                render_logs_content(&mut log_lines, job_id, is_loading, current_job_id, logs);
            }
            super::app::EventsLogView::Changelog => {
                // Render changelog view
                render_changelog_content(&mut log_lines, job_id, events);
            }
        }

        app.detail_total_lines = log_lines.len() as u16;

        // Apply scrolling
        let visible_lines: Vec<Line> = log_lines
            .into_iter()
            .skip(app.events_scroll as usize)
            .collect();

        // Create title with navigation tabs
        let title_line = create_events_title(job_id, app);

        let logs_border_color = if app.events_focus_right {
            Color::White
        } else {
            Color::Cyan
        };
        let paragraph = Paragraph::new(visible_lines)
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(logs_border_color))
                    .title(title_line),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, chunks[1]);
    }
}

fn create_events_title<'a>(job_id: &'a str, app: &'a App) -> Line<'a> {
    use super::app::EventsLogView;

    let action = if job_id.contains("plan") {
        ("PLAN", Color::Cyan)
    } else if job_id.contains("apply") {
        ("APPLY", Color::Green)
    } else if job_id.contains("destroy") {
        ("DESTROY", Color::Red)
    } else {
        ("JOB", Color::White)
    };

    let (events_style, logs_style, changelog_style) = match app.events_log_view {
        EventsLogView::Events => (
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            Style::default().fg(Color::DarkGray),
            Style::default().fg(Color::DarkGray),
        ),
        EventsLogView::Logs => (
            Style::default().fg(Color::DarkGray),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            Style::default().fg(Color::DarkGray),
        ),
        EventsLogView::Changelog => (
            Style::default().fg(Color::DarkGray),
            Style::default().fg(Color::DarkGray),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ),
    };

    Line::from(vec![
        Span::styled(
            action.0,
            Style::default().fg(action.1).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" │ "),
        Span::styled("1:", events_style),
        Span::styled("Events", events_style),
        Span::raw(" "),
        Span::styled("2:", logs_style),
        Span::styled("Logs", logs_style),
        Span::raw(" "),
        Span::styled("3:", changelog_style),
        Span::styled("Changelog", changelog_style),
    ])
}

fn render_events_content<'a>(
    log_lines: &mut Vec<Line<'a>>,
    job_id: &'a str,
    events: &'a [EventData],
    is_loading: bool,
    current_job_id: &'a str,
    logs: &'a [env_defs::LogData],
) {
    // Extract action from job_id
    let action = if job_id.contains("plan") {
        "PLAN"
    } else if job_id.contains("apply") {
        "APPLY"
    } else if job_id.contains("destroy") {
        "DESTROY"
    } else {
        "JOB"
    };

    let action_color = match action {
        "PLAN" => Color::Cyan,
        "APPLY" => Color::Green,
        "DESTROY" => Color::Red,
        _ => Color::White,
    };

    // Get last event for overall status
    let last_event = events.last().unwrap();
    let (status_icon, status_color) = match last_event.status.as_str() {
        "completed" | "success" => ("✓", Color::Green),
        "failed" | "error" => ("✗", Color::Red),
        "in_progress" | "running" => ("⏳", Color::Yellow),
        _ => ("•", Color::White),
    };

    // Show job header with action and status
    log_lines.push(Line::from(vec![
        Span::styled(
            action,
            Style::default()
                .fg(action_color)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ),
        Span::raw("  "),
        Span::styled(
            status_icon,
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            &last_event.status,
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    log_lines.push(Line::from(vec![
        Span::styled("Job: ", Style::default().fg(Color::DarkGray)),
        Span::styled(job_id, Style::default().fg(Color::Cyan)),
    ]));
    log_lines.push(Line::from(""));

    // Show events for this job
    for (idx, event) in events.iter().enumerate() {
        let (evt_icon, evt_color) = match event.status.as_str() {
            "completed" | "success" => ("✓", Color::Green),
            "failed" | "error" => ("✗", Color::Red),
            "in_progress" | "running" => ("⏳", Color::Yellow),
            _ => ("•", Color::White),
        };

        log_lines.push(Line::from(Span::styled(
            "━".repeat(70),
            Style::default().fg(Color::DarkGray),
        )));
        log_lines.push(Line::from(vec![
            Span::styled(
                format!("Event {} ", idx + 1),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                evt_icon,
                Style::default().fg(evt_color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                &event.event,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        log_lines.push(Line::from(vec![
            Span::styled("  Status: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                &event.status,
                Style::default().fg(evt_color).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  │  "),
            Span::styled("Time: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&event.timestamp, Style::default().fg(Color::White)),
        ]));

        if !event.error_text.is_empty() {
            log_lines.push(Line::from(""));
            log_lines.push(Line::from(Span::styled(
                "  ⚠ Error:",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
            for line in event.error_text.lines() {
                log_lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(line.to_string(), Style::default().fg(Color::Red)),
                ]));
            }
        }

        if event.output != serde_json::Value::Null {
            log_lines.push(Line::from(""));
            log_lines.push(Line::from(Span::styled(
                "  📄 Output:",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            let output_str =
                serde_json::to_string_pretty(&event.output).unwrap_or_else(|_| "{}".to_string());
            for line in output_str.lines() {
                log_lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(
                        line.to_string(),
                        Style::default().fg(Color::Rgb(120, 120, 120)),
                    ),
                ]));
            }
        }

        log_lines.push(Line::from(""));
    }

    // Add logs section
    log_lines.push(Line::from(""));
    log_lines.push(Line::from(Span::styled(
        "═".repeat(70),
        Style::default().fg(Color::Yellow),
    )));
    log_lines.push(Line::from(Span::styled(
        "📝 Job Logs",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    )));
    log_lines.push(Line::from(Span::styled(
        "═".repeat(70),
        Style::default().fg(Color::Yellow),
    )));
    log_lines.push(Line::from(""));

    if is_loading && current_job_id == job_id {
        log_lines.push(Line::from(vec![
            Span::styled("⏳ ", Style::default().fg(Color::Yellow)),
            Span::styled("Loading logs...", Style::default().fg(Color::Yellow)),
        ]));
    } else if logs.is_empty() {
        log_lines.push(Line::from(Span::styled(
            "No logs available for this job",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for log in logs.iter() {
            // Split multi-line log messages
            for line in log.message.lines() {
                log_lines.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::White),
                )));
            }
        }
    }
}

fn render_logs_content<'a>(
    log_lines: &mut Vec<Line<'a>>,
    job_id: &'a str,
    is_loading: bool,
    current_job_id: &'a str,
    logs: &'a [env_defs::LogData],
) {
    log_lines.push(Line::from(vec![Span::styled(
        "📝 Job Logs",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    )]));
    log_lines.push(Line::from(vec![
        Span::styled("Job: ", Style::default().fg(Color::DarkGray)),
        Span::styled(job_id, Style::default().fg(Color::Cyan)),
    ]));
    log_lines.push(Line::from(""));

    if is_loading && current_job_id == job_id {
        log_lines.push(Line::from(vec![
            Span::styled("⏳ ", Style::default().fg(Color::Yellow)),
            Span::styled("Loading logs...", Style::default().fg(Color::Yellow)),
        ]));
    } else if logs.is_empty() {
        log_lines.push(Line::from(Span::styled(
            "No logs available for this job",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for log in logs.iter() {
            // Split multi-line log messages
            for line in log.message.lines() {
                log_lines.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::White),
                )));
            }
        }
    }
}

fn render_changelog_content<'a>(
    log_lines: &mut Vec<Line<'a>>,
    _job_id: &'a str,
    events: &'a [EventData],
) {
    log_lines.push(Line::from(vec![Span::styled(
        "� Changelog",
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    )]));
    log_lines.push(Line::from(""));

    // Show a timeline of events with timestamps
    for event in events.iter() {
        let (status_icon, status_color) = match event.status.as_str() {
            "completed" | "success" => ("✓", Color::Green),
            "failed" | "error" => ("✗", Color::Red),
            "in_progress" | "running" => ("⏳", Color::Yellow),
            _ => ("•", Color::White),
        };

        log_lines.push(Line::from(vec![
            Span::styled(
                status_icon,
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(&event.timestamp, Style::default().fg(Color::DarkGray)),
            Span::raw(" - "),
            Span::styled(&event.event, Style::default().fg(Color::Cyan)),
        ]));

        if !event.error_text.is_empty() {
            log_lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("Error: ", Style::default().fg(Color::Red)),
                Span::styled(
                    truncate(&event.error_text, 80),
                    Style::default().fg(Color::Red),
                ),
            ]));
        }

        log_lines.push(Line::from(""));
    }
}

fn render_module_detail(
    frame: &mut Frame,
    area: Rect,
    app: &mut App,
    module: &env_defs::ModuleResp,
) {
    // Create two-pane layout: left for navigation tree, right for details
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30), // Left: Navigation tree
            Constraint::Percentage(70), // Right: Details
        ])
        .split(area);

    // Build navigation tree items
    let mut nav_items = vec!["📋 General".to_string()];

    if !module.tf_variables.is_empty() {
        nav_items.push(format!("🔧 Variables ({})", module.tf_variables.len()));

        // Sort variables: required first, then optional
        let mut sorted_vars: Vec<_> = module.tf_variables.iter().collect();
        sorted_vars.sort_by_key(|var| {
            let is_required = is_variable_required(var);
            (!is_required, var.name.clone()) // Sort by required (reversed), then by name
        });

        for var in sorted_vars {
            let camel_case = to_camel_case(&var.name);
            let is_required = is_variable_required(var);
            let icon = if is_required { "* " } else { "" };
            nav_items.push(format!("  └─ {}{}", icon, camel_case));
        }
    }

    if !module.tf_outputs.is_empty() {
        nav_items.push(format!("📤 Outputs ({})", module.tf_outputs.len()));
        for output in &module.tf_outputs {
            let camel_case = to_camel_case(&output.name);
            nav_items.push(format!("  └─ {}", camel_case));
        }
    }

    // Render navigation tree (left pane)
    let nav_list_items: Vec<ListItem> = nav_items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let style = if idx == app.detail_browser_index {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(item.clone(), style)))
        })
        .collect();

    // Use white border for focused pane, magenta for unfocused
    let nav_border_color = if !app.detail_focus_right {
        Color::White
    } else {
        Color::Magenta
    };

    let nav_list = List::new(nav_list_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(nav_border_color))
                .title(Span::styled(
                    " 🗂️  Browse ",
                    Style::default()
                        .fg(nav_border_color)
                        .add_modifier(Modifier::BOLD),
                )),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    let mut nav_state = ListState::default();
    nav_state.select(Some(app.detail_browser_index));

    frame.render_stateful_widget(nav_list, chunks[0], &mut nav_state);

    // Render detail content (right pane) based on selected item
    // Update the visible lines based on the right pane's actual area (subtract 2 for borders)
    app.detail_visible_lines = chunks[1].height.saturating_sub(2);

    let scroll_pos = app.detail_scroll as usize;
    let detail_lines = build_detail_content(app, module);
    let total_lines = detail_lines.len() as u16;

    // Update total lines count for scroll calculation
    app.detail_total_lines = total_lines;

    // Apply scrolling
    let visible_lines: Vec<Line> = detail_lines.into_iter().skip(scroll_pos).collect();

    let title_line = Line::from(vec![
        Span::styled("📦 ", Style::default().fg(Color::Cyan)),
        Span::styled(
            "Module Details",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    // Use white border for focused pane, magenta for unfocused
    let detail_border_color = if app.detail_focus_right {
        Color::White
    } else {
        Color::Magenta
    };

    let paragraph = Paragraph::new(visible_lines)
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(detail_border_color))
                .title(title_line),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, chunks[1]);
}

fn to_camel_case(snake_case: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;

    for c in snake_case.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_uppercase().next().unwrap_or(c));
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }

    result
}

fn is_variable_required(var: &env_defs::TfVariable) -> bool {
    // A variable is required if:
    // 1. It has no default value (default is None)
    // 2. OR it's not nullable and has default value as null
    if var.default.is_none() {
        return true;
    }

    if !var.nullable && var.default == Some(serde_json::Value::Null) {
        return true;
    }

    false
}

fn build_detail_content(app: &App, module: &env_defs::ModuleResp) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();

    // Determine what to show based on selected browser index
    let mut current_idx = 0;

    // General section (index 0)
    if app.detail_browser_index == current_idx {
        lines.push(Line::from(Span::styled(
            "📋 General Information",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "═".repeat(60),
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));

        lines.push(Line::from(vec![
            Span::styled("Module: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                module.module_name.clone(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        lines.push(Line::from(vec![
            Span::styled("Version: ", Style::default().fg(Color::DarkGray)),
            Span::styled(module.version.clone(), Style::default().fg(Color::Yellow)),
        ]));

        lines.push(Line::from(vec![
            Span::styled("Track: ", Style::default().fg(Color::DarkGray)),
            Span::styled(module.track.clone(), Style::default().fg(Color::Green)),
        ]));

        lines.push(Line::from(vec![
            Span::styled("Type: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                module.module_type.clone(),
                Style::default().fg(Color::White),
            ),
        ]));

        lines.push(Line::from(""));

        if !module.description.is_empty() {
            lines.push(Line::from(Span::styled(
                "Description:",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                module.description.clone(),
                Style::default().fg(Color::White),
            )));
            lines.push(Line::from(""));
        }

        lines.push(Line::from(Span::styled(
            "Summary:",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(vec![
            Span::raw("  • Variables: "),
            Span::styled(
                module.tf_variables.len().to_string(),
                Style::default().fg(Color::Yellow),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::raw("  • Outputs: "),
            Span::styled(
                module.tf_outputs.len().to_string(),
                Style::default().fg(Color::Cyan),
            ),
        ]));

        if !module.tf_required_providers.is_empty() {
            lines.push(Line::from(vec![
                Span::raw("  • Required Providers: "),
                Span::styled(
                    module.tf_required_providers.len().to_string(),
                    Style::default().fg(Color::Blue),
                ),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Raw JSON",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "─".repeat(60),
            Style::default().fg(Color::DarkGray),
        )));

        for line in app.detail_content.lines() {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(Color::Rgb(80, 80, 80)),
            )));
        }

        return lines;
    }
    current_idx += 1;

    // Variables section
    if !module.tf_variables.is_empty() {
        // Sort variables: required first, then optional
        let mut sorted_vars: Vec<_> = module.tf_variables.iter().collect();
        sorted_vars.sort_by_key(|var| {
            let is_required = is_variable_required(var);
            (!is_required, var.name.clone()) // Sort by required (reversed), then by name
        });

        // Variables category header
        if app.detail_browser_index == current_idx {
            lines.push(Line::from(Span::styled(
                "🔧 Terraform Variables",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                "═".repeat(60),
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));

            for var in &sorted_vars {
                let type_str = match &var._type {
                    serde_json::Value::String(s) => s.clone(),
                    other => format!("{}", other),
                };
                let is_required = is_variable_required(var);
                let camel_case = to_camel_case(&var.name);

                // Highlight required variables with red bullet and bold name
                let bullet = if is_required {
                    Span::styled("⚠ ", Style::default().fg(Color::Red))
                } else {
                    Span::styled("• ", Style::default().fg(Color::Yellow))
                };

                let name_style = if is_required {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                } else {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                };

                lines.push(Line::from(vec![
                    bullet,
                    Span::styled(camel_case, name_style),
                    Span::raw(" : "),
                    Span::styled(type_str, Style::default().fg(Color::Blue)),
                ]));

                if !var.description.is_empty() {
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(var.description.clone(), Style::default().fg(Color::White)),
                    ]));
                }

                if let Some(default) = &var.default {
                    let default_str = match default {
                        serde_json::Value::String(s) => format!("\"{}\"", s),
                        serde_json::Value::Null => "null".to_string(),
                        other => format!("{}", other),
                    };
                    lines.push(Line::from(vec![
                        Span::raw("  Default: "),
                        Span::styled(default_str, Style::default().fg(Color::Green)),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(
                            "⚠ REQUIRED",
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ),
                    ]));
                }

                lines.push(Line::from(""));
            }

            return lines;
        }
        current_idx += 1;

        // Individual variables
        for var in &sorted_vars {
            if app.detail_browser_index == current_idx {
                let type_str = match &var._type {
                    serde_json::Value::String(s) => s.clone(),
                    other => format!("{}", other),
                };
                let is_required = is_variable_required(var);
                let camel_case = to_camel_case(&var.name);

                let icon = if is_required { "⚠ " } else { "🔧 " };

                lines.push(Line::from(vec![
                    Span::styled(
                        icon,
                        Style::default().fg(if is_required {
                            Color::Red
                        } else {
                            Color::Magenta
                        }),
                    ),
                    Span::styled(
                        camel_case,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                lines.push(Line::from(Span::styled(
                    "═".repeat(60),
                    Style::default().fg(Color::DarkGray),
                )));
                lines.push(Line::from(""));

                lines.push(Line::from(vec![
                    Span::styled("Type: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(type_str, Style::default().fg(Color::Blue)),
                ]));

                if !var.description.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "Description:",
                        Style::default().fg(Color::DarkGray),
                    )));
                    lines.push(Line::from(Span::styled(
                        var.description.clone(),
                        Style::default().fg(Color::White),
                    )));
                }

                lines.push(Line::from(""));

                if is_required {
                    lines.push(Line::from(vec![
                        Span::styled("⚠ ", Style::default().fg(Color::Red)),
                        Span::styled(
                            "REQUIRED",
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ),
                    ]));
                    lines.push(Line::from(""));
                }

                if let Some(default) = &var.default {
                    let default_str = match default {
                        serde_json::Value::String(s) => format!("\"{}\"", s),
                        serde_json::Value::Null => "null".to_string(),
                        other => serde_json::to_string_pretty(other)
                            .unwrap_or_else(|_| format!("{}", other)),
                    };
                    lines.push(Line::from(Span::styled(
                        "Default Value:".to_string(),
                        Style::default().fg(Color::DarkGray),
                    )));
                    for line in default_str.lines() {
                        lines.push(Line::from(Span::styled(
                            line.to_string(),
                            Style::default().fg(Color::Green),
                        )));
                    }
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Attributes:",
                    Style::default().fg(Color::DarkGray),
                )));

                lines.push(Line::from(vec![
                    Span::raw("  • Nullable: "),
                    Span::styled(
                        if var.nullable { "Yes" } else { "No" },
                        if var.nullable {
                            Style::default().fg(Color::Green)
                        } else {
                            Style::default().fg(Color::Red)
                        },
                    ),
                ]));

                lines.push(Line::from(vec![
                    Span::raw("  • Sensitive: "),
                    Span::styled(
                        if var.sensitive { "Yes ⚠" } else { "No" },
                        if var.sensitive {
                            Style::default().fg(Color::Red)
                        } else {
                            Style::default().fg(Color::Green)
                        },
                    ),
                ]));

                return lines;
            }
            current_idx += 1;
        }
    }

    // Outputs section
    if !module.tf_outputs.is_empty() {
        // Outputs category header
        if app.detail_browser_index == current_idx {
            lines.push(Line::from(Span::styled(
                "� Terraform Outputs",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                "═".repeat(60),
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));

            for output in &module.tf_outputs {
                let camel_case = to_camel_case(&output.name);

                lines.push(Line::from(vec![
                    Span::styled("• ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        camel_case,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));

                if !output.description.is_empty() {
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(
                            output.description.clone(),
                            Style::default().fg(Color::White),
                        ),
                    ]));
                }

                lines.push(Line::from(""));
            }

            return lines;
        }
        current_idx += 1;

        // Individual outputs
        for output in &module.tf_outputs {
            if app.detail_browser_index == current_idx {
                let camel_case = to_camel_case(&output.name);

                lines.push(Line::from(vec![
                    Span::styled("📤 ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        camel_case,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                lines.push(Line::from(Span::styled(
                    "═".repeat(60),
                    Style::default().fg(Color::DarkGray),
                )));
                lines.push(Line::from(""));

                if !output.description.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "Description:",
                        Style::default().fg(Color::DarkGray),
                    )));
                    lines.push(Line::from(Span::styled(
                        output.description.clone(),
                        Style::default().fg(Color::White),
                    )));
                    lines.push(Line::from(""));
                }

                lines.push(Line::from(Span::styled(
                    "Value Expression:".to_string(),
                    Style::default().fg(Color::DarkGray),
                )));
                for line in output.value.lines() {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(Color::Green),
                    )));
                }

                return lines;
            }
            current_idx += 1;
        }
    }

    lines
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}…", &s[..max_len - 1])
    } else {
        s.to_string()
    }
}

fn render_versions_modal(frame: &mut Frame, area: Rect, app: &App) {
    // Create a centered modal area (80% width, 80% height)
    let modal_area = Rect {
        x: area.width / 10,
        y: area.height / 10,
        width: area.width * 8 / 10,
        height: area.height * 8 / 10,
    };

    // Create a darkened overlay background by filling the area with dark characters
    // This creates a visual "fade" effect
    use ratatui::widgets::Clear;
    frame.render_widget(Clear, area); // Clear the area first
    let overlay = Block::default().style(Style::default().bg(Color::Rgb(20, 20, 20)));
    frame.render_widget(overlay, area);

    // Create modal layout with header and content
    let modal_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header with track selector
            Constraint::Min(0),    // Versions list
            Constraint::Length(3), // Footer with instructions
        ])
        .split(modal_area);

    // Split header into two parts: module name and track selector
    let header_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(modal_chunks[0]);

    // Render module name
    let module_header = Paragraph::new(Line::from(vec![
        Span::styled("📦 ", Style::default().fg(Color::Cyan)),
        Span::styled(
            &app.modal_module_name,
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

    // Render track selector
    let track_tabs: Vec<Span> = app
        .available_tracks
        .iter()
        .enumerate()
        .filter(|(_, track)| track.as_str() != "all") // Skip "all" in modal
        .flat_map(|(idx, track)| {
            let is_selected = idx == app.modal_track_index;
            let is_available = app.modal_available_tracks.contains(track);

            let (label_style, bracket_style) = if !is_available {
                // Unavailable tracks: very dark grey
                (
                    Style::default().fg(Color::Rgb(60, 60, 60)),
                    Style::default().fg(Color::Rgb(60, 60, 60)),
                )
            } else if is_selected {
                // Selected available track: cyan and bold
                (
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                    Style::default().fg(Color::Cyan),
                )
            } else {
                // Available but not selected: dark grey
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
        .collect();

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

    // Render versions list
    if app.modal_versions.is_empty() {
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
        let items: Vec<ListItem> = app
            .modal_versions
            .iter()
            .map(|version| {
                let content = vec![
                    Span::styled(
                        format!("{:<40}", truncate(&version.version, 39)),
                        Style::default().fg(Color::Green),
                    ),
                    Span::styled(
                        version.timestamp.clone(),
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
        state.select(Some(app.modal_selected_index));

        frame.render_stateful_widget(list, modal_chunks[1], &mut state);
    }

    // Render footer with instructions
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

fn render_confirmation_modal(frame: &mut Frame, area: Rect, app: &App) {
    // Create a centered modal area (60% width, 40% height, but at least 15 lines)
    let modal_width = std::cmp::max(area.width * 6 / 10, 50);
    let modal_height = std::cmp::max(area.height * 4 / 10, 15);
    let modal_area = Rect {
        x: (area.width.saturating_sub(modal_width)) / 2,
        y: (area.height.saturating_sub(modal_height)) / 2,
        width: modal_width,
        height: modal_height,
    };

    // Create a darkened overlay background
    use ratatui::widgets::Clear;
    frame.render_widget(Clear, area);
    let overlay = Block::default().style(Style::default().bg(Color::Rgb(20, 20, 20)));
    frame.render_widget(overlay, area);

    // Create modal layout with message and buttons
    let modal_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // Message
            Constraint::Length(3), // Buttons
        ])
        .split(modal_area);

    // Render the confirmation message
    let message_lines: Vec<Line> = app
        .confirmation_message
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

    // Render buttons
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
