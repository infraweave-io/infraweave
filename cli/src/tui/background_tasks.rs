use anyhow::Result;
use std::time::Duration;

use super::app::{App, EventsLogView};
use super::background::BackgroundMessage;

/// Check and trigger auto-refresh of logs if needed
/// Only auto-refreshes for deployments with status "requested" or "initiated"
pub async fn check_auto_refresh_logs(app: &mut App) -> Result<()> {
    if !app.auto_refresh_logs {
        return Ok(());
    }

    if !app.events_state.showing_events {
        return Ok(());
    }

    if app.events_log_view != EventsLogView::Logs {
        return Ok(());
    }

    // Check if the current deployment has a status that requires auto-refresh
    let should_auto_refresh = should_deployment_auto_refresh(app);
    if !should_auto_refresh {
        return Ok(());
    }

    if let Some(last_refresh) = app.last_log_refresh {
        if last_refresh.elapsed() >= Duration::from_secs(5) {
            let job_id = app.events_current_job_id.clone();
            if !job_id.is_empty() {
                app.load_logs_for_job(&job_id).await?;
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

/// Process all pending background messages from the channel
pub fn process_background_messages(
    app: &mut App,
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<BackgroundMessage>,
) {
    while let Ok(message) = receiver.try_recv() {
        app.process_background_message(message);
    }
}
