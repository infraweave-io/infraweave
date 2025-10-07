use anyhow::Result;
use crossterm::event::KeyCode;

use crate::tui::app::App;

pub struct DetailHandler;

impl DetailHandler {
    pub fn handle_key(app: &mut App, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.close_detail();
            }
            KeyCode::Char('c') => {
                // Open claim builder
                if let Some(module) = app.detail_state.detail_module.clone() {
                    app.claim_builder_state.open_for_module(module);
                } else if let Some(stack) = app.detail_state.detail_stack.clone() {
                    app.claim_builder_state.open_for_stack(stack);
                }
            }
            KeyCode::Char('h') | KeyCode::Left => {
                app.detail_focus_left();
            }
            KeyCode::Char('l') | KeyCode::Right => {
                app.detail_focus_right();
            }
            KeyCode::Char('w') => {
                app.toggle_detail_wrap();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if app.detail_focus_right {
                    app.scroll_detail_up();
                } else {
                    app.detail_browser_up();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.detail_focus_right {
                    app.scroll_detail_down();
                } else {
                    app.detail_browser_down();
                }
            }
            KeyCode::PageUp => {
                if app.detail_focus_right {
                    app.scroll_detail_page_up();
                }
            }
            KeyCode::PageDown => {
                if app.detail_focus_right {
                    app.scroll_detail_page_down();
                }
            }
            _ => {}
        }
        Ok(())
    }
}
