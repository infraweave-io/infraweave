use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use super::app::{App, View};
use super::renderers::{
    common::{render_footer, render_header, render_loading, render_navigation, render_search_bar},
    deployments_renderer, detail_renderer, events_renderer, modules_renderer, policies_renderer,
    stacks_renderer,
};

/// Main render function - orchestrates the entire UI
pub fn render(frame: &mut Frame, app: &mut App) {
    let size = frame.area();

    // If showing events view, use simplified layout without navigation/header
    if app.events_state.showing_events {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Min(0),    // Content (events view)
                Constraint::Length(3), // Actions footer
            ])
            .split(size);

        events_renderer::render_events(frame, chunks[0], app);
        render_footer(frame, chunks[1], app);
        return;
    }

    // If showing detail view, use simplified layout without navigation/header
    if app.detail_state.showing_detail {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Min(0),    // Content (detail view)
                Constraint::Length(3), // Actions footer
            ])
            .split(size);

        detail_renderer::render_detail(frame, chunks[0], app);
        render_footer(frame, chunks[1], app);
        return;
    }

    // Create main layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if app.search_state.search_mode && !app.is_loading {
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
                Constraint::Length(3), // Header
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
    let content_chunk_idx = if app.search_state.search_mode && !app.is_loading {
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
            View::Modules => {
                modules_renderer::render_modules(frame, chunks[content_chunk_idx], app)
            }
            View::Stacks => stacks_renderer::render_stacks(frame, chunks[content_chunk_idx], app),
            View::Policies => {
                policies_renderer::render_policies(frame, chunks[content_chunk_idx], app)
            }
            View::Deployments => {
                deployments_renderer::render_deployments(frame, chunks[content_chunk_idx], app)
            }
        }
    }

    // Render actions footer
    render_footer(frame, chunks[content_chunk_idx + 1], app);

    // Render modals on top if active
    if app.modal_state.showing_versions_modal {
        render_versions_modal(frame, size, app);
    }

    if app.modal_state.showing_confirmation {
        render_confirmation_modal(frame, size, app);
    }
}

/// Helper function to truncate strings
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
