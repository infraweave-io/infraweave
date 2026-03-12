use env_defs::{CloudProvider, LogData};

use crate::interface::GenericCloudHandler;

pub async fn read_logs(
    handler: &GenericCloudHandler,
    _project_id: &str,
    job_id: &str,
) -> Result<Vec<LogData>, anyhow::Error> {
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
