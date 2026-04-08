use anyhow::Result;
use crossterm::event::KeyCode;

use crate::tui::app::{App, PendingAction, View};

pub struct ModalHandler;

impl ModalHandler {
    pub fn handle_confirmation_key(app: &mut App, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.confirm_action();
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.close_confirmation();
            }
            _ => {}
        }
        Ok(())
    }

    pub fn handle_versions_key(app: &mut App, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.close_modal();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.modal_move_up();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.modal_move_down();
            }
            KeyCode::Left => {
                app.modal_previous_track();
            }
            KeyCode::Right => {
                app.modal_next_track();
            }
            KeyCode::Char('r') => {
                app.modal_reload_versions();
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if app.current_view == View::Stacks {
                    app.schedule_action(PendingAction::ShowStackDetail(app.modal_selected_index));
                } else {
                    app.schedule_action(PendingAction::ShowModuleDetail(app.modal_selected_index));
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn handle_filter_key(app: &mut App, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.modal_state.close_filter_modal();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if app.modal_state.filter_selected_index > 0 {
                    app.modal_state.filter_selected_index -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                // Limit to length (which is the "All" option at the end)
                if app.modal_state.filter_selected_index < app.modal_state.filter_options.len() {
                    app.modal_state.filter_selected_index += 1;
                }
            }
            KeyCode::Enter => {
                let selected = if app.modal_state.filter_selected_index
                    >= app.modal_state.filter_options.len()
                {
                    None
                } else {
                    Some(
                        app.modal_state.filter_options[app.modal_state.filter_selected_index]
                            .clone(),
                    )
                };

                match app.modal_state.filter_type {
                    crate::tui::state::modal_state::FilterType::Project => {
                        app.selected_project_filter = selected;
                        app.selected_index = 0; // Reset selection to avoid out of bounds
                        app.project_selection_made = true;
                        app.schedule_action(PendingAction::LoadDeployments);
                    }
                    crate::tui::state::modal_state::FilterType::Region => {
                        app.selected_region_filter = selected;
                        app.selected_index = 0; // Reset selection to avoid out of bounds
                        app.project_selection_made = true;
                        app.schedule_action(PendingAction::LoadDeployments);
                    }
                    _ => {}
                }
                app.modal_state.close_filter_modal();
            }
            _ => {}
        }
        Ok(())
    }
}
