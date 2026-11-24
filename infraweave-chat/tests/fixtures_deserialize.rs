//! Every fixture must deserialize into the `env_defs` type the tool binds it to.
//! A fixture that drifts from the real shape fails here, by name, instead of
//! turning a success-path scenario into a quiet tool error.

mod support;

use env_defs::{DeploymentResp, EventData, ProjectData};
use serde::de::DeserializeOwned;
use serde_json::Value;
use support::fixtures;

fn parses<T: DeserializeOwned>(name: &str, value: Value) {
    if let Err(e) = serde_json::from_value::<T>(value) {
        panic!("fixture `{name}` does not deserialize: {e}");
    }
}

#[test]
fn project_fixture_matches_project_data() {
    parses::<Vec<ProjectData>>("one_project", fixtures::one_project(&["us-west-2"]));
}

#[test]
fn deployment_fixture_matches_deployment_resp() {
    parses::<DeploymentResp>(
        "deployment",
        fixtures::deployment("job-77", "2.3.1", "failed"),
    );
}

#[test]
fn event_fixture_matches_event_data() {
    parses::<Vec<EventData>>(
        "event",
        Value::Array(vec![fixtures::event("job-77", "failed", 1)]),
    );
}
