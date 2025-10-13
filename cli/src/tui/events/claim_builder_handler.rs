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
                KeyCode::Char('y') => {
                    // Copy to clipboard (Ctrl+Y for "yank")
                    if !state.show_preview {
                        state.generate_yaml();
                    }

                    // Validate first
                    if let Err(err) = state.validate() {
                        app.detail_state
                            .show_error(&format!("Cannot copy claim: {}", err));
                        return Ok(());
                    }

                    // Copy to clipboard
                    match arboard::Clipboard::new() {
                        Ok(mut clipboard) => match clipboard.set_text(&state.generated_yaml) {
                            Ok(_) => {
                                app.detail_state
                                    .show_message("✅ Claim YAML copied to clipboard!".to_string());
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
                    // Run claim with confirmation
                    if !state.show_preview {
                        state.generate_yaml();
                    }

                    // Validate first
                    if let Err(err) = state.validate() {
                        app.detail_state
                            .show_error(&format!("Cannot run claim: {}", err));
                        return Ok(());
                    }

                    // Show confirmation dialog
                    let deployment_name = &state.deployment_name;
                    let region = &state.region;
                    let message = format!(
                        "Are you sure you want to run this deployment claim?\n\n\
                        Deployment: {}\n\
                        Region: {}\n\n\
                        This will create or update the deployment.\n\n\
                        Press 'y' to confirm or 'n' to cancel.",
                        deployment_name, region
                    );

                    app.modal_state.showing_confirmation = true;
                    app.modal_state.confirmation_message = message.clone();
                    app.modal_state.confirmation_action =
                        crate::tui::app::PendingAction::RunClaimFromBuilder;

                    app.showing_confirmation = true;
                    app.confirmation_message = message;
                    app.confirmation_action = crate::tui::app::PendingAction::RunClaimFromBuilder;

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
