use anyhow::Result;
use std::time::Duration;

use super::app::{App, EventsLogView};
use super::background::BackgroundMessage;

/// Check and trigger auto-refresh of logs if needed
/// Only auto-refreshes for deployments with status "requested" or "initiated"
/// Supports both events view and deployment detail view
pub async fn check_auto_refresh_logs(app: &mut App) -> Result<()> {
    if !app.auto_refresh_logs {
        return Ok(());
    }

    // Check if we're viewing logs in either events view or deployment detail view
    let viewing_logs_in_events =
        app.events_state.showing_events && app.events_log_view == EventsLogView::Logs;

    let viewing_logs_in_detail = app.showing_detail
        && app.detail_deployment.is_some()
        && app.detail_browser_index == app.calculate_logs_section_index();

    if !viewing_logs_in_events && !viewing_logs_in_detail {
        return Ok(());
    }

    // Check if the current deployment has a status that requires auto-refresh
    let should_auto_refresh = if viewing_logs_in_events {
        should_deployment_auto_refresh(app)
    } else if viewing_logs_in_detail {
        should_detail_deployment_auto_refresh(app)
    } else {
        false
    };

    if !should_auto_refresh {
        return Ok(());
    }

    if let Some(last_refresh) = app.last_log_refresh {
        if last_refresh.elapsed() >= Duration::from_secs(5) {
            let job_id = app.events_current_job_id.clone();
            if !job_id.is_empty() {
                // Show subtle refreshing indicator during auto-refresh
                app.set_refreshing(true);
                app.load_logs_for_job_with_options(&job_id, false).await?;
            }
        }
    }

    Ok(())
}

/// Check if the current deployment should auto-refresh based on its status
/// Returns true if status is "requested" or "initiated"
fn should_deployment_auto_refresh(app: &App) -> bool {
    // Find the deployment from the events data
    if let Some(deployment_id) = app.events_deployment_id.as_str().split("::").last() {
        for event in &app.events_data {
            if event.deployment_id == deployment_id {
                let status = event.status.to_lowercase();
                return status == "requested" || status == "initiated";
            }
        }
    }

    // If we can't determine the status, don't auto-refresh
    false
}

/// Check if the deployment in detail view should auto-refresh based on its status
/// Returns true if status is "requested" or "initiated"
fn should_detail_deployment_auto_refresh(app: &App) -> bool {
    // Check the status from the deployment detail
    if let Some(deployment) = &app.detail_deployment {
        let status = deployment.status.to_lowercase();
        return status == "requested" || status == "initiated";
    }

    // If we can't determine the status, don't auto-refresh
    false
}

/// Process all pending background messages from the channel
pub fn process_background_messages(
    app: &mut App,
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<BackgroundMessage>,
) {
    while let Ok(message) = receiver.try_recv() {
        app.process_background_message(message);
    }
}

/// Check and trigger auto-refresh of deployments list if needed
/// Refreshes every 10 seconds when viewing deployments
/// Shows "Refreshing..." indicator during refresh, then "Auto-refresh (10s)" when idle
pub async fn check_auto_refresh_deployments(app: &mut App) -> Result<()> {
    use crate::current_region_handler;

    // Only auto-refresh when viewing deployments
    if !matches!(app.current_view, super::app::View::Deployments) {
        return Ok(());
    }

    // Check if enough time has passed since last refresh
    if let Some(last_refresh) = app.last_deployments_refresh {
        if last_refresh.elapsed() >= Duration::from_secs(10) {
            // Show subtle refreshing indicator during auto-refresh
            app.set_refreshing(true);

            // Trigger completely non-blocking background refresh
            if let Some(sender) = &app.background_sender {
                let sender_clone = sender.clone();
                tokio::spawn(async move {
                    // Need to import trait for method resolution
                    use env_defs::*;

                    let result = current_region_handler().await.get_all_deployments("").await;

                    let deployments_result = result
                        .map(|deployments| {
                            deployments
                                .into_iter()
                                .map(|d| {
                                    let timestamp = if d.epoch > 0 {
                                        let secs = (d.epoch / 1000) as i64;
                                        chrono::DateTime::from_timestamp(secs, 0)
                                            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                                            .unwrap_or_else(|| "Unknown".to_string())
                                    } else {
                                        "Unknown".to_string()
                                    };

                                    super::app::Deployment {
                                        status: d.status,
                                        deployment_id: d.deployment_id,
                                        module: d.module,
                                        module_version: d.module_version,
                                        environment: d.environment,
                                        epoch: d.epoch,
                                        timestamp,
                                    }
                                })
                                .collect::<Vec<_>>()
                        })
                        .map_err(|e| e.to_string());

                    let message = BackgroundMessage::DeploymentsLoaded(deployments_result);
                    let _ = sender_clone.send(message);
                });
            }
            app.last_deployments_refresh = Some(std::time::Instant::now());
        }
    } else {
        // Initialize the timer on first view
        app.last_deployments_refresh = Some(std::time::Instant::now());
    }

    Ok(())
}
