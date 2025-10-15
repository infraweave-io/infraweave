use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};

use crate::tui::app::{App, PendingAction};

pub struct MainHandler;

impl MainHandler {
    pub fn handle_key(app: &mut App, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        // Handle Ctrl+key combinations first
        if modifiers.contains(KeyModifiers::CONTROL) {
            match key {
                KeyCode::Char('n') => {
                    if matches!(app.current_view, crate::tui::app::View::Stacks) {
                        // Open stack builder - load all modules
                        app.schedule_action(PendingAction::OpenStackBuilder);
                    }
                    return Ok(());
                }
                KeyCode::Char('r') => {
                    if matches!(app.current_view, crate::tui::app::View::Deployments) {
                        // Get the current deployment
                        let filtered_deployments = app.get_filtered_deployments();
                        if app.selected_index < filtered_deployments.len() {
                            let deployment = &filtered_deployments[app.selected_index];
                            let message = format!(
                                "Are you sure you want to reapply this deployment?\n\n\
                                Deployment ID: {}\n\
                                Module: {} ({})\n\
                                Environment: {}\n\
                                Status: {}\n\n\
                                Press 'y' to confirm or 'n' to cancel.",
                                deployment.deployment_id,
                                deployment.module,
                                deployment.module_version,
                                deployment.environment,
                                deployment.status
                            );

                            // Update modal_state directly
                            app.modal_state.show_confirmation(
                                message.clone(),
                                app.selected_index,
                                PendingAction::ReapplyDeployment(app.selected_index),
                            );

                            // Also update legacy fields for handler compatibility
                            app.showing_confirmation = true;
                            app.confirmation_message = message;
                            app.confirmation_deployment_index = Some(app.selected_index);
                            app.confirmation_action =
                                PendingAction::ReapplyDeployment(app.selected_index);
                        }
                    }
                    return Ok(());
                }
                KeyCode::Char('d') => {
                    if matches!(app.current_view, crate::tui::app::View::Deployments) {
                        // Get the current deployment
                        let filtered_deployments = app.get_filtered_deployments();
                        if app.selected_index < filtered_deployments.len() {
                            let deployment = &filtered_deployments[app.selected_index];

                            // Check if this deployment is managed by GitHub (quick check)
                            let is_github_managed = !deployment.reference.is_empty()
                                && deployment.reference.contains("github");

                            let message = if is_github_managed {
                                format!(
                                    "🚨 IMPORTANT WARNING: GITHUB-MANAGED DEPLOYMENT 🚨\n\n\
                                    ⚠️  This deployment's lifecycle is managed in Git!\n\
                                        Deleting it here may cause inconsistencies!\n\n\
                                    Deployment ID: {}\n\
                                    Module: {} ({})\n\
                                    Environment: {}\n\
                                    Status: {}\n\
                                    Managed in: {}\n\n\
                                    🔴 DISCOURAGED: The deployment will also the handled\n\
                                    by your GitOps workflow if the manifest still exists in Git.\n\n\
                                    Instead, remove the deployment from your Git repository\n\
                                    and let your GitOps process handle the deletion.\n\n\
                                    Are you ABSOLUTELY SURE you want to proceed?\n\
                                    Press 'y' to confirm or 'n' to cancel.",
                                    deployment.deployment_id,
                                    deployment.module,
                                    deployment.module_version,
                                    deployment.environment,
                                    deployment.status,
                                    deployment.reference
                                )
                            } else {
                                format!(
                                    "⚠️  WARNING: Are you sure you want to DESTROY this deployment?\n\n\
                                    Deployment ID: {}\n\
                                    Module: {} ({})\n\
                                    Environment: {}\n\
                                    Status: {}\n\n\
                                    This action cannot be undone!\n\
                                    Press 'y' to confirm or 'n' to cancel.",
                                    deployment.deployment_id,
                                    deployment.module,
                                    deployment.module_version,
                                    deployment.environment,
                                    deployment.status
                                )
                            };

                            // Update modal_state directly
                            app.modal_state.show_confirmation(
                                message.clone(),
                                app.selected_index,
                                PendingAction::DestroyDeployment(app.selected_index),
                            );

                            // Also update legacy fields for handler compatibility
                            app.showing_confirmation = true;
                            app.confirmation_message = message;
                            app.confirmation_deployment_index = Some(app.selected_index);
                            app.confirmation_action =
                                PendingAction::DestroyDeployment(app.selected_index);
                        }
                    }
                    return Ok(());
                }
                _ => {}
            }
        }

        // Handle regular key presses
        match key {
            KeyCode::Char('q') => {
                app.should_quit = true;
            }
            KeyCode::Char('1') => {
                app.change_view(crate::tui::app::View::Modules);
                app.schedule_action(PendingAction::LoadModules);
            }
            KeyCode::Char('2') => {
                app.change_view(crate::tui::app::View::Stacks);
                app.schedule_action(PendingAction::LoadStacks);
            }
            KeyCode::Char('3') => {
                app.change_view(crate::tui::app::View::Policies);
            }
            KeyCode::Char('4') => {
                app.change_view(crate::tui::app::View::Deployments);
                app.schedule_action(PendingAction::LoadDeployments);
            }
            KeyCode::Char('/') => {
                app.enter_search_mode();
            }
            KeyCode::Char('e') => {
                if matches!(app.current_view, crate::tui::app::View::Deployments) {
                    // Show events for the selected deployment
                    let filtered_deployments = app.get_filtered_deployments();
                    if app.selected_index < filtered_deployments.len() {
                        app.schedule_action(PendingAction::ShowDeploymentEvents(
                            app.selected_index,
                        ));
                    }
                }
            }
            KeyCode::Char('r') => match app.current_view {
                crate::tui::app::View::Modules => {
                    app.schedule_action(PendingAction::LoadModules);
                }
                crate::tui::app::View::Stacks => {
                    app.schedule_action(PendingAction::LoadStacks);
                }
                crate::tui::app::View::Deployments => {
                    app.schedule_action(PendingAction::LoadDeployments);
                }
                _ => {}
            },
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
                if matches!(
                    app.current_view,
                    crate::tui::app::View::Modules | crate::tui::app::View::Stacks
                ) {
                    app.previous_track();
                }
            }
            KeyCode::Right => {
                if matches!(
                    app.current_view,
                    crate::tui::app::View::Modules | crate::tui::app::View::Stacks
                ) {
                    app.next_track();
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => match app.current_view {
                crate::tui::app::View::Modules => {
                    app.schedule_action(PendingAction::ShowModuleVersions(app.selected_index));
                }
                crate::tui::app::View::Stacks => {
                    app.schedule_action(PendingAction::ShowStackVersions(app.selected_index));
                }
                crate::tui::app::View::Deployments => {
                    app.schedule_action(PendingAction::ShowDeploymentDetail(app.selected_index));
                }
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }

    pub fn handle_search_key(app: &mut App, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.exit_search_mode();
            }
            KeyCode::Enter => match app.current_view {
                crate::tui::app::View::Modules => {
                    app.schedule_action(PendingAction::ShowModuleDetail(app.selected_index));
                }
                crate::tui::app::View::Stacks => {
                    app.schedule_action(PendingAction::ShowStackDetail(app.selected_index));
                }
                crate::tui::app::View::Deployments => {
                    app.schedule_action(PendingAction::ShowDeploymentDetail(app.selected_index));
                }
                _ => {}
            },
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
            KeyCode::Backspace => {
                app.search_backspace();
            }
            KeyCode::Char(c) => {
                // Allow all characters to be typed in search, including 'j' and 'k'
                app.search_input(c);
            }
            _ => {}
        }
        Ok(())
    }
}
