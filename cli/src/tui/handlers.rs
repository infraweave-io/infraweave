use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use std::time::Duration;

use super::app::{App, PendingAction, View};

pub async fn handle_events(app: &mut App) -> Result<()> {
    if event::poll(Duration::from_millis(100))? {
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                // Handle Ctrl+C to quit
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    app.should_quit = true;
                    return Ok(());
                }
                handle_key_event(app, key.code, key.modifiers)?;
            }
        }
    }
    Ok(())
}

fn handle_key_event(app: &mut App, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
    // Don't handle other input while loading
    if app.is_loading {
        return Ok(());
    }

    // Confirmation modal has highest priority
    if app.showing_confirmation {
        match key {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.confirm_action();
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.close_confirmation();
            }
            _ => {}
        }
        return Ok(());
    }

    // Version modal has next highest priority
    if app.showing_versions_modal {
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
                // Reload versions for the selected track
                app.modal_reload_versions();
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                // Show details of the selected version
                // Use ShowStackDetail if we're in the Stacks view, otherwise ShowModuleDetail
                if app.current_view == View::Stacks {
                    app.schedule_action(PendingAction::ShowStackDetail(app.modal_selected_index));
                } else {
                    app.schedule_action(PendingAction::ShowModuleDetail(app.modal_selected_index));
                }
            }
            _ => {}
        }
        return Ok(());
    }

    // Events view has highest priority
    if app.showing_events {
        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.close_events();
            }
            KeyCode::Char('1') => {
                app.events_log_view = super::app::EventsLogView::Events;
                app.events_scroll = 0;
            }
            KeyCode::Char('2') => {
                app.events_log_view = super::app::EventsLogView::Logs;
                app.events_scroll = 0;

                // Load logs for the currently selected job
                let grouped_events = app.get_grouped_events();
                if let Some((job_id, _)) = grouped_events.get(app.events_browser_index) {
                    app.schedule_action(PendingAction::LoadJobLogs(job_id.clone()));
                }
            }
            KeyCode::Char('3') => {
                app.events_log_view = super::app::EventsLogView::Changelog;
                app.events_scroll = 0;
            }
            KeyCode::Tab => {
                app.events_log_view_next();

                // If we landed on the Logs view, load logs for the current job
                if matches!(app.events_log_view, super::app::EventsLogView::Logs) {
                    let grouped_events = app.get_grouped_events();
                    if let Some((job_id, _)) = grouped_events.get(app.events_browser_index) {
                        app.schedule_action(PendingAction::LoadJobLogs(job_id.clone()));
                    }
                }
            }
            KeyCode::BackTab => {
                app.events_log_view_previous();

                // If we landed on the Logs view, load logs for the current job
                if matches!(app.events_log_view, super::app::EventsLogView::Logs) {
                    let grouped_events = app.get_grouped_events();
                    if let Some((job_id, _)) = grouped_events.get(app.events_browser_index) {
                        app.schedule_action(PendingAction::LoadJobLogs(job_id.clone()));
                    }
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                app.events_focus_left();
            }
            KeyCode::Right | KeyCode::Char('l') => {
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
                app.scroll_events_page_up();
            }
            KeyCode::PageDown => {
                app.scroll_events_page_down();
            }
            _ => {}
        }
        return Ok(());
    }

    // Detail view has priority - handle it next regardless of search mode
    if app.showing_detail {
        // Detail view key handlers
        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.close_detail();
            }
            KeyCode::Left | KeyCode::Char('h') => {
                app.detail_focus_left();
            }
            KeyCode::Right | KeyCode::Char('l') => {
                app.detail_focus_right();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                // If viewing a module, stack, or deployment with structured data, navigate browser items (left pane)
                // or scroll detail content (right pane)
                if app.detail_module.is_some()
                    || app.detail_stack.is_some()
                    || app.detail_deployment.is_some()
                {
                    if app.detail_focus_right {
                        app.scroll_detail_up();
                    } else {
                        app.detail_browser_up();
                    }
                } else {
                    app.scroll_detail_up();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                // If viewing a module, stack, or deployment with structured data, navigate browser items (left pane)
                // or scroll detail content (right pane)
                if app.detail_module.is_some()
                    || app.detail_stack.is_some()
                    || app.detail_deployment.is_some()
                {
                    if app.detail_focus_right {
                        app.scroll_detail_down();
                    } else {
                        app.detail_browser_down();
                    }
                } else {
                    app.scroll_detail_down();
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
            KeyCode::Char('w') => {
                // Toggle line wrapping
                app.toggle_detail_wrap();
            }
            _ => {}
        }
        return Ok(());
    }

    // Search mode key handlers (only when not showing detail)
    if app.search_mode {
        match key {
            KeyCode::Esc => {
                app.exit_search_mode();
            }
            KeyCode::Enter => {
                // Show versions modal for modules/stacks, details for deployments
                match app.current_view {
                    View::Modules => {
                        app.schedule_action(PendingAction::ShowModuleVersions(app.selected_index));
                    }
                    View::Stacks => {
                        app.schedule_action(PendingAction::ShowStackVersions(app.selected_index));
                    }
                    View::Deployments => {
                        app.schedule_action(PendingAction::ShowDeploymentDetail(
                            app.selected_index,
                        ));
                    }
                    _ => {}
                }
            }
            KeyCode::Backspace => {
                app.search_backspace();
            }
            KeyCode::Up => {
                app.move_up();
            }
            KeyCode::Down => {
                app.move_down();
            }
            KeyCode::PageUp => {
                app.page_up();
            }
            KeyCode::PageDown => {
                app.page_down();
            }
            KeyCode::Char(c) => {
                // In search mode, 'q' exits search, all other chars are typed
                if c == 'q' {
                    app.exit_search_mode();
                } else {
                    app.search_input(c);
                }
            }
            _ => {}
        }
        return Ok(());
    }

    // Main view key handlers
    match key {
        KeyCode::Char('1') => {
            app.change_view(View::Modules);
            app.schedule_action(PendingAction::LoadModules);
        }
        KeyCode::Char('2') => {
            app.change_view(View::Stacks);
            app.schedule_action(PendingAction::LoadStacks);
        }
        KeyCode::Char('3') => {
            app.change_view(View::Policies);
        }
        KeyCode::Char('4') => {
            app.change_view(View::Deployments);
            app.schedule_action(PendingAction::LoadDeployments);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.move_up();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.move_down();
        }
        KeyCode::PageUp => {
            app.page_up();
        }
        KeyCode::PageDown => {
            app.page_down();
        }
        KeyCode::Left => {
            // Navigate to previous track in modules/stacks view
            if matches!(app.current_view, View::Modules | View::Stacks) {
                app.previous_track();
            }
        }
        KeyCode::Right => {
            // Navigate to next track in modules/stacks view
            if matches!(app.current_view, View::Modules | View::Stacks) {
                app.next_track();
            }
        }
        KeyCode::Enter | KeyCode::Char(' ') => match app.current_view {
            View::Modules => {
                app.schedule_action(PendingAction::ShowModuleVersions(app.selected_index));
            }
            View::Stacks => {
                app.schedule_action(PendingAction::ShowStackVersions(app.selected_index));
            }
            View::Deployments => {
                app.schedule_action(PendingAction::ShowDeploymentDetail(app.selected_index));
            }
            _ => {}
        },
        KeyCode::Char('r') => {
            // Check if Ctrl is held for reapply
            if modifiers.contains(KeyModifiers::CONTROL) {
                // Ctrl+R: Reapply deployment with confirmation
                if matches!(app.current_view, View::Deployments) {
                    let filtered_deployments = app.get_filtered_deployments();
                    if let Some(deployment) = filtered_deployments.get(app.selected_index) {
                        let message = format!(
                                "Reapply deployment?\n\nDeployment ID: {}\nModule: {}\nVersion: {}\nEnvironment: {}\n\nPress Y to confirm, N to cancel",
                                deployment.deployment_id,
                                deployment.module,
                                deployment.module_version,
                                deployment.environment
                            );
                        app.show_confirmation(
                            message,
                            app.selected_index,
                            PendingAction::ReapplyDeployment(app.selected_index),
                        );
                    }
                }
            } else {
                // Regular 'r': Reload current view
                match app.current_view {
                    View::Modules => {
                        app.schedule_action(PendingAction::LoadModules);
                    }
                    View::Stacks => {
                        app.schedule_action(PendingAction::LoadStacks);
                    }
                    View::Deployments => {
                        app.schedule_action(PendingAction::LoadDeployments);
                    }
                    _ => {}
                }
            }
        }
        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
            // Ctrl+D: Destroy deployment with confirmation
            if matches!(app.current_view, View::Deployments) {
                let filtered_deployments = app.get_filtered_deployments();
                if let Some(deployment) = filtered_deployments.get(app.selected_index) {
                    let message = format!(
                            "⚠️  DESTROY DEPLOYMENT?\n\nThis action will PERMANENTLY DELETE the deployment and all its resources!\n\nDeployment ID: {}\nModule: {}\nVersion: {}\nEnvironment: {}\n\nPress Y to confirm, N to cancel",
                            deployment.deployment_id,
                            deployment.module,
                            deployment.module_version,
                            deployment.environment
                        );
                    app.show_confirmation(
                        message,
                        app.selected_index,
                        PendingAction::DestroyDeployment(app.selected_index),
                    );
                }
            }
        }
        KeyCode::Char('/') => {
            // Enter search mode
            app.enter_search_mode();
        }
        KeyCode::Char('e') => {
            // Show events for selected deployment
            if matches!(app.current_view, View::Deployments) {
                app.schedule_action(PendingAction::ShowDeploymentEvents(app.selected_index));
            }
        }
        _ => {}
    }
    Ok(())
}
