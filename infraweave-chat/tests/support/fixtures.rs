//! Minimal internal-api payloads that deserialize into the real `env_defs` types.
//!
//! A fixture that fails to deserialize turns a success-path scenario into a
//! silent tool error, so keep these in one place and match the real shapes.

use serde_json::{json, Value};

pub const PROJECTS: &str = "/api/v1/projects";
pub const PROJECT_ID: &str = "123456789012";

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
