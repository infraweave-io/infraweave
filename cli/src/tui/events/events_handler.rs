use anyhow::Result;
use crossterm::event::KeyCode;

use crate::tui::app::{App, EventsLogView, PendingAction};

pub struct EventsHandler;

impl EventsHandler {
    pub fn handle_key(app: &mut App, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.close_events();
            }
            KeyCode::Char('r') => {
                // Reload logs when viewing logs
                if app.events_log_view == EventsLogView::Logs
                    && !app.events_current_job_id.is_empty()
                {
                    let job_id = app.events_current_job_id.clone();
                    app.schedule_action(PendingAction::LoadJobLogs(job_id));
                }
            }
            KeyCode::Char('1') => {
                app.events_log_view = EventsLogView::Events;
                app.events_scroll = 0;
            }
            KeyCode::Char('2') => {
                app.events_log_view = EventsLogView::Logs;
                app.events_scroll = 0;

                let grouped_events = app.get_grouped_events();
                if let Some((job_id, _)) = grouped_events.get(app.events_browser_index) {
                    app.schedule_action(PendingAction::LoadJobLogs(job_id.clone()));
                }
            }
            KeyCode::Char('3') => {
                app.events_log_view = EventsLogView::Changelog;
                app.events_scroll = 0;
            }
            KeyCode::Tab => {
                app.events_log_view_next();
            }
            KeyCode::Char('h') | KeyCode::Left => {
                app.events_focus_left();
            }
            KeyCode::Char('l') | KeyCode::Right => {
                app.events_focus_right();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if app.events_focus_right {
                    app.scroll_events_up();
                } else {
                    app.events_browser_up();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.events_focus_right {
                    app.scroll_events_down();
                } else {
                    app.events_browser_down();
                }
            }
            KeyCode::PageUp => {
                if app.events_focus_right {
                    app.scroll_events_page_up();
                }
            }
            KeyCode::PageDown => {
                if app.events_focus_right {
                    app.scroll_events_page_down();
                }
            }
            _ => {}
        }
        Ok(())
    }
}
