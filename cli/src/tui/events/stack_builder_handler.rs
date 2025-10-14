use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};

use crate::tui::app::App;
use crate::tui::state::stack_builder_state::StackBuilderPage;

pub struct StackBuilderHandler;

impl StackBuilderHandler {
    pub fn handle_key(app: &mut App, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        let state = &mut app.stack_builder_state;

        // If reference picker is showing, handle it first
        if state.showing_reference_picker {
            return handle_reference_picker_key(app, key, modifiers);
        }

        // If modal is showing, handle modal keys
        if state.showing_module_modal {
            return handle_modal_key(app, key, modifiers);
        }

        // Handle Ctrl key combinations first
        if modifiers.contains(KeyModifiers::CONTROL) {
            match key {
                KeyCode::Char('a') => {
                    // Add module (only on module list page)
                    if state.current_page == StackBuilderPage::ModuleList {
                        state.open_module_modal();
                    }
                    return Ok(());
                }
                KeyCode::Char('r') => {
                    // Open reference picker (only on variable configuration page)
                    if state.current_page == StackBuilderPage::VariableConfiguration {
                        state.open_reference_picker();
                    }
                    return Ok(());
                }
                KeyCode::Char('n') => {
                    // Move to next page
                    if let Err(err) = state.next_page() {
                        state.validation_error = Some(err);
                    }
                    return Ok(());
                }
                KeyCode::Char('b') => {
                    // Move to previous page (back)
                    state.previous_page();
                    return Ok(());
                }
                KeyCode::Char('d') => {
                    // Delete selected module instance (only on module list page)
                    if state.current_page == StackBuilderPage::ModuleList {
                        if state.selected_instance_index < state.module_instances.len() {
                            state.remove_module_instance(state.selected_instance_index);
                        }
                    }
                    return Ok(());
                }
                KeyCode::Char('s') => {
                    // Save to file (only on preview page)
                    if state.current_page == StackBuilderPage::Preview {
                        app.schedule_action(crate::tui::app::PendingAction::SaveStackToFile);
                    }
                    return Ok(());
                }
                KeyCode::Char('y') => {
                    // Copy to clipboard (only on preview page)
                    if state.current_page == StackBuilderPage::Preview {
                        match arboard::Clipboard::new() {
                            Ok(mut clipboard) => match clipboard.set_text(&state.generated_yaml) {
                                Ok(_) => {
                                    app.detail_state.show_message(
                                        "✅ Stack YAML copied to clipboard!".to_string(),
                                    );
                                }
                                Err(e) => {
                                    app.detail_state
                                        .show_error(&format!("Failed to copy to clipboard: {}", e));
                                }
                            },
                            Err(e) => {
                                app.detail_state
                                    .show_error(&format!("Failed to access clipboard: {}", e));
                            }
                        }
                    }
                    return Ok(());
                }
                _ => {}
            }
        }

        // Handle regular key presses based on current page
        match state.current_page {
            StackBuilderPage::ModuleList => {
                handle_module_list_key(app, key)?;
            }
            StackBuilderPage::VariableConfiguration => {
                handle_variable_configuration_key(app, key)?;
            }
            StackBuilderPage::Preview => {
                handle_preview_key(app, key)?;
            }
        }

        Ok(())
    }
}

fn handle_modal_key(app: &mut App, key: KeyCode, _modifiers: KeyModifiers) -> Result<()> {
    let state = &mut app.stack_builder_state;

    if state.editing_instance_name {
        // Handle instance name input
        match key {
            KeyCode::Esc => {
                state.cancel_modal();
            }
            KeyCode::Enter => {
                if let Err(err) = state.confirm_modal_selection() {
                    state.validation_error = Some(err);
                }
            }
            KeyCode::Backspace => {
                state.backspace();
            }
            KeyCode::Left => {
                state.move_cursor_left();
            }
            KeyCode::Right => {
                state.move_cursor_right();
            }
            KeyCode::Char(c) => {
                state.insert_char(c);
            }
            _ => {}
        }
    } else {
        // Handle module selection in modal
        match key {
            KeyCode::Esc => {
                state.cancel_modal();
            }
            KeyCode::Tab => {
                state.next_modal_module();
            }
            KeyCode::BackTab => {
                state.previous_modal_module();
            }
            KeyCode::Up => {
                state.previous_modal_module();
            }
            KeyCode::Down => {
                state.next_modal_module();
            }
            KeyCode::Enter => {
                state.select_modal_module();
            }
            _ => {}
        }
    }

    Ok(())
}

