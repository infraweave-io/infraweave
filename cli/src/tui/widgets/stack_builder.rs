use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::tui::state::stack_builder_state::{
    ReferencePickerStep, StackBuilderPage, StackBuilderState,
};

/// Render the stack builder view
pub fn render_stack_builder(f: &mut Frame, area: Rect, state: &mut StackBuilderState) {
    // If reference picker is showing, render it on top
    if state.showing_reference_picker {
        render_reference_picker_modal(f, area, state);
        return;
    }

    // If module modal is showing, render it on top
    if state.showing_module_modal {
        render_module_modal(f, area, state);
        return;
    }

    match state.current_page {
        StackBuilderPage::ModuleList => render_module_list_page(f, area, state),
        StackBuilderPage::VariableConfiguration => {
            render_variable_configuration_page(f, area, state)
        }
        StackBuilderPage::Preview => render_preview_page(f, area, state),
    }
}

/// Render the module selection modal
fn render_module_modal(f: &mut Frame, area: Rect, state: &mut StackBuilderState) {
    // Create a centered modal
    let modal_width = area.width.saturating_sub(10).min(80);
    let modal_height = area.height.saturating_sub(4).min(25);
    let modal_x = (area.width.saturating_sub(modal_width)) / 2;
    let modal_y = (area.height.saturating_sub(modal_height)) / 2;

    let modal_area = Rect {
        x: area.x + modal_x,
        y: area.y + modal_y,
        width: modal_width,
        height: modal_height,
    };

    // Clear the background
    let bg = Block::default().style(Style::default().bg(Color::Black));
    f.render_widget(bg, area);

    // Modal block
    let modal_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(vec![
            Span::raw(" "),
            Span::styled("📦 ", Style::default().fg(Color::Yellow)),
            Span::styled(
                "Select Module",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]);

    let inner = modal_block.inner(modal_area);
    f.render_widget(modal_block, modal_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Instance name input (when editing)
            Constraint::Min(5),    // Module list
            Constraint::Length(3), // Help
        ])
        .split(inner);

    // Instance name input (shown after module is selected)
    if state.editing_instance_name {
        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" Instance Name ");

        let input_inner = input_block.inner(chunks[0]);
        f.render_widget(input_block, chunks[0]);

        let input_text = if state.instance_name_input.is_empty() {
            Span::styled(
                "<enter instance name>",
                Style::default().fg(Color::DarkGray),
            )
        } else {
            Span::raw(&state.instance_name_input)
        };

        let para = Paragraph::new(input_text);
        f.render_widget(para, input_inner);
    } else {
        let hint_para = Paragraph::new("Press Enter on a module to select it")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        f.render_widget(hint_para, chunks[0]);
    }

    // Module list
    let list_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(format!(" Modules ({}) ", state.available_modules.len()));

    let list_inner = list_block.inner(chunks[1]);
    f.render_widget(list_block, chunks[1]);

    if state.available_modules.is_empty() {
        let empty_text =
            Paragraph::new("No modules available").style(Style::default().fg(Color::DarkGray));
        f.render_widget(empty_text, list_inner);
    } else {
        // Calculate visible area height for scrolling and update scroll offset
        let visible_height = list_inner.height as usize;
        state.update_modal_scroll(visible_height);
        let scroll_offset = state.modal_scroll_offset as usize;

        let items: Vec<ListItem> = state
            .available_modules
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_height)
            .map(|(i, module)| {
                let is_selected = i == state.modal_selected_index;
                let style = if is_selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let content = format!(
                    "{} (v{}) - {}",
                    module.module_name, module.version, module.module
                );
                ListItem::new(content).style(style)
            })
            .collect();

        let list = List::new(items);
        f.render_widget(list, list_inner);
    }

    // Help text
    let help_lines = if state.editing_instance_name {
        vec![Line::from(vec![
            Span::styled(
                "Enter",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Confirm  "),
            Span::styled(
                "Esc",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Cancel"),
        ])]
    } else {
        vec![Line::from(vec![
            Span::styled(
                "↑/↓",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Navigate  "),
            Span::styled(
                "PgUp/PgDn",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Jump  "),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Select  "),
            Span::styled(
                "Esc",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Cancel"),
        ])]
    };

    let help_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let help_inner = help_block.inner(chunks[2]);
    f.render_widget(help_block, chunks[2]);

    let help = Paragraph::new(help_lines).alignment(Alignment::Center);
    f.render_widget(help, help_inner);
}

/// Render the reference picker modal
fn render_reference_picker_modal(f: &mut Frame, area: Rect, state: &StackBuilderState) {
    // Create a centered modal
    let modal_width = area.width.saturating_sub(10).min(80);
    let modal_height = area.height.saturating_sub(4).min(30);
    let modal_x = (area.width.saturating_sub(modal_width)) / 2;
    let modal_y = (area.height.saturating_sub(modal_height)) / 2;

    let modal_area = Rect {
        x: area.x + modal_x,
        y: area.y + modal_y,
        width: modal_width,
        height: modal_height,
    };

    // Clear the background
    let bg = Block::default().style(Style::default().bg(Color::Black));
    f.render_widget(bg, area);

    // Modal block
    let title = match state.reference_picker_step {
        ReferencePickerStep::SelectInstance => " Select Module Instance ",
        ReferencePickerStep::SelectOutput => " Select Output Field ",
    };

    let modal_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(vec![
            Span::raw(" "),
            Span::styled("🔗 ", Style::default().fg(Color::Yellow)),
            Span::styled(
                title,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]);

    let inner = modal_block.inner(modal_area);
    f.render_widget(modal_block, modal_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),    // List
            Constraint::Length(3), // Help
        ])
        .split(inner);

    // Render list based on current step
    match state.reference_picker_step {
        ReferencePickerStep::SelectInstance => {
            render_instance_list_for_reference(f, chunks[0], state);
        }
        ReferencePickerStep::SelectOutput => {
            render_output_list_for_reference(f, chunks[0], state);
        }
    }

    // Help text
    let help_lines = match state.reference_picker_step {
        ReferencePickerStep::SelectInstance => vec![Line::from(vec![
            Span::styled(
                "Tab/Shift+Tab",
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
            Span::raw(": Select  "),
            Span::styled(
                "Esc",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Cancel"),
        ])],
        ReferencePickerStep::SelectOutput => vec![Line::from(vec![
            Span::styled(
                "Tab/Shift+Tab",
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
            Span::raw(": Insert  "),
            Span::styled(
                "Backspace/Esc",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Back"),
        ])],
    };

    let help_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let help_inner = help_block.inner(chunks[1]);
    f.render_widget(help_block, chunks[1]);

    let help = Paragraph::new(help_lines).alignment(Alignment::Center);
    f.render_widget(help, help_inner);
}

fn render_instance_list_for_reference(f: &mut Frame, area: Rect, state: &StackBuilderState) {
    // Filter out the current instance (can't reference itself)
    let available_instances: Vec<_> = state
        .module_instances
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != state.current_instance_index)
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(format!(
            " Module Instances ({}) ",
            available_instances.len()
        ));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if available_instances.is_empty() {
        let empty_text = Paragraph::new("No other module instances available")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(empty_text, inner);
    } else {
        let items: Vec<ListItem> = available_instances
            .iter()
            .enumerate()
            .map(|(display_idx, (_, instance))| {
                let is_selected = display_idx == state.reference_selected_instance_index;
                let style = if is_selected {
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let content = format!(
                    "{} ({}) v{}",
                    instance.instance_name, instance.module_name, instance.version
                );
                ListItem::new(content).style(style)
            })
            .collect();

        let list = List::new(items);
        f.render_widget(list, inner);
    }
}

fn render_output_list_for_reference(f: &mut Frame, area: Rect, state: &StackBuilderState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Output Fields ");

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Get filtered instances (excluding current)
    let available_instances: Vec<_> = state
        .module_instances
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != state.current_instance_index)
        .collect();

    if let Some((_, instance)) = available_instances.get(state.reference_selected_instance_index) {
        // Find the module to get its outputs
        if let Some(module) = state
            .available_modules
            .iter()
            .find(|m| m.module_name == instance.module_name && m.version == instance.version)
        {
            if module.tf_outputs.is_empty() {
                let empty_text = Paragraph::new("No outputs available for this module")
                    .style(Style::default().fg(Color::DarkGray));
                f.render_widget(empty_text, inner);
            } else {
                let items: Vec<ListItem> = module
                    .tf_outputs
                    .iter()
                    .enumerate()
                    .map(|(i, output)| {
                        let is_selected = i == state.reference_selected_output_index;
                        let style = if is_selected {
                            Style::default()
                                .fg(Color::Magenta)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };

                        let content = format!(
                            "{} - {}",
                            output.name,
                            if output.description.is_empty() {
                                "No description"
                            } else {
                                &output.description
                            }
                        );
                        ListItem::new(content).style(style)
                    })
                    .collect();

                let list = List::new(items);
                f.render_widget(list, inner);
            }
        }
    }
}

/// Render the module list page
fn render_module_list_page(f: &mut Frame, area: Rect, state: &StackBuilderState) {
    let main_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(vec![
            Span::raw(" "),
            Span::styled("🏗️  ", Style::default().fg(Color::Yellow)),
            Span::styled(
                "Stack Builder",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]);

    let inner = main_block.inner(area);
    f.render_widget(main_block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Stack name
            Constraint::Min(5),    // Module instances list
            Constraint::Length(5), // Help text
        ])
        .split(inner);

    // Stack name input
    render_stack_name_input(f, chunks[0], state);

    // List of added module instances
    render_module_instances(f, chunks[1], state);

    // Help text
    render_module_list_help(f, chunks[2], state);
}

fn render_stack_name_input(f: &mut Frame, area: Rect, state: &StackBuilderState) {
    let border_style = if state.editing_stack_name {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(" Stack Name ");

    let inner = block.inner(area);
    f.render_widget(block, area);

    let text = if state.stack_name.is_empty() {
        Span::styled("<enter stack name>", Style::default().fg(Color::DarkGray))
    } else {
        Span::raw(&state.stack_name)
    };

    let para = Paragraph::new(text);
    f.render_widget(para, inner);
}

fn render_module_instances(f: &mut Frame, area: Rect, state: &StackBuilderState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(format!(
            " Module Instances ({}) ",
            state.module_instances.len()
        ));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if state.module_instances.is_empty() {
        let empty_text = Paragraph::new(vec![
            Line::from(Span::styled(
                "No module instances added yet.",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(vec![
                Span::raw("Press "),
                Span::styled(
                    "Ctrl+A",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" to add a module"),
            ]),
        ])
        .alignment(Alignment::Center);
        f.render_widget(empty_text, inner);
    } else {
        let items: Vec<ListItem> = state
            .module_instances
            .iter()
            .enumerate()
            .map(|(i, instance)| {
                let is_selected = i == state.selected_instance_index;
                let style = if is_selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let content = format!(
                    "{}. {} ({}) v{}",
                    i + 1,
                    instance.instance_name,
                    instance.module_name,
                    instance.version
                );
                ListItem::new(content).style(style)
            })
            .collect();

        let list = List::new(items);
        f.render_widget(list, inner);
    }
}

fn render_module_list_help(f: &mut Frame, area: Rect, _state: &StackBuilderState) {
    let help_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Help ");

    let inner = help_block.inner(area);
    f.render_widget(help_block, area);

    let help_lines = vec![
        Line::from(vec![
            Span::styled(
                "Ctrl+A",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Add Module  "),
            Span::styled(
                "Tab",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Toggle Panes  "),
            Span::styled(
                "↑/↓",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Navigate  "),
        ]),
        Line::from(vec![
            Span::styled(
                "Ctrl+D",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Delete Selected  "),
            Span::styled(
                "Ctrl+N",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Next Page  "),
            Span::styled(
                "Esc",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Cancel"),
        ]),
    ];

    let help = Paragraph::new(help_lines).alignment(Alignment::Center);
    f.render_widget(help, inner);
}

/// Render the variable configuration page
fn render_variable_configuration_page(f: &mut Frame, area: Rect, state: &StackBuilderState) {
    let main_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(vec![
            Span::raw(" "),
            Span::styled("🔧 ", Style::default().fg(Color::Yellow)),
            Span::styled(
                "Stack Builder - Variable Configuration",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]);

    let inner = main_block.inner(area);
    f.render_widget(main_block, area);

    if state.module_instances.is_empty() {
        let error_text = Paragraph::new("No module instances to configure.")
            .style(Style::default().fg(Color::Red));
        f.render_widget(error_text, inner);
        return;
    }

    // Split into sidebar (instances overview) and main area (variables)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(35), // Sidebar for instances overview
            Constraint::Min(40),    // Main area for variables
        ])
        .split(inner);

    // Render instances overview sidebar
    render_instances_overview(f, main_chunks[0], state);

    // Right side: current instance variables and help
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Current instance header
            Constraint::Min(5),    // Variables list
            Constraint::Length(5), // Help text
        ])
        .split(main_chunks[1]);

    // Current instance header
    if let Some(instance) = state.module_instances.get(state.current_instance_index) {
        let instance_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" Configuring ");

        let instance_inner = instance_block.inner(right_chunks[0]);
        f.render_widget(instance_block, right_chunks[0]);

        let instance_text = format!(
            "{} ({}) v{}",
            instance.instance_name, instance.module_name, instance.version
        );
        let para = Paragraph::new(instance_text).style(Style::default().fg(Color::White));
        f.render_widget(para, instance_inner);
    }

    // Variables list
    render_variables_list(f, right_chunks[1], state);

    // Help text
    render_variable_config_help(f, right_chunks[2], state);
}

fn render_instances_overview(f: &mut Frame, area: Rect, state: &StackBuilderState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Module Instances ");

    let inner = block.inner(area);
    f.render_widget(block, area);

    let items: Vec<ListItem> = state
        .module_instances
        .iter()
        .enumerate()
        .map(|(i, instance)| {
            let is_current = i == state.current_instance_index;

            // Count configured vs total variables
            let total_vars = instance.variable_inputs.len();
            let configured_vars = instance
                .variable_inputs
                .iter()
                .filter(|v| !v.user_value.is_empty() || v.default_value.is_some())
                .count();

            let (prefix, style) = if is_current {
                (
                    "▶ ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )
            } else if configured_vars == total_vars && total_vars > 0 {
                ("✓ ", Style::default().fg(Color::Green))
            } else if configured_vars > 0 {
                ("◐ ", Style::default().fg(Color::Yellow))
            } else {
                ("○ ", Style::default().fg(Color::DarkGray))
            };

            let content = vec![
                Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(&instance.instance_name, style),
                ]),
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        format!("{}/{} vars", configured_vars, total_vars),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
            ];

            ListItem::new(content).style(style)
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}

fn render_variables_list(f: &mut Frame, area: Rect, state: &StackBuilderState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Variables ");

    let inner = block.inner(area);
    f.render_widget(block, area);

    if let Some(instance) = state.module_instances.get(state.current_instance_index) {
        if instance.variable_inputs.is_empty() {
            let no_vars = Paragraph::new("No variables to configure.")
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(no_vars, inner);
            return;
        }

        let items: Vec<ListItem> = instance
            .variable_inputs
            .iter()
            .enumerate()
            .map(|(i, var)| {
                let is_selected = i == state.selected_variable_index;
                let required_marker = if var.is_required { "*" } else { " " };

                let (value_display, value_style) = if var.user_value.is_empty() {
                    if let Some(default) = &var.default_value {
                        (
                            format!("(default: {})", default),
                            Style::default().fg(Color::DarkGray),
                        )
                    } else if var.is_required {
                        ("<REQUIRED>".to_string(), Style::default().fg(Color::Red))
                    } else {
                        (
                            "<not set>".to_string(),
                            Style::default().fg(Color::DarkGray),
                        )
                    }
                } else {
                    (var.user_value.clone(), Style::default().fg(Color::White))
                };

                let style = if is_selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    value_style
                };

                let content = format!("{}{}: {}", required_marker, var.name, value_display);
                ListItem::new(content).style(style)
            })
            .collect();

        let list = List::new(items);
        f.render_widget(list, inner);
    }
}

fn render_variable_config_help(f: &mut Frame, area: Rect, _state: &StackBuilderState) {
    let help_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Help ");

    let inner = help_block.inner(area);
    f.render_widget(help_block, area);

    let help_lines = vec![
        Line::from(vec![
            Span::styled(
                "↑/↓",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Navigate  "),
            Span::styled(
                "←/→",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Switch Instance  "),
            Span::styled(
                "Ctrl+R",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Insert Reference  "),
        ]),
        Line::from(vec![
            Span::styled(
                "Ctrl+N",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Preview  "),
            Span::styled(
                "Ctrl+B",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Back"),
        ]),
    ];

    let help = Paragraph::new(help_lines).alignment(Alignment::Left);
    f.render_widget(help, inner);
}

/// Render the preview page
fn render_preview_page(f: &mut Frame, area: Rect, state: &StackBuilderState) {
    let main_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(vec![
            Span::raw(" "),
            Span::styled("📋 ", Style::default().fg(Color::Yellow)),
            Span::styled(
                "Stack Builder - Preview",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]);

    let inner = main_block.inner(area);
    f.render_widget(main_block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),    // YAML preview
            Constraint::Length(5), // Help
        ])
        .split(inner);

    // YAML preview
    let yaml_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Generated Stack YAML ");

    let yaml_inner = yaml_block.inner(chunks[0]);
    f.render_widget(yaml_block, chunks[0]);

    let yaml_lines: Vec<Line> = state
        .generated_yaml
        .lines()
        .skip(state.preview_scroll as usize)
        .map(|line| {
            if line.starts_with("apiVersion:") || line.starts_with("kind:") {
                Line::from(Span::styled(line, Style::default().fg(Color::Cyan)))
            } else if line.starts_with("metadata:") || line.starts_with("spec:") {
                Line::from(Span::styled(line, Style::default().fg(Color::Yellow)))
            } else if line.trim().starts_with("{{") {
                Line::from(Span::styled(line, Style::default().fg(Color::Magenta)))
            } else {
                Line::from(line)
            }
        })
        .collect();

    let yaml_para = Paragraph::new(yaml_lines).wrap(Wrap { trim: false });
    f.render_widget(yaml_para, yaml_inner);

    // Help
    render_preview_help(f, chunks[1], state);
}

fn render_preview_help(f: &mut Frame, area: Rect, _state: &StackBuilderState) {
    let help_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Help ");

    let inner = help_block.inner(area);
    f.render_widget(help_block, area);

    let help_lines = vec![
        Line::from(vec![
            Span::styled(
                "↑/↓",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Scroll  "),
            Span::styled(
                "PgUp/PgDn",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Jump  "),
            Span::styled(
                "Ctrl+S",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Save  "),
            Span::styled(
                "Ctrl+Y",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Copy  "),
        ]),
        Line::from(vec![
            Span::styled(
                "Ctrl+B",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Back  "),
            Span::styled(
                "Esc",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Close"),
        ]),
    ];

    let help = Paragraph::new(help_lines).alignment(Alignment::Center);
    f.render_widget(help, inner);
}
