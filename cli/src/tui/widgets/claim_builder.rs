use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Padding, Paragraph, Wrap},
    Frame,
};

use crate::tui::state::claim_builder_state::ClaimBuilderState;

/// Render the claim builder view
pub fn render_claim_builder(f: &mut Frame, area: Rect, state: &ClaimBuilderState) {
    if state.show_preview {
        render_preview_mode(f, area, state);
    } else {
        render_form_mode(f, area, state);
    }
}

/// Render the form editing mode with improved UI/UX
fn render_form_mode(f: &mut Frame, area: Rect, state: &ClaimBuilderState) {
    // Main container with styled border
    let main_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(vec![
            Span::raw(" "),
            Span::styled("📝 ", Style::default().fg(Color::Yellow)),
            Span::styled(
                if state.is_stack {
                    format!(
                        "Build Deployment Claim - Stack: {}",
                        state
                            .source_stack
                            .as_ref()
                            .map(|s| s.module_name.as_str())
                            .unwrap_or("Unknown")
                    )
                } else {
                    format!(
                        "Build Deployment Claim - Module: {}",
                        state
                            .source_module
                            .as_ref()
                            .map(|m| m.module_name.as_str())
                            .unwrap_or("Unknown")
                    )
                },
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]);

    let inner = main_block.inner(area);
    f.render_widget(main_block, area);

    // Split into sections
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Status bar
            Constraint::Min(0),    // Form content
            Constraint::Length(if state.validation_error.is_some() {
                3
            } else {
                0
            }), // Validation error
            Constraint::Length(5), // Help section
        ])
        .split(inner);

    // Status bar with progress indicator
    render_status_bar(f, chunks[0], state);

    // Form fields with better layout
    render_enhanced_form_fields(f, chunks[1], state);

    // Validation error display
    if state.validation_error.is_some() {
        render_validation_error(f, chunks[2], state);
    }

    // Enhanced help text
    let help_chunk_index = if state.validation_error.is_some() {
        3
    } else {
        2
    };
    render_enhanced_help(f, chunks[help_chunk_index], state);
}

/// Render status bar with validation feedback
fn render_status_bar(f: &mut Frame, area: Rect, state: &ClaimBuilderState) {
    let filled_required = state
        .variable_inputs
        .iter()
        .filter(|v| v.is_required && !v.user_value.is_empty())
        .count();
    let total_required = state
        .variable_inputs
        .iter()
        .filter(|v| v.is_required)
        .count()
        + 1; // +1 for name only

    let base_filled = if !state.deployment_name.is_empty() {
        1
    } else {
        0
    };

    let total_filled = filled_required + base_filled;
    let progress = if total_required > 0 {
        (total_filled as f32 / total_required as f32 * 100.0) as u16
    } else {
        100
    };

    let status_text = vec![
        Span::styled("Progress: ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("{}% ", progress),
            if progress == 100 {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            },
        ),
        Span::styled("│ ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}/{} required fields ", total_filled, total_required),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled("│ ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{} total variables", state.variable_inputs.len()),
            Style::default().fg(Color::Gray),
        ),
    ];

    let status = Paragraph::new(Line::from(status_text)).alignment(Alignment::Left);
    f.render_widget(status, area);
}

/// Render validation error message
fn render_validation_error(f: &mut Frame, area: Rect, state: &ClaimBuilderState) {
    if let Some(error) = &state.validation_error {
        let error_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red))
            .title(vec![
                Span::raw(" "),
                Span::styled(
                    "❌ Validation Error",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
            ]);

        let error_text = Paragraph::new(error.as_str())
            .style(Style::default().fg(Color::Red))
            .block(error_block)
            .alignment(Alignment::Left);

        f.render_widget(error_text, area);
    }
}

