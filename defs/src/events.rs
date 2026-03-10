use serde_json::{json, Value};

/// Event payload builders for cloud function invocations
/// These define the standard event format used across AWS and Azure

// File operations

pub fn upload_file_base64_event(key: &str, bucket: &str, base64_content: &str) -> Value {
    json!({
        "event": "upload_file_base64",
        "data": {
            "key": key,
            "bucket_name": bucket,
            "base64_content": base64_content
        }
    })
}

pub fn upload_file_url_event(key: &str, bucket: &str, url: &str) -> Value {
    json!({
        "event": "upload_file_url",
        "data": {
            "key": key,
            "bucket_name": bucket,
            "url": url
        }
    })
}

pub fn generate_presigned_url_event(key: &str, bucket: &str) -> Value {
    json!({
        "event": "generate_presigned_url",
        "data": {
            "key": key,
            "bucket_name": bucket,
            "expires_in": 60,
        }
    })
}

// Database operations

pub fn transact_write_event(items: &Value) -> Value {
    json!({
        "event": "transact_write",
        "items": items,
    })
}

pub fn read_db_event(table: &str, query: &Value) -> Value {
    json!({
        "event": "read_db",
        "table": table,
        "data": {
            "query": query
        }
    })
}

// Job operations

pub fn get_job_status_event(job_id: &str) -> Value {
    json!({
        "event": "get_job_status",
        "data": {
            "job_id": job_id
        }
    })
}

pub fn read_logs_event(job_id: &str, next_token: Option<&str>, limit: Option<i32>) -> Value {
    let mut data = json!({
        "job_id": job_id
    });

    if let Some(token) = next_token {
        data["next_token"] = json!(token);
    }

    if let Some(l) = limit {
        data["limit"] = json!(l);
    }

    json!({
        "event": "read_logs",
        "data": data
    })
}

// Environment operations

pub fn get_environment_variables_event() -> Value {
    json!({
        "event": "get_environment_variables"
    })
}

// Database insert operations

pub fn insert_db_event(table: &str, data: &Value) -> Value {
    json!({
        "event": "insert_db",
        "table": table,
        "data": data
    })
}

// Infrastructure operations

pub fn start_runner_event(data: &Value) -> Value {
    json!({
        "event": "start_runner",
        "data": data
    })
}

// Notification operations

pub fn publish_notification_event(data: &Value) -> Value {
    json!({
        "event": "publish_notification",
        "data": data
    })
}