fn handle_module_list_key(app: &mut App, key: KeyCode) -> Result<()> {
    let state = &mut app.stack_builder_state;

    match key {
        KeyCode::Esc => {
            state.close();
        }
        KeyCode::Tab => {
            // Toggle between stack name and module instances
            if state.editing_stack_name {
                if !state.module_instances.is_empty() {
                    state.editing_stack_name = false;
                    state.selected_instance_index = 0;
                }
            } else {
                state.editing_stack_name = true;
            }
        }
        KeyCode::Enter => {
            // Start editing stack name if not already editing
            if !state.editing_stack_name {
                state.editing_stack_name = true;
            }
        }
        KeyCode::Up => {
            if state.editing_stack_name {
                // Can't go up from stack name, do nothing
            } else if state.selected_instance_index == 0 {
                // At first instance, go back to stack name
                state.editing_stack_name = true;
            } else {
                state.previous_selected_instance();
            }
        }
        KeyCode::Down => {
            if state.editing_stack_name {
                // Move from stack name to first module instance
                if !state.module_instances.is_empty() {
                    state.editing_stack_name = false;
                    state.selected_instance_index = 0;
                }
            } else {
                // Navigate through module instances
                state.next_selected_instance();
            }
        }
        KeyCode::Backspace => {
            if state.editing_stack_name {
                state.backspace();
            }
        }
        KeyCode::Left => {
            if state.editing_stack_name {
                state.move_cursor_left();
            }
        }
        KeyCode::Right => {
            if state.editing_stack_name {
                state.move_cursor_right();
            }
        }
        KeyCode::Char(c) => {
            if state.editing_stack_name {
                state.insert_char(c);
            }
        }
        _ => {}
    }

    Ok(())
}

fn handle_variable_configuration_key(app: &mut App, key: KeyCode) -> Result<()> {
    let state = &mut app.stack_builder_state;

    match key {
        KeyCode::Esc => {
            state.close();
        }
        KeyCode::Up => {
            state.previous_variable();
        }
        KeyCode::Down => {
            state.next_variable();
        }
        KeyCode::Left => {
            state.previous_instance();
        }
        KeyCode::Right => {
            state.next_instance();
        }
        KeyCode::Backspace => {
            state.backspace();
        }
        KeyCode::Char(c) => {
            state.insert_char(c);
        }
        _ => {}
    }

    Ok(())
}

fn handle_preview_key(app: &mut App, key: KeyCode) -> Result<()> {
    let state = &mut app.stack_builder_state;

    match key {
        KeyCode::Esc => {
            state.close();
        }
        KeyCode::Up => {
            state.scroll_preview_up();
        }
        KeyCode::Down => {
            state.scroll_preview_down();
        }
        KeyCode::PageUp => {
            for _ in 0..10 {
                state.scroll_preview_up();
            }
        }
        KeyCode::PageDown => {
            for _ in 0..10 {
                state.scroll_preview_down();
            }
        }
        _ => {}
    }

    Ok(())
}

fn handle_reference_picker_key(
    app: &mut App,
    key: KeyCode,
    _modifiers: KeyModifiers,
) -> Result<()> {
    let state = &mut app.stack_builder_state;

    use crate::tui::state::stack_builder_state::ReferencePickerStep;

    match state.reference_picker_step {
        ReferencePickerStep::SelectInstance => match key {
            KeyCode::Esc => {
                state.close_reference_picker();
            }
            KeyCode::Tab => {
                state.next_reference_instance();
            }
            KeyCode::BackTab => {
                state.previous_reference_instance();
            }
            KeyCode::Up => {
                state.previous_reference_instance();
            }
            KeyCode::Down => {
                state.next_reference_instance();
            }
            KeyCode::Enter => {
                state.select_reference_instance();
            }
            _ => {}
        },
        ReferencePickerStep::SelectOutput => match key {
            KeyCode::Esc => {
                state.back_to_instance_selection();
            }
            KeyCode::Tab => {
                state.next_reference_output();
            }
            KeyCode::BackTab => {
                state.previous_reference_output();
            }
            KeyCode::Up => {
                state.previous_reference_output();
            }
            KeyCode::Down => {
                state.next_reference_output();
            }
            KeyCode::Enter => {
                state.confirm_reference_selection();
            }
            KeyCode::Backspace => {
                state.back_to_instance_selection();
            }
            _ => {}
        },
    }

    Ok(())
}
