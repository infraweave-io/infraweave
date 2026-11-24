//! Minimal internal-api payloads that deserialize into the real `env_defs` types.
//!
//! A fixture that fails to deserialize turns a success-path scenario into a
//! silent tool error, so keep these in one place and match the real shapes.
//! `tests/fixtures_deserialize.rs` checks each one against its `env_defs` type.

use serde_json::{json, Value};

pub const PROJECTS: &str = "/api/v1/projects";
pub const PROJECT_ID: &str = "123456789012";
pub const REGION: &str = "us-west-2";
pub const ENVIRONMENT: &str = "prod";
pub const DEPLOYMENT_ID: &str = "payments-api";

/// `GET /api/v1/projects` with one project configured in `regions`.
pub fn one_project(regions: &[&str]) -> Value {
    json!([{
        "project_id": PROJECT_ID,
        "name": "platform",
        "description": "",
        "regions": regions,
        "repositories": [],
    }])
}

/// One `env_defs::DeploymentResp`, as `GET /api/v1/deployment/...` returns it.
pub fn deployment(job_id: &str, module_version: &str, status: &str) -> Value {
    json!({
        "epoch": 1_757_000_000_000u64,
        "deployment_id": DEPLOYMENT_ID,
        "status": status,
        "job_id": job_id,
        "environment": ENVIRONMENT,
        "project_id": PROJECT_ID,
        "region": REGION,
        "module": "payments",
        "module_version": module_version,
        "module_type": "module",
        "module_track": "stable",
        "drift_detection": {},
        "next_drift_check_epoch": 0,
        "has_drifted": false,
        "variables": {},
        "output": {},
        "policy_results": [],
        "error_text": "",
        "deleted": false,
        "dependencies": [],
        "initiated_by": "ci",
        "cpu": "1024",
        "memory": "2048",
        "reference": "",
        "tf_resources": null,
    })
}

/// One `env_defs::EventData`, as `GET /api/v1/events/...` returns it in a list.
pub fn event(job_id: &str, status: &str, epoch: u64) -> Value {
    json!({
        "deployment_id": DEPLOYMENT_ID,
        "project_id": PROJECT_ID,
        "region": REGION,
        "environment": ENVIRONMENT,
        "event": "apply",
        "epoch": epoch,
        "error_text": "",
        "id": format!("event-{epoch}"),
        "job_id": job_id,
        "metadata": {},
        "drift_detection": {},
        "next_drift_check_epoch": 0,
        "has_drifted": false,
        "module": "payments",
        "module_version": "2.3.1",
        "name": DEPLOYMENT_ID,
        "status": status,
        "timestamp": "2026-09-04T10:15:00Z",
        "output": {},
        "policy_results": [],
        "initiated_by": "ci",
        "event_duration": 0,
    })
}

/// One page of `GET /api/v1/logs/...`: CloudWatch events joined into `logs`,
/// read forward from the head of the stream.
pub fn log_page(lines: &[&str], next_forward_token: Option<&str>) -> Value {
    let mut logs = String::new();
    for line in lines {
        logs.push_str(line);
        logs.push('\n');
    }
    let mut page = json!({ "logs": logs });
    if let Some(token) = next_forward_token {
        page["nextForwardToken"] = json!(token);
    }
    page
}

/// `GET /api/v1/job_status/...` on AWS: the ECS task's state.
pub fn job_status(status: &str, exit_code: i64, stopped_reason: &str) -> Value {
    json!({
        "status": status,
        "stopped_reason": stopped_reason,
        "exit_code": exit_code,
    })
}
