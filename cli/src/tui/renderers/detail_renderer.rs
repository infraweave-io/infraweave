use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
    Frame,
};

use crate::tui::app::{App, PendingAction, View};
use crate::tui::utils::{to_camel_case, NavItem};

/// Render detail view (module/stack/deployment details)
pub fn render_detail(frame: &mut Frame, area: Rect, app: &mut App) {
    // Update the visible lines based on the actual rendered area
    // Subtract 2 for borders
    app.detail_visible_lines = area.height.saturating_sub(2);

    // If we have structured deployment data, render it nicely
    if let Some(deployment) = app.detail_deployment.clone() {
        render_deployment_detail(frame, area, app, &deployment);
    } else if let Some(stack) = app.detail_stack.clone() {
        // If we have structured stack data, render it nicely
        render_stack_detail(frame, area, app, &stack);
    } else if let Some(module) = app.detail_module.clone() {
        // If we have structured module data, render it nicely
        render_module_detail(frame, area, app, &module);
    } else {
        // Fallback to simple text rendering for deployments or when module data is missing
        let (icon, title) = match app.current_view {
            View::Modules => ("📦", "Module Details"),
            View::Stacks => ("📚", "Stack Details"),
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

fn render_deployment_detail(
    frame: &mut Frame,
    area: Rect,
    app: &mut App,
    deployment: &env_defs::DeploymentResp,
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

    if !deployment.variables.is_null() && deployment.variables.is_object() {
        let var_count = deployment
            .variables
            .as_object()
            .map(|o| o.len())
            .unwrap_or(0);
        if var_count > 0 {
            nav_items.push(format!("🔧 Variables ({})", var_count));
        }
    }

    if !deployment.output.is_null() && deployment.output.is_object() {
        let output_count = deployment.output.as_object().map(|o| o.len()).unwrap_or(0);
        if output_count > 0 {
            nav_items.push(format!("📤 Outputs ({})", output_count));
        }
    }

    if !deployment.dependencies.is_empty() {
        nav_items.push(format!(
            "🔗 Dependencies ({})",
            deployment.dependencies.len()
        ));
    }

    if !deployment.policy_results.is_empty() {
        nav_items.push(format!(
            "📜 Policy Results ({})",
            deployment.policy_results.len()
        ));
    }

    // Add logs section
    nav_items.push("📝 Logs".to_string());

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
    app.detail_visible_lines = chunks[1].height.saturating_sub(2);

    let scroll_pos = app.detail_scroll as usize;
    let detail_lines = build_deployment_detail_content(app, deployment);
    let total_lines = detail_lines.len() as u16;

    app.detail_total_lines = total_lines;

    // Apply scrolling
    let visible_lines: Vec<Line> = detail_lines.into_iter().skip(scroll_pos).collect();

    // Get the current section title from nav_items
    let section_title = nav_items
        .get(app.detail_browser_index)
        .map(|s| s.as_str())
        .unwrap_or("Deployment Details");

    let title_line = Line::from(vec![
        Span::styled("🚀 ", Style::default().fg(Color::Cyan)),
        Span::styled(
            section_title,
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

    let mut paragraph = Paragraph::new(visible_lines)
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(detail_border_color))
                .title(title_line),
        );

    // Apply wrapping based on the setting
    if app.detail_wrap_text {
        paragraph = paragraph.wrap(Wrap { trim: false });
    }

    frame.render_widget(paragraph, chunks[1]);

    // Render scrollbar for the detail pane
    if app.detail_focus_right && total_lines > 0 {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));

        let mut scrollbar_state = ScrollbarState::new(total_lines as usize).position(scroll_pos);

        let scrollbar_area = chunks[1].inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 0,
        });

        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

fn format_json_value_nicely(value: &serde_json::Value, indent: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let indent_str = "  ".repeat(indent);

    match value {
        serde_json::Value::String(s) => {
            lines.push(Line::from(Span::styled(
                format!("{}{}", indent_str, s),
                Style::default().fg(Color::Green),
            )));
        }
        serde_json::Value::Number(n) => {
            lines.push(Line::from(Span::styled(
                format!("{}{}", indent_str, n),
                Style::default().fg(Color::Yellow),
            )));
        }
        serde_json::Value::Bool(b) => {
            lines.push(Line::from(Span::styled(
                format!("{}{}", indent_str, b),
                Style::default().fg(Color::Cyan),
            )));
        }
        serde_json::Value::Null => {
            lines.push(Line::from(Span::styled(
                format!("{}null", indent_str),
                Style::default().fg(Color::DarkGray),
            )));
        }
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("{}[]", indent_str),
                    Style::default().fg(Color::White),
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    format!("{}[", indent_str),
                    Style::default().fg(Color::White),
                )));
                for (i, item) in arr.iter().enumerate() {
                    let mut item_lines = format_json_value_nicely(item, indent + 1);
                    if i < arr.len() - 1 {
                        if let Some(last_line) = item_lines.last_mut() {
                            // Add comma to last span of the line
                            if let Some(span) = last_line.spans.last() {
                                let text_with_comma = format!("{},", span.content);
                                let new_span = Span::styled(text_with_comma, span.style);
                                let mut new_spans: Vec<Span> =
                                    last_line.spans[..last_line.spans.len() - 1].to_vec();
                                new_spans.push(new_span);
                                *last_line = Line::from(new_spans);
                            }
                        }
                    }
                    lines.extend(item_lines);
                }
                lines.push(Line::from(Span::styled(
                    format!("{}]", indent_str),
                    Style::default().fg(Color::White),
                )));
            }
        }
        serde_json::Value::Object(obj) => {
            if obj.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("{}{{}}", indent_str),
                    Style::default().fg(Color::White),
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    format!("{}{{", indent_str),
                    Style::default().fg(Color::White),
                )));
                let entries: Vec<_> = obj.iter().collect();
                for (i, (key, val)) in entries.iter().enumerate() {
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("{}  ", indent_str),
                            Style::default().fg(Color::White),
                        ),
                        Span::styled(
                            format!("{}: ", key),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]));
                    let mut val_lines = format_json_value_nicely(val, indent + 2);
                    if !val_lines.is_empty() {
                        // Remove indent from first line since we already have the key
                        if let Some(first_line) = val_lines.first_mut() {
                            if let Some(first_span) = first_line.spans.first() {
                                let trimmed = first_span.content.trim_start();
                                let new_span = Span::styled(trimmed.to_string(), first_span.style);
                                let mut new_spans = vec![new_span];
                                new_spans.extend(first_line.spans[1..].to_vec());
                                *first_line = Line::from(new_spans);
                            }
                        }

                        // Merge key line with first value line
                        if let Some(first_val_line) = val_lines.first() {
                            if let Some(last_line) = lines.last_mut() {
                                last_line.spans.extend(first_val_line.spans.clone());
                            }
                            val_lines.remove(0);
                        }
                    }

                    if i < entries.len() - 1 {
                        // Add comma to the merged line
                        if let Some(last_line) = lines.last_mut() {
                            last_line
                                .spans
                                .push(Span::styled(",", Style::default().fg(Color::White)));
                        }
                    }
                    lines.extend(val_lines);
                }
                lines.push(Line::from(Span::styled(
                    format!("{}}}", indent_str),
                    Style::default().fg(Color::White),
                )));
            }
        }
    }

    lines
}

