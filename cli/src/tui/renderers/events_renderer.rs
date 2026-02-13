use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::tui::app::{App, EventsLogView};
use env_defs::EventData;

/// Helper function to truncate strings
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}…", &s[..max_len - 1])
    } else {
        s.to_string()
    }
}

/// Render events view (events/logs/changelog)
pub fn render_events(frame: &mut Frame, area: Rect, app: &mut App) {
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
                    Span::styled(truncate(status, 15), Style::default().fg(status_color)),
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
        // Create layout for right pane: navigation box + content
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Navigation box
                Constraint::Min(0),    // Content
            ])
            .split(chunks[1]);

        // Render navigation box
        let nav_line = create_nav_line(job_id, app);
        let nav_border_color = if app.events_focus_right {
            Color::White
        } else {
            Color::Cyan
        };
        let nav_box = Paragraph::new(nav_line)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(nav_border_color)),
            )
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(nav_box, right_chunks[0]);

        // Calculate visible lines for content area
        app.detail_visible_lines = right_chunks[1].height.saturating_sub(2);

        let mut log_lines: Vec<Line> = Vec::new();

        // Render different content based on selected view
        match app.events_log_view {
            EventsLogView::Events => {
                // Render events view (original content)
                render_events_content(&mut log_lines, job_id, events);
            }
            EventsLogView::Logs => {
                // Render logs view
                let is_loading = app.is_loading;
                let current_job_id = &app.events_current_job_id;
                let logs = &app.events_logs;
                render_logs_content(&mut log_lines, job_id, is_loading, current_job_id, logs);
            }
            EventsLogView::Changelog => {
                // Render changelog view - show change record if available
                let is_loading = app.is_loading;
                let change_record = app.change_records.get(job_id);
                render_changelog_content(&mut log_lines, job_id, events, is_loading, change_record);
            }
        }

        app.detail_total_lines = log_lines.len() as u16;

        // Apply scrolling
        let visible_lines: Vec<Line> = log_lines
            .into_iter()
            .skip(app.events_scroll as usize)
            .collect();

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
                    .border_style(Style::default().fg(logs_border_color)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, right_chunks[1]);
    }
}

fn create_nav_line<'a>(job_id: &'a str, app: &'a App) -> Line<'a> {
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
        Span::styled("[1] ", events_style),
        Span::styled("Events", events_style),
        Span::raw("  │  "),
        Span::styled("[2] ", logs_style),
        Span::styled("Logs", logs_style),
        Span::raw("  │  "),
        Span::styled("[3] ", changelog_style),
        Span::styled("Changelog", changelog_style),
    ])
}

fn render_events_content<'a>(
    log_lines: &mut Vec<Line<'a>>,
    job_id: &'a str,
    events: &'a [EventData],
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
    job_id: &'a str,
    events: &'a [EventData],
    is_loading: bool,
    change_record: Option<&'a env_defs::InfraChangeRecord>,
) {
    log_lines.push(Line::from(vec![Span::styled(
        "📜 Changelog",
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    )]));
    log_lines.push(Line::from(vec![
        Span::styled("Job: ", Style::default().fg(Color::DarkGray)),
        Span::styled(job_id, Style::default().fg(Color::Cyan)),
    ]));
    log_lines.push(Line::from(""));

    // If change record is available, display the plan/apply output
    if let Some(record) = change_record {
        // Display the plan/apply output with diff-style coloring
        for line in record.plan_std_output.lines() {
            let trimmed = line.trim_start();
            let styled_line = if trimmed.starts_with('+') {
                // Added lines in green
                Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Green),
                ))
            } else if trimmed.starts_with('-') {
                // Removed lines in red
                Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Red),
                ))
            } else if trimmed.starts_with('~') {
                // Modified lines in yellow
                Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Yellow),
                ))
            } else if trimmed.starts_with('#') {
                // Comments/headers in cyan
                Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Cyan),
                ))
            } else {
                // Regular lines in white
                Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::White),
                ))
            };
            log_lines.push(styled_line);
        }
    } else if is_loading {
        log_lines.push(Line::from(vec![
            Span::styled("⏳ ", Style::default().fg(Color::Yellow)),
            Span::styled(
                "Loading change record...",
                Style::default().fg(Color::Yellow),
            ),
        ]));
    } else {
        // No change record available
        log_lines.push(Line::from(vec![
            Span::styled("ℹ️  ", Style::default().fg(Color::Cyan)),
            Span::styled(
                "No change record available for this job",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        log_lines.push(Line::from(""));

        // Fallback to event timeline if no change record available
        log_lines.push(Line::from(Span::styled(
            "Event Timeline",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
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
}