/// Render the enhanced form fields with better visual hierarchy
fn render_enhanced_form_fields(f: &mut Frame, area: Rect, state: &ClaimBuilderState) {
    // Split into two columns for better layout
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // Left column: Form fields
    let left_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White))
        .title(vec![
            Span::raw(" "),
            Span::styled(
                "⚙️  Configuration",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ])
        .padding(Padding::new(1, 1, 0, 0));

    let left_inner = left_block.inner(columns[0]);
    f.render_widget(left_block, columns[0]);

    // Calculate scroll for form fields
    let available_height = left_inner.height as usize;
    let scroll_offset =
        if state.selected_field_index >= state.scroll_offset as usize + available_height {
            state.selected_field_index - available_height + 1
        } else if state.selected_field_index < state.scroll_offset as usize {
            state.selected_field_index
        } else {
            state.scroll_offset as usize
        };

    // Build form field items
    let mut items = Vec::new();

    // Base fields with icons (deployment name only)
    let base_fields = vec![(
        "🏷️  Deployment Name",
        &state.deployment_name,
        state.deployment_name_cursor,
        0,
        true,
        "A unique identifier for this deployment",
    )];

    for (idx, (label, value, cursor_pos, field_idx, is_required, _hint)) in
        base_fields.iter().enumerate()
    {
        if idx < scroll_offset {
            continue;
        }
        if items.len() >= available_height.saturating_sub(1) {
            break;
        }

        let is_selected = state.selected_field_index == *field_idx;
        let has_value = !value.is_empty();

        let label_style = if is_selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else if *is_required && !has_value {
            Style::default().fg(Color::Red)
        } else if has_value {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::White)
        };

        let value_style = if is_selected {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if has_value {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let display_value = if is_selected {
            let mut display = value.to_string();
            if *cursor_pos <= display.len() {
                display.insert(*cursor_pos, '▊');
            }
            display
        } else if value.is_empty() {
            if *is_required {
                "<required>".to_string()
            } else {
                "<optional>".to_string()
            }
        } else {
            value.to_string()
        };

        let label_text = if *is_required {
            format!("{} *", label)
        } else {
            label.to_string()
        };

        let mut line_spans = vec![
            Span::styled(label_text, label_style),
            Span::raw("\n  "),
            Span::styled(display_value, value_style),
        ];

        if is_selected {
            line_spans.push(Span::raw(" "));
            line_spans.push(Span::styled("◀", Style::default().fg(Color::Yellow)));
        }

        items.push(ListItem::new(Line::from(line_spans)));
    }

    // Add separator
    if items.len() < available_height.saturating_sub(1) && base_fields.len() >= scroll_offset {
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "─".repeat(50),
            Style::default().fg(Color::DarkGray),
        )])));
    }

    // Variable fields with better formatting
    let mut current_section: Option<String> = None;

    for (idx, var) in state.variable_inputs.iter().enumerate() {
        let global_idx = idx + 1; // Updated from 2 to 1 since we removed environment

        if global_idx < scroll_offset {
            // Still track section changes even when scrolled past
            if state.is_stack {
                if let Some((instance_name, _)) = var.name.split_once("__") {
                    current_section = Some(instance_name.to_string());
                }
            }
            continue;
        }
        if items.len() >= available_height.saturating_sub(1) {
            break;
        }

        // For stacks, add section headers for each module instance (only when visible)
        if state.is_stack {
            if let Some((instance_name, _)) = var.name.split_once("__") {
                if current_section.as_deref() != Some(instance_name) {
                    // New section - add header
                    if current_section.is_some() {
                        // Add spacing between sections
                        items.push(ListItem::new(Line::from("")));
                    }
                    items.push(ListItem::new(Line::from(vec![Span::styled(
                        format!("── {} ──", instance_name),
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD),
                    )])));
                    current_section = Some(instance_name.to_string());
                }
            }
        }

        let is_selected = state.selected_field_index == global_idx;
        let has_value = !var.user_value.is_empty();

        let label_style = if is_selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else if var.is_required && !has_value {
            Style::default().fg(Color::Red)
        } else if has_value {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Cyan)
        };

        let value_style = if is_selected {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if has_value {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let icon = if var.is_sensitive { "🔒" } else { "⚙️ " };

        // For stack variables, show just the variable name (after __)
        let display_name = if state.is_stack {
            var.name
                .split_once("__")
                .map(|(_, v)| v)
                .unwrap_or(&var.name)
        } else {
            &var.name
        };

        let label = if var.is_required {
            format!("{} {} *", icon, display_name)
        } else {
            format!("{} {}", icon, display_name)
        };

        let display_value = if is_selected {
            let mut display = var.user_value.clone();
            if var.cursor_position <= display.len() {
                display.insert(var.cursor_position, '▊');
            }
            display
        } else if var.user_value.is_empty() {
            // Show helpful placeholder based on type
            let type_lower = var.var_type.to_lowercase();
            if var.is_required {
                if type_lower.contains("bool") {
                    "<true|false>".to_string()
                } else if type_lower.contains("map") || type_lower.contains("object") {
                    "<{} - Ctrl+T for template>".to_string()
                } else if type_lower.contains("list")
                    || type_lower.contains("array")
                    || type_lower.contains("set")
                {
                    "<[] - Ctrl+T for template>".to_string()
                } else if type_lower.contains("number") || type_lower.contains("int") {
                    "<number>".to_string()
                } else {
                    "<required>".to_string()
                }
            } else {
                if type_lower.contains("bool") {
                    "<true|false>".to_string()
                } else if type_lower.contains("map") || type_lower.contains("object") {
                    "<{}>".to_string()
                } else if type_lower.contains("list")
                    || type_lower.contains("array")
                    || type_lower.contains("set")
                {
                    "<[]>".to_string()
                } else {
                    format!("<{}>", var.var_type.chars().take(20).collect::<String>())
                }
            }
        } else {
            var.user_value.clone()
        };

        let mut line_spans = vec![
            Span::styled(label, label_style),
            Span::raw("\n  "),
            Span::styled(display_value, value_style),
        ];

        if is_selected {
            line_spans.push(Span::raw(" "));
            line_spans.push(Span::styled("◀", Style::default().fg(Color::Yellow)));
        }

        items.push(ListItem::new(Line::from(line_spans)));
    }

    let list = List::new(items);
    f.render_widget(list, left_inner);

    // Right column: Help and info
    render_info_panel(f, columns[1], state);
}

