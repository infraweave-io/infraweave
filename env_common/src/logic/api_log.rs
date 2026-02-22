use env_defs::{CloudProvider, LogData};

use crate::interface::GenericCloudHandler;

pub async fn read_logs(
    handler: &GenericCloudHandler,
    _project_id: &str,
    job_id: &str,
) -> Result<Vec<LogData>, anyhow::Error> {
    // Check if HTTP mode is enabled
    if env_aws::is_http_mode_enabled() {
        let logs_str =
            env_aws::http_get_logs(handler.get_project_id(), handler.get_region(), job_id).await?;

        // Parse the logs string into LogData entries
        // The HTTP API returns logs as a concatenated string with newlines
        let log_entries: Vec<LogData> = logs_str
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| LogData {
                message: line.to_string(),
            })
            .collect();

        return Ok(log_entries);
    }

    let payload = env_defs::read_logs_event(job_id, None, None);
    let response = match handler.run_function(&payload).await {
        Ok(response) => response.payload,
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to read logs: {}", e));
        }
    };

    let log_events = response.get("events").expect("Log events not found");

    if let Some(log_entries) = log_events.as_array() {
        let mut log_entry_vec: Vec<LogData> = vec![];
        for log in log_entries {
            // warn!("Event: {:?}", event);
            let message: LogData =
                serde_json::from_value(log.clone()).expect("Failed to parse log entry");
            log_entry_vec.push(message);
        }

        // let logs = log_entry_vec.join("\n");
        // println!("Logs: {}", logs);
        Ok(log_entry_vec)
    } else {
        panic!("Expected an array of log_entry");
    }
}
