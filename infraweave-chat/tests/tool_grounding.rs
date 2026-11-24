mod support;

use infraweave_chat::llm::{LlmContent, LlmResponse, LlmUsage, StopReason};
use serde_json::json;
use support::fixtures::{one_project, PROJECTS, PROJECT_ID};
use support::scenario::Scenario;

fn text_response(text: &str) -> LlmResponse {
    LlmResponse {
        content: vec![LlmContent::Text {
            text: text.to_string(),
        }],
        stop_reason: StopReason::EndTurn,
        usage: LlmUsage::default(),
    }
}

fn tool_call(id: &str, name: &str, input: serde_json::Value) -> LlmResponse {
    LlmResponse {
        content: vec![LlmContent::ToolUse {
            id: id.to_string(),
            name: name.to_string(),
            input,
        }],
        stop_reason: StopReason::ToolUse,
        usage: LlmUsage::default(),
    }
}

fn two_region_project() -> serde_json::Value {
    one_project(&["us-west-2", "eu-central-1"])
}

#[tokio::test]
async fn a_hallucinated_project_id_is_rejected_with_actionable_guidance() {
    let result = Scenario::new("what is deployed in the developer account?")
        .with_api_route(PROJECTS, two_region_project())
        .with_script(vec![
            tool_call(
                "call-1",
                "list_deployments",
                json!({ "project_id": "developer account", "region": "us-west-2" }),
            ),
            text_response("Let me look up the project id first."),
        ])
        .run()
        .await;

    let blocks = result.tool_result_blocks();
    let LlmContent::ToolResult {
        content, is_error, ..
    } = &blocks[0]
    else {
        panic!("expected a tool result block");
    };
    assert!(is_error);
    // The error must tell the model how to recover, not merely that it failed.
    assert!(content.contains("unknown project_id"));
    assert!(content.contains("list_projects"));
}

#[tokio::test]
async fn a_broad_region_alias_is_rejected_and_the_valid_regions_are_named() {
    let result = Scenario::new("what is deployed in europe?")
        .with_api_route(PROJECTS, two_region_project())
        .with_script(vec![
            tool_call(
                "call-1",
                "list_deployments",
                json!({ "project_id": PROJECT_ID, "region": "eu" }),
            ),
            text_response("Which region did you mean?"),
        ])
        .run()
        .await;

    let blocks = result.tool_result_blocks();
    let LlmContent::ToolResult {
        content, is_error, ..
    } = &blocks[0]
    else {
        panic!("expected a tool result block");
    };
    assert!(is_error);
    assert!(content.contains("invalid region"));
    assert!(
        content.contains("eu-central-1"),
        "the error should list the configured regions so the model can retry"
    );
}

#[tokio::test]
async fn session_defaults_fill_in_omitted_arguments() {
    let result = Scenario::new("what is deployed here?")
        .with_session_defaults(PROJECT_ID, "us-west-2", "prod", "stable")
        .with_api_route(PROJECTS, two_region_project())
        .with_api_route("/api/v1/deployments/123456789012/us-west-2", json!([]))
        .with_script(vec![
            // The model omits project and region entirely.
            tool_call("call-1", "list_deployments", json!({})),
            text_response("Nothing is deployed there."),
        ])
        .run()
        .await;

    // The context defaults must be applied, not left for the model to guess.
    assert!(result
        .requested_paths
        .iter()
        .any(|p| p == "/api/v1/deployments/123456789012/us-west-2"));
    assert!(result.tool_errors().is_empty());
}

#[tokio::test]
async fn a_missing_required_argument_with_no_default_asks_rather_than_guesses() {
    let result = Scenario::new("why did it fail?")
        .with_api_route(PROJECTS, two_region_project())
        .with_script(vec![
            // No deployment_id, no session defaults to fall back on.
            tool_call("call-1", "debug_deployment", json!({})),
            text_response("Which deployment did you mean?"),
        ])
        .run()
        .await;

    let blocks = result.tool_result_blocks();
    let LlmContent::ToolResult {
        content, is_error, ..
    } = &blocks[0]
    else {
        panic!("expected a tool result block");
    };
    assert!(is_error);
    assert!(content.contains("deployment_id"));
}