fn build_deployment_detail_content(
    app: &App,
    deployment: &env_defs::DeploymentResp,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();
    let mut current_idx = 0;

    // General section (index 0)
    if app.detail_browser_index == current_idx {
        // Show loading indicator when reloading deployment details
        if app.is_loading
            && matches!(
                app.pending_action,
                PendingAction::ReloadCurrentDeploymentDetail
            )
        {
            lines.push(Line::from(vec![
                Span::styled("⏳ ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    "Reloading deployment details...",
                    Style::default().fg(Color::Yellow),
                ),
            ]));
            lines.push(Line::from(""));
        }

        lines.push(Line::from(vec![
            Span::styled("Deployment ID: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                deployment.deployment_id.clone(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        let (status_icon, status_color) = match deployment.status.as_str() {
            "DEPLOYED" => ("✓", Color::Green),
            "FAILED" => ("✗", Color::Red),
            "IN_PROGRESS" => ("⏳", Color::Yellow),
            _ => ("•", Color::White),
        };

        lines.push(Line::from(vec![
            Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                status_icon,
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                deployment.status.clone(),
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        lines.push(Line::from(vec![
            Span::styled("Module: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                deployment.module.clone(),
                Style::default().fg(Color::Magenta),
            ),
        ]));

        lines.push(Line::from(vec![
            Span::styled("Version: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                deployment.module_version.clone(),
                Style::default().fg(Color::Yellow),
            ),
        ]));

        lines.push(Line::from(vec![
            Span::styled("Track: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                deployment.module_track.clone(),
                Style::default().fg(Color::Green),
            ),
        ]));

        lines.push(Line::from(vec![
            Span::styled("Type: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                deployment.module_type.clone(),
                Style::default().fg(Color::White),
            ),
        ]));

        lines.push(Line::from(vec![
            Span::styled("Environment: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                deployment.environment.clone(),
                Style::default().fg(Color::Blue),
            ),
        ]));

        lines.push(Line::from(vec![
            Span::styled("Region: ", Style::default().fg(Color::DarkGray)),
            Span::styled(deployment.region.clone(), Style::default().fg(Color::White)),
        ]));

        lines.push(Line::from(vec![
            Span::styled("Project ID: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                deployment.project_id.clone(),
                Style::default().fg(Color::White),
            ),
        ]));

        lines.push(Line::from(""));

        lines.push(Line::from(vec![
            Span::styled("Job ID: ", Style::default().fg(Color::DarkGray)),
            Span::styled(deployment.job_id.clone(), Style::default().fg(Color::Cyan)),
        ]));

        if !deployment.reference.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Reference: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    deployment.reference.clone(),
                    Style::default().fg(Color::White),
                ),
            ]));
        }

        if !deployment.initiated_by.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Initiated By: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    deployment.initiated_by.clone(),
                    Style::default().fg(Color::White),
                ),
            ]));
        }

        lines.push(Line::from(""));

        lines.push(Line::from(vec![
            Span::styled("CPU: ", Style::default().fg(Color::DarkGray)),
            Span::styled(deployment.cpu.clone(), Style::default().fg(Color::Yellow)),
        ]));

        lines.push(Line::from(vec![
            Span::styled("Memory: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                deployment.memory.clone(),
                Style::default().fg(Color::Yellow),
            ),
        ]));

        lines.push(Line::from(""));

        // Drift Detection subsection
        lines.push(Line::from(Span::styled(
            "Drift Detection",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "─".repeat(40),
            Style::default().fg(Color::DarkGray),
        )));

        let enabled_color = if deployment.drift_detection.enabled {
            Color::Green
        } else {
            Color::Red
        };

        lines.push(Line::from(vec![
            Span::styled("Enabled: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                deployment.drift_detection.enabled.to_string(),
                Style::default()
                    .fg(enabled_color)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        if deployment.drift_detection.enabled {
            lines.push(Line::from(vec![
                Span::styled("Interval: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    deployment.drift_detection.interval.clone(),
                    Style::default().fg(Color::White),
                ),
            ]));

            lines.push(Line::from(vec![
                Span::styled("Auto Remediate: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    deployment.drift_detection.auto_remediate.to_string(),
                    Style::default().fg(if deployment.drift_detection.auto_remediate {
                        Color::Green
                    } else {
                        Color::Red
                    }),
                ),
            ]));
        }

        lines.push(Line::from(""));

        if deployment.has_drifted {
            lines.push(Line::from(vec![
                Span::styled("⚠ ", Style::default().fg(Color::Red)),
                Span::styled(
                    "DRIFT DETECTED",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));
        }

        if deployment.deleted {
            lines.push(Line::from(vec![
                Span::styled("🗑 ", Style::default().fg(Color::Red)),
                Span::styled(
                    "DELETED",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));
        }

        if !deployment.error_text.is_empty() {
            lines.push(Line::from(Span::styled(
                "Error:",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
            for line in deployment.error_text.lines() {
                lines.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Red),
                )));
            }
            lines.push(Line::from(""));
        }

        // TF Resources subsection
        if let Some(ref resources) = deployment.tf_resources {
            if !resources.is_empty() {
                lines.push(Line::from(Span::styled(
                    "Terraform Resources",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(Span::styled(
                    "─".repeat(40),
                    Style::default().fg(Color::DarkGray),
                )));

                lines.push(Line::from(vec![
                    Span::styled("Total: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        resources.len().to_string(),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                lines.push(Line::from(""));

                for (idx, resource) in resources.iter().enumerate() {
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("  {}. ", idx + 1),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(resource.clone(), Style::default().fg(Color::Green)),
                    ]));
                }

                lines.push(Line::from(""));
            }
        }

        return lines;
    }
    current_idx += 1;

    // Variables section
    if !deployment.variables.is_null() && deployment.variables.is_object() {
        if let Some(obj) = deployment.variables.as_object() {
            if !obj.is_empty() {
                if app.detail_browser_index == current_idx {
                    for (key, value) in obj {
                        lines.push(Line::from(vec![
                            Span::styled("⚙ ", Style::default().fg(Color::Yellow)),
                            Span::styled(
                                key.clone(),
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]));

                        // Check if this is a structured variable object or just a raw value
                        if let Some(var_obj) = value.as_object() {
                            // Check if it has 'type' and 'value' fields (structured format)
                            if var_obj.contains_key("type") || var_obj.contains_key("value") {
                                // Get type
                                if let Some(type_val) = var_obj.get("type") {
                                    let type_str = match type_val {
                                        serde_json::Value::String(s) => s.clone(),
                                        _ => format!("{}", type_val),
                                    };
                                    lines.push(Line::from(vec![
                                        Span::raw("  Type: "),
                                        Span::styled(type_str, Style::default().fg(Color::Blue)),
                                    ]));
                                }

                                // Get sensitive flag
                                if let Some(sensitive_val) = var_obj.get("sensitive") {
                                    if let Some(is_sensitive) = sensitive_val.as_bool() {
                                        if is_sensitive {
                                            lines.push(Line::from(vec![
                                                Span::raw("  "),
                                                Span::styled(
                                                    "🔒 SENSITIVE",
                                                    Style::default()
                                                        .fg(Color::Red)
                                                        .add_modifier(Modifier::BOLD),
                                                ),
                                            ]));
                                        }
                                    }
                                }

                                lines.push(Line::from(""));

                                // Get value
                                if let Some(val) = var_obj.get("value") {
                                    lines.push(Line::from(Span::styled(
                                        "  Value:",
                                        Style::default().fg(Color::DarkGray),
                                    )));

                                    // Use the nice formatter for the value
                                    let formatted_lines = format_json_value_nicely(val, 1);
                                    lines.extend(formatted_lines);
                                } else {
                                    // No 'value' field, show the whole object
                                    lines.push(Line::from(Span::styled(
                                        "  Value:",
                                        Style::default().fg(Color::DarkGray),
                                    )));
                                    let formatted_lines = format_json_value_nicely(value, 1);
                                    lines.extend(formatted_lines);
                                }
                            } else {
                                // Object but not in structured format, still show Type and Value labels
                                let type_str = "object";
                                lines.push(Line::from(vec![
                                    Span::raw("  Type: "),
                                    Span::styled(
                                        type_str.to_string(),
                                        Style::default().fg(Color::Blue),
                                    ),
                                ]));

                                lines.push(Line::from(""));
                                lines.push(Line::from(Span::styled(
                                    "  Value:",
                                    Style::default().fg(Color::DarkGray),
                                )));
                                let formatted_lines = format_json_value_nicely(value, 1);
                                lines.extend(formatted_lines);
                            }
                        } else {
                            // Not an object, infer type and show with labels
                            let type_str = match value {
                                serde_json::Value::String(_) => "string",
                                serde_json::Value::Number(_) => "number",
                                serde_json::Value::Bool(_) => "bool",
                                serde_json::Value::Null => "null",
                                serde_json::Value::Array(_) => "array",
                                serde_json::Value::Object(_) => "object",
                            };

                            lines.push(Line::from(vec![
                                Span::raw("  Type: "),
                                Span::styled(
                                    type_str.to_string(),
                                    Style::default().fg(Color::Blue),
                                ),
                            ]));

                            lines.push(Line::from(""));
                            lines.push(Line::from(Span::styled(
                                "  Value:",
                                Style::default().fg(Color::DarkGray),
                            )));
                            let formatted_lines = format_json_value_nicely(value, 1);
                            lines.extend(formatted_lines);
                        }

                        lines.push(Line::from(""));
                    }

                    return lines;
                }
                current_idx += 1;
            }
        }
    }

    // Outputs section
    if !deployment.output.is_null() && deployment.output.is_object() {
        if let Some(obj) = deployment.output.as_object() {
            if !obj.is_empty() {
                if app.detail_browser_index == current_idx {
                    for (key, value) in obj {
                        lines.push(Line::from(vec![
                            Span::styled("📦 ", Style::default().fg(Color::Cyan)),
                            Span::styled(
                                key.clone(),
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]));

                        // Extract type, value, and sensitive from the output object
                        if let Some(output_obj) = value.as_object() {
                            // Get type
                            if let Some(type_val) = output_obj.get("type") {
                                let type_str = match type_val {
                                    serde_json::Value::String(s) => s.clone(),
                                    _ => format!("{}", type_val),
                                };
                                lines.push(Line::from(vec![
                                    Span::raw("  Type: "),
                                    Span::styled(type_str, Style::default().fg(Color::Blue)),
                                ]));
                            }

                            // Get sensitive flag
                            if let Some(sensitive_val) = output_obj.get("sensitive") {
                                if let Some(is_sensitive) = sensitive_val.as_bool() {
                                    if is_sensitive {
                                        lines.push(Line::from(vec![
                                            Span::raw("  "),
                                            Span::styled(
                                                "🔒 SENSITIVE",
                                                Style::default()
                                                    .fg(Color::Red)
                                                    .add_modifier(Modifier::BOLD),
                                            ),
                                        ]));
                                    }
                                }
                            }

                            lines.push(Line::from(""));

                            // Get value
                            if let Some(val) = output_obj.get("value") {
                                lines.push(Line::from(Span::styled(
                                    "  Value:",
                                    Style::default().fg(Color::DarkGray),
                                )));

                                // Use the nice formatter for the value
                                let formatted_lines = format_json_value_nicely(val, 1);
                                lines.extend(formatted_lines);
                            }
                        } else {
                            // Fallback if output is not in expected format
                            let type_str = match value {
                                serde_json::Value::String(_) => "string",
                                serde_json::Value::Number(_) => "number",
                                serde_json::Value::Bool(_) => "bool",
                                serde_json::Value::Null => "null",
                                serde_json::Value::Array(_) => "array",
                                serde_json::Value::Object(_) => "object",
                            };

                            lines.push(Line::from(vec![
                                Span::raw("  Type: "),
                                Span::styled(
                                    type_str.to_string(),
                                    Style::default().fg(Color::Blue),
                                ),
                            ]));

                            lines.push(Line::from(""));
                            lines.push(Line::from(Span::styled(
                                "  Value:",
                                Style::default().fg(Color::DarkGray),
                            )));

                            let formatted_lines = format_json_value_nicely(value, 1);
                            lines.extend(formatted_lines);
                        }

                        lines.push(Line::from(""));
                    }

                    return lines;
                }
                current_idx += 1;
            }
        }
    }

    // Dependencies section
    if !deployment.dependencies.is_empty() {
        if app.detail_browser_index == current_idx {
            for dep in &deployment.dependencies {
                lines.push(Line::from(vec![
                    Span::styled("• ", Style::default().fg(Color::Blue)),
                    Span::styled(
                        dep.deployment_id.clone(),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));

                lines.push(Line::from(vec![
                    Span::raw("  Environment: "),
                    Span::styled(dep.environment.clone(), Style::default().fg(Color::White)),
                ]));

                lines.push(Line::from(vec![
                    Span::raw("  Region: "),
                    Span::styled(dep.region.clone(), Style::default().fg(Color::White)),
                ]));

                lines.push(Line::from(vec![
                    Span::raw("  Project: "),
                    Span::styled(dep.project_id.clone(), Style::default().fg(Color::White)),
                ]));

                lines.push(Line::from(""));
            }

            return lines;
        }
        current_idx += 1;
    }

    // Policy Results section
    if !deployment.policy_results.is_empty() {
        if app.detail_browser_index == current_idx {
            for result in &deployment.policy_results {
                let json_str = serde_json::to_string_pretty(result)
                    .unwrap_or_else(|_| format!("{:?}", result));

                for line in json_str.lines() {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(Color::White),
                    )));
                }
                lines.push(Line::from(""));
            }

            return lines;
        }
        current_idx += 1;
    }

    // Logs section
    if app.detail_browser_index == current_idx {
        lines.push(Line::from(vec![
            Span::styled("Job ID: ", Style::default().fg(Color::DarkGray)),
            Span::styled(deployment.job_id.clone(), Style::default().fg(Color::Cyan)),
        ]));
        lines.push(Line::from(""));

        // Check if we're currently loading logs for this job
        let is_loading =
            app.is_loading && matches!(app.pending_action, PendingAction::LoadJobLogs(_));

        let current_job_id = &app.events_current_job_id;
        let logs = &app.events_logs;

        if is_loading && current_job_id == &deployment.job_id {
            lines.push(Line::from(vec![
                Span::styled("⏳ ", Style::default().fg(Color::Yellow)),
                Span::styled("Loading logs...", Style::default().fg(Color::Yellow)),
            ]));
        } else if current_job_id != &deployment.job_id {
            // Different job - logs will be loaded automatically when this section is viewed
            lines.push(Line::from(vec![
                Span::styled("ℹ ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    "Logs are being loaded...",
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        } else if logs.is_empty() {
            // Current job but no logs
            lines.push(Line::from(vec![
                Span::styled("ℹ ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    "No logs found for this job",
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "This could mean:",
                Style::default().fg(Color::DarkGray),
            )]));
            lines.push(Line::from(vec![Span::styled(
                "  • The job hasn't generated any logs yet",
                Style::default().fg(Color::DarkGray),
            )]));
            lines.push(Line::from(vec![Span::styled(
                "  • Logs have expired or been cleaned up",
                Style::default().fg(Color::DarkGray),
            )]));
            lines.push(Line::from(vec![Span::styled(
                "  • The job is still running",
                Style::default().fg(Color::DarkGray),
            )]));
        } else {
            for log in logs.iter() {
                // Split multi-line log messages
                for line in log.message.lines() {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(Color::White),
                    )));
                }
            }
        }

        return lines;
    }

    lines
}

fn render_stack_detail(frame: &mut Frame, area: Rect, app: &mut App, stack: &env_defs::ModuleResp) {
    // Create two-pane layout: left for navigation tree, right for details
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30), // Left: Navigation tree
            Constraint::Percentage(70), // Right: Details
        ])
        .split(area);

    // Render navigation tree (left pane) using the structured NavItems
    let nav_list_items: Vec<ListItem> = app
        .detail_nav_items
        .iter()
        .enumerate()
        .map(|(idx, nav_item)| {
            let style = if idx == app.detail_browser_index {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(nav_item.display_string(), style)))
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

    // Build content for the selected browser item
    let lines = build_stack_detail_content(app, stack);

    // Update total lines count
    app.detail_total_lines = lines.len() as u16;

    // Calculate scroll limits
    let max_scroll = app.get_max_detail_scroll();
    if app.detail_scroll > max_scroll {
        app.detail_scroll = max_scroll;
    }

    // Get the dynamic title from the selected nav item
    let detail_title = if app.detail_browser_index < app.detail_nav_items.len() {
        format!(
            " 🚚 {} ",
            app.detail_nav_items[app.detail_browser_index].title()
        )
    } else {
        " 🚚 Stack Details ".to_string()
    };

    // Use white border for focused pane, magenta for unfocused
    let detail_border_color = if app.detail_focus_right {
        Color::White
    } else {
        Color::Magenta
    };

    // Create the detail paragraph
    let mut paragraph = Paragraph::new(lines.clone())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(detail_border_color))
                .title(Span::styled(
                    detail_title,
                    Style::default()
                        .fg(detail_border_color)
                        .add_modifier(Modifier::BOLD),
                )),
        )
        .scroll((app.detail_scroll, 0));

    // Apply wrapping if enabled
    if app.detail_wrap_text {
        paragraph = paragraph.wrap(Wrap { trim: false });
    }

    frame.render_widget(paragraph, chunks[1]);

    // Render scrollbar on the detail pane when focused and there's content
    if app.detail_focus_right && app.detail_total_lines > 0 {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));

        let mut scrollbar_state =
            ScrollbarState::new(max_scroll as usize).position(app.detail_scroll as usize);

        frame.render_stateful_widget(scrollbar, chunks[1], &mut scrollbar_state);
    }
}

fn build_stack_detail_content(app: &App, stack: &env_defs::ModuleResp) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();
    let current_idx = app.detail_browser_index;

    // Get the current nav item to determine what to show
    if current_idx >= app.detail_nav_items.len() {
        return lines;
    }

    let nav_item = &app.detail_nav_items[current_idx];

    match nav_item {
        NavItem::General => {
            render_stack_general(stack, &mut lines);
        }
        NavItem::Composition => {
            render_stack_composition(stack, &mut lines);
        }
        NavItem::VariablesHeader => {
            render_all_variables(stack, &mut lines);
        }
        NavItem::VariableFolder { module_name } => {
            render_variable_folder_message(module_name, &mut lines);
        }
        NavItem::Variable { name, .. } => {
            render_variable_detail(stack, name, &mut lines);
        }
        NavItem::OutputsHeader => {
            render_all_outputs(stack, &mut lines);
        }
        NavItem::OutputFolder { module_name } => {
            render_output_folder_message(module_name, &mut lines);
        }
        NavItem::Output { name, .. } => {
            render_output_detail(stack, name, &mut lines);
        }
        _ => {
            lines.push(Line::from(Span::styled(
                "Section not implemented",
                Style::default().fg(Color::Yellow),
            )));
        }
    }

    lines
}

// Helper functions for rendering each section

fn render_stack_general(stack: &env_defs::ModuleResp, lines: &mut Vec<Line<'static>>) {
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
        Span::styled("Stack: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            stack.module_name.clone(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Version: ", Style::default().fg(Color::DarkGray)),
        Span::styled(stack.version.clone(), Style::default().fg(Color::Yellow)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Track: ", Style::default().fg(Color::DarkGray)),
        Span::styled(stack.track.clone(), Style::default().fg(Color::Green)),
    ]));

    lines.push(Line::from(""));

    if !stack.description.is_empty() {
        lines.push(Line::from(Span::styled(
            "Description:",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(Span::styled(
            stack.description.clone(),
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
            stack.tf_variables.len().to_string(),
            Style::default().fg(Color::Yellow),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::raw("  • Outputs: "),
        Span::styled(
            stack.tf_outputs.len().to_string(),
            Style::default().fg(Color::Cyan),
        ),
    ]));

    if let Some(stack_data) = &stack.stack_data {
        lines.push(Line::from(vec![
            Span::raw("  • Composition: "),
            Span::styled(
                stack_data.modules.len().to_string(),
                Style::default().fg(Color::Magenta),
            ),
        ]));
    }
}

fn render_stack_composition(stack: &env_defs::ModuleResp, lines: &mut Vec<Line<'static>>) {
    if let Some(stack_data) = &stack.stack_data {
        use std::collections::HashMap;
        let mut module_counts: HashMap<(String, String, String), usize> = HashMap::new();

        for module in &stack_data.modules {
            let key = (
                module.module.clone(),
                module.version.clone(),
                module.track.clone(),
            );
            *module_counts.entry(key).or_insert(0) += 1;
        }

        let mut sorted_modules: Vec<((String, String, String), usize)> =
            module_counts.into_iter().collect();
        sorted_modules.sort_by(|a, b| a.0.cmp(&b.0));

        for (i, ((module_name, version, track), count)) in sorted_modules.iter().enumerate() {
            if i > 0 {
                lines.push(Line::from(""));
            }

            let title = if *count > 1 {
                format!("{} (x{})", to_camel_case(module_name), count)
            } else {
                to_camel_case(module_name)
            };

            lines.push(Line::from(Span::styled(
                title,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));

            lines.push(Line::from(vec![
                Span::styled("  Module: ", Style::default().fg(Color::DarkGray)),
                Span::styled(module_name.clone(), Style::default().fg(Color::Magenta)),
            ]));

            lines.push(Line::from(vec![
                Span::styled("  Version: ", Style::default().fg(Color::DarkGray)),
                Span::styled(version.clone(), Style::default().fg(Color::Green)),
            ]));

            lines.push(Line::from(vec![
                Span::styled("  Track: ", Style::default().fg(Color::DarkGray)),
                Span::styled(track.clone(), Style::default().fg(Color::Yellow)),
            ]));
        }
    }
}

fn render_all_variables(stack: &env_defs::ModuleResp, lines: &mut Vec<Line<'static>>) {
    lines.push(Line::from(Span::styled(
        "🔧 Stack Variables",
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "═".repeat(60),
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));

    for (i, variable) in stack.tf_variables.iter().enumerate() {
        if i > 0 {
            lines.push(Line::from(""));
        }

        let is_required = variable.required();
        let bullet = if is_required {
            Span::styled("⚠ ", Style::default().fg(Color::Red))
        } else {
            Span::styled("• ", Style::default().fg(Color::Yellow))
        };

        let parts: Vec<&str> = variable.name.split("__").collect();
        if parts.len() >= 2 {
            let module_name = parts[0];
            let var_name = parts[1..].join("__");

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
                Span::styled(
                    to_camel_case(module_name),
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" / ", Style::default().fg(Color::DarkGray)),
                Span::styled(to_camel_case(&var_name), name_style),
            ]));
        } else {
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
                Span::styled(to_camel_case(&variable.name), name_style),
            ]));
        }

        let type_str = match &variable._type {
            serde_json::Value::String(s) => s.clone(),
            _ => format!("{}", variable._type),
        };
        lines.push(Line::from(vec![
            Span::styled("  Type: ", Style::default().fg(Color::DarkGray)),
            Span::styled(type_str, Style::default().fg(Color::Blue)),
        ]));

        if !variable.description.is_empty() {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    variable.description.clone(),
                    Style::default().fg(Color::White),
                ),
            ]));
        }

        if let Some(default_value) = &variable.default {
            lines.push(Line::from(Span::styled(
                "  Default:",
                Style::default().fg(Color::DarkGray),
            )));
            let formatted_lines = format_json_value_nicely(default_value, 1);
            lines.extend(formatted_lines);
        } else if is_required {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "⚠ REQUIRED",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
            ]));
        }

        if variable.sensitive {
            lines.push(Line::from(vec![
                Span::styled("  Sensitive: ", Style::default().fg(Color::DarkGray)),
                Span::styled("true", Style::default().fg(Color::Red)),
            ]));
        }
    }
}

