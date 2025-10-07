use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};

use crate::tui::app::App;

pub struct ClaimBuilderHandler;

impl ClaimBuilderHandler {
    pub fn handle_key(app: &mut App, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        let state = &mut app.claim_builder_state;

        // Handle Ctrl key combinations first
        if modifiers.contains(KeyModifiers::CONTROL) {
            match key {
                KeyCode::Char('p') => {
                    state.toggle_preview();
                    return Ok(());
                }
                KeyCode::Char('s') => {
                    // Save to file
                    if state.show_preview {
                        app.schedule_action(crate::tui::app::PendingAction::SaveClaimToFile);
                    } else {
                        state.generate_yaml();
                        app.schedule_action(crate::tui::app::PendingAction::SaveClaimToFile);
                    }
                    return Ok(());
                }
                KeyCode::Char('r') => {
                    // Run claim
                    if state.show_preview {
                        app.schedule_action(crate::tui::app::PendingAction::RunClaimFromBuilder);
                    } else {
                        state.generate_yaml();
                        app.schedule_action(crate::tui::app::PendingAction::RunClaimFromBuilder);
                    }
                    return Ok(());
                }
                KeyCode::Char('t') => {
                    // Insert template for current field type
                    if !state.show_preview {
                        state.insert_template();
                    }
                    return Ok(());
                }
                _ => {}
            }
        }

        if state.show_preview {
            // Preview mode navigation
            match key {
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
                KeyCode::Esc => {
                    // Go back to form editing instead of closing
                    state.toggle_preview();
                }
                _ => {}
            }
        } else {
            // Form editing mode
            match key {
                KeyCode::Tab => {
                    state.next_field();
                }
                KeyCode::BackTab => {
                    state.previous_field();
                }
                KeyCode::Up => {
                    state.previous_field();
                }
                KeyCode::Down => {
                    state.next_field();
                }
                KeyCode::Left => {
                    state.move_cursor_left();
                }
                KeyCode::Right => {
                    state.move_cursor_right();
                }
                KeyCode::Home => match state.selected_field_index {
                    0 => state.deployment_name_cursor = 0,
                    i if i >= 1 => {
                        let var_index = i - 1;
                        if let Some(input) = state.variable_inputs.get_mut(var_index) {
                            input.move_cursor_home();
                        }
                    }
                    _ => {}
                },
                KeyCode::End => match state.selected_field_index {
                    0 => state.deployment_name_cursor = state.deployment_name.len(),
                    i if i >= 1 => {
                        let var_index = i - 1;
                        if let Some(input) = state.variable_inputs.get_mut(var_index) {
                            input.move_cursor_end();
                        }
                    }
                    _ => {}
                },
                KeyCode::Backspace => {
                    state.backspace();
                }
                KeyCode::Enter => {
                    // Toggle YAML preview
                    state.toggle_preview();
                }
                KeyCode::Char(c) => {
                    state.insert_char(c);
                }
                KeyCode::Esc => {
                    state.close();
                }
                _ => {}
            }
        }

        Ok(())
    }
}