/// Render the info panel on the right side
fn render_info_panel(f: &mut Frame, area: Rect, state: &ClaimBuilderState) {
    let info_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White))
        .title(vec![
            Span::raw(" "),
            Span::styled(
                "💡 Field Info",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ])
        .padding(Padding::new(1, 1, 1, 1));

    let inner = info_block.inner(area);
    f.render_widget(info_block, area);

    // Show info for current field
    let info_text = match state.selected_field_index {
        0 => vec![
            Line::from(vec![Span::styled(
                "Deployment Name",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Required: ", Style::default().fg(Color::Red)),
                Span::raw("Yes"),
            ]),
            Line::from(""),
            Line::from(
                "A unique identifier for this deployment. Must be unique within the environment.",
            ),
            Line::from(""),
            Line::from(vec![
                Span::styled("Example: ", Style::default().fg(Color::Yellow)),
                Span::raw("my-app-deployment"),
            ]),
        ],
        i if i >= 1 => {
            let var_idx = i - 1;
            if let Some(var) = state.variable_inputs.get(var_idx) {
                let mut lines = vec![
                    Line::from(vec![Span::styled(
                        &var.name,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )]),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled(
                            "Required: ",
                            if var.is_required {
                                Style::default().fg(Color::Red)
                            } else {
                                Style::default().fg(Color::Green)
                            },
                        ),
                        Span::raw(if var.is_required { "Yes" } else { "No" }),
                    ]),
                    Line::from(vec![
                        Span::styled("Type: ", Style::default().fg(Color::Yellow)),
                        Span::raw(&var.var_type),
                    ]),
                ];

                if var.is_sensitive {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![Span::styled(
                        "🔒 Sensitive",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    )]));
                }

                if !var.description.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(var.description.as_str()));
                }

                // Add type-specific hints
                let type_lower = var.var_type.to_lowercase();
                if type_lower.contains("bool") {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("💡 Hint: ", Style::default().fg(Color::Yellow)),
                        Span::raw("Type 'true' or 'false'"),
                    ]));
                } else if type_lower.contains("map") || type_lower.contains("object") {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("💡 Hint: ", Style::default().fg(Color::Yellow)),
                        Span::raw("Press Ctrl+T for {} template"),
                    ]));
                    lines.push(Line::from("Must be valid JSON object"));
                } else if type_lower.contains("list")
                    || type_lower.contains("array")
                    || type_lower.contains("set")
                {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("💡 Hint: ", Style::default().fg(Color::Yellow)),
                        Span::raw("Press Ctrl+T for [] template"),
                    ]));
                    lines.push(Line::from("Must be valid JSON array"));
                } else if type_lower.contains("number") || type_lower.contains("int") {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("💡 Hint: ", Style::default().fg(Color::Yellow)),
                        Span::raw("Numeric values only"),
                    ]));
                }

                lines
            } else {
                vec![Line::from("No info available")]
            }
        }
        _ => vec![Line::from("No info available")],
    };

    let info = Paragraph::new(info_text).wrap(Wrap { trim: false });
    f.render_widget(info, inner);
}