fn render_variable_folder_message(module_name: &str, lines: &mut Vec<Line<'static>>) {
    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(
            "Select a variable to see details for {}",
            to_camel_case(module_name)
        ),
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    )));
}

fn render_variable_detail(
    stack: &env_defs::ModuleResp,
    var_name: &str,
    lines: &mut Vec<Line<'static>>,
) {
    // Find the variable by name
    if let Some(variable) = stack.tf_variables.iter().find(|v| v.name == var_name) {
        let is_required = variable.required();
        let icon = if is_required { "⚠ " } else { "🔧 " };

        let parts: Vec<&str> = variable.name.split("__").collect();
        if parts.len() >= 2 {
            let module_name = parts[0];
            let var_name = parts[1..].join("__");

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
                    to_camel_case(module_name),
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" / ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    to_camel_case(&var_name),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        } else {
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
                    to_camel_case(&variable.name),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }

        lines.push(Line::from(Span::styled(
            "═".repeat(60),
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));

        let type_str = match &variable._type {
            serde_json::Value::String(s) => s.clone(),
            _ => format!("{}", variable._type),
        };
        lines.push(Line::from(vec![
            Span::styled("Type: ", Style::default().fg(Color::DarkGray)),
            Span::styled(type_str, Style::default().fg(Color::Blue)),
        ]));

        if !variable.description.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Description:",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                variable.description.clone(),
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

        if let Some(default_value) = &variable.default {
            let default_str = match default_value {
                serde_json::Value::String(s) => format!("\"{}\"", s),
                serde_json::Value::Null => "null".to_string(),
                other => {
                    serde_json::to_string_pretty(other).unwrap_or_else(|_| format!("{}", other))
                }
            };
            lines.push(Line::from(Span::styled(
                "Default Value:",
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
                if variable.nullable { "Yes" } else { "No" },
                if variable.nullable {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Red)
                },
            ),
        ]));

        lines.push(Line::from(vec![
            Span::raw("  • Sensitive: "),
            Span::styled(
                if variable.sensitive { "Yes ⚠" } else { "No" },
                if variable.sensitive {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::Green)
                },
            ),
        ]));
    }
}

