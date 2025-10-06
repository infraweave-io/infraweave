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
}