/// Render enhanced help text
fn render_enhanced_help(f: &mut Frame, area: Rect, _state: &ClaimBuilderState) {
    let help_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(vec![
            Span::raw(" "),
            Span::styled("⌨️  Keyboard Shortcuts", Style::default().fg(Color::White)),
            Span::raw(" "),
        ]);

    let inner = help_block.inner(area);
    f.render_widget(help_block, area);

    let help_lines = vec![
        Line::from(vec![
            Span::styled(
                "Tab",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" / "),
            Span::styled(
                "Shift+Tab",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Navigate  "),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Preview  "),
            Span::styled(
                "Ctrl+T",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Template"),
        ]),
        Line::from(vec![
            Span::styled(
                "Ctrl+S",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Save  "),
            Span::styled(
                "Ctrl+R",
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Run Claim  "),
            Span::styled(
                "Esc",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Close builder"),
        ]),
    ];

    let help = Paragraph::new(help_lines).alignment(Alignment::Center);
    f.render_widget(help, inner);
}

/// Render the preview mode with enhanced styling
fn render_preview_mode(f: &mut Frame, area: Rect, state: &ClaimBuilderState) {
    // Main container
    let main_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(vec![
            Span::raw(" "),
            Span::styled("📋 ", Style::default().fg(Color::Yellow)),
            Span::styled(
                "Deployment Claim Preview",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]);

    let inner = main_block.inner(area);
    f.render_widget(main_block, area);

    // Split into preview, validation, and help
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Validation status
            Constraint::Min(0),    // YAML preview
            Constraint::Length(4), // Help
        ])
        .split(inner);

    // Validation status
    let validation_result = state.validate();
    let (status_icon, status_text, status_color) = match &validation_result {
        Ok(_) => ("✅", "Valid - Ready to save", Color::Green),
        Err(e) => ("❌", e.as_str(), Color::Red),
    };

    let status = Paragraph::new(vec![Line::from(vec![
        Span::styled(
            status_icon,
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            status_text,
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
    ])])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(status_color))
            .title(" Status "),
    )
    .alignment(Alignment::Center);
    f.render_widget(status, chunks[0]);

    // YAML preview with syntax-like highlighting
    let yaml_lines: Vec<Line> = state
        .generated_yaml
        .lines()
        .map(|line| {
            if line.starts_with("apiVersion:") || line.starts_with("kind:") {
                Line::from(Span::styled(
                    line,
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ))
            } else if line.trim().starts_with("name:") || line.trim().starts_with("environment:") {
                Line::from(Span::styled(line, Style::default().fg(Color::Green)))
            } else if line.contains(":") && !line.trim().starts_with("#") {
                let parts: Vec<&str> = line.splitn(2, ':').collect();
                if parts.len() == 2 {
                    Line::from(vec![
                        Span::styled(parts[0], Style::default().fg(Color::Cyan)),
                        Span::raw(":"),
                        Span::styled(parts[1], Style::default().fg(Color::Yellow)),
                    ])
                } else {
                    Line::from(Span::styled(line, Style::default().fg(Color::White)))
                }
            } else {
                Line::from(Span::styled(line, Style::default().fg(Color::White)))
            }
        })
        .collect();

    let preview = Paragraph::new(yaml_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(vec![
                    Span::raw(" "),
                    Span::styled(
                        "YAML Output",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                ])
                .padding(Padding::new(2, 2, 1, 1)),
        )
        .scroll((state.preview_scroll, 0));
    f.render_widget(preview, chunks[1]);

    // Help text
    let help_lines = vec![Line::from(vec![
        Span::styled(
            "Enter",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" / "),
        Span::styled(
            "Esc",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Back to form  "),
        Span::styled(
            "Ctrl+S",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Save  "),
        Span::styled(
            "Ctrl+R",
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Run Claim  "),
        Span::styled(
            "↑↓",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Scroll"),
    ])];

    let help = Paragraph::new(help_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Help "),
        )
        .alignment(Alignment::Center);
    f.render_widget(help, chunks[2]);
}