fn render_all_outputs(stack: &env_defs::ModuleResp, lines: &mut Vec<Line<'static>>) {
    lines.push(Line::from(Span::styled(
        "📤 Stack Outputs",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "═".repeat(60),
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));

    for (i, output) in stack.tf_outputs.iter().enumerate() {
        if i > 0 {
            lines.push(Line::from(""));
        }

        let parts: Vec<&str> = output.name.split("__").collect();
        if parts.len() >= 2 {
            let module_name = parts[0];
            let output_name = parts[1..].join("__");

            lines.push(Line::from(vec![
                Span::styled("• ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    to_camel_case(module_name),
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" / ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    to_camel_case(&output_name),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("• ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    to_camel_case(&output.name),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }

        if !output.description.is_empty() {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    output.description.clone(),
                    Style::default().fg(Color::White),
                ),
            ]));
        }
    }
}

fn render_output_folder_message(module_name: &str, lines: &mut Vec<Line<'static>>) {
    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(
            "Select an output to see details for {}",
            to_camel_case(module_name)
        ),
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    )));
}

fn render_output_detail(
    stack: &env_defs::ModuleResp,
    output_name: &str,
    lines: &mut Vec<Line<'static>>,
) {
    // Find the output by name
    if let Some(output) = stack.tf_outputs.iter().find(|o| o.name == output_name) {
        let parts: Vec<&str> = output.name.split("__").collect();
        if parts.len() >= 2 {
            let module_name = parts[0];
            let out_name = parts[1..].join("__");

            lines.push(Line::from(vec![
                Span::styled("📊 ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    to_camel_case(module_name),
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" / ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    to_camel_case(&out_name),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("📊 ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    to_camel_case(&output.name),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }

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
        }
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
            let is_required = var.required();
            (!is_required, var.name.clone()) // Sort by required (reversed), then by name
        });

        for var in sorted_vars {
            let camel_case = to_camel_case(&var.name);
            let is_required = var.required();
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

// Removed duplicate to_camel_case and is_variable_required - now using versions from utils module

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

        // Show deprecation warning if deprecated
        if module.deprecated {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "⚠️  WARNING: DEPRECATED",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
            if let Some(msg) = &module.deprecated_message {
                lines.push(Line::from(vec![
                    Span::styled("Reason: ", Style::default().fg(Color::Yellow)),
                    Span::styled(msg.clone(), Style::default().fg(Color::White)),
                ]));
            }
        }

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
            let is_required = var.required();
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
                let is_required = var.required();
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
                let is_required = var.required();
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
