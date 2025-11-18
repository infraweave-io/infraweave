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
                app.events_log_view = EventsLogView::Changelog;
                app.events_scroll = 0;

                // Load change record for selected job if not already loaded
                let grouped_events = app.get_grouped_events();
                if let Some((job_id, events)) = grouped_events.get(app.events_browser_index) {
                    // Check if we already have the change record
                    if !app.change_records.contains_key(job_id.as_str()) {
                        if let Some(first_event) = events.first() {
                            // Determine change type from event field
                            // Note: API expects uppercase (PLAN, APPLY, DESTROY)
                            // The event field contains values like "apply", "plan", "destroy"
                            let change_type = first_event.event.to_uppercase();

                            // Get environment and deployment_id from the event data
                            let environment = first_event.environment.clone();
                            let deployment_id = first_event.deployment_id.clone();

                            app.schedule_action(PendingAction::LoadChangeRecord(
                                job_id.clone(),
                                environment,
                                deployment_id,
                                change_type,
                            ));
                        }
                    }
                }
            }
            KeyCode::Char('2') => {
                app.events_log_view = EventsLogView::Events;
                app.events_scroll = 0;
            }
            KeyCode::Char('3') => {
                app.events_log_view = EventsLogView::Logs;
                app.events_scroll = 0;

                let grouped_events = app.get_grouped_events();
                if let Some((job_id, _)) = grouped_events.get(app.events_browser_index) {
                    app.schedule_action(PendingAction::LoadJobLogs(job_id.clone()));
                }
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
