mod support;

use infraweave_chat::llm::{LlmContent, LlmResponse, LlmUsage, StopReason};
use serde_json::json;
use support::fixtures::{one_project, PROJECTS};
use support::scenario::Scenario;

#[tokio::test]
async fn runs_a_single_tool_turn_end_to_end() {
    let result = Scenario::new("list the projects")
        .with_api_route(PROJECTS, one_project(&["us-west-2"]))
        .with_script(vec![
            LlmResponse {
                content: vec![LlmContent::ToolUse {
                    id: "call-1".to_string(),
                    name: "list_projects".to_string(),
                    input: json!({}),
                }],
                stop_reason: StopReason::ToolUse,
                usage: LlmUsage::default(),
            },
            LlmResponse {
                content: vec![LlmContent::Text {
                    text: "There is one project.".to_string(),
                }],
                stop_reason: StopReason::EndTurn,
                usage: LlmUsage::default(),
            },
        ])
        .run()
        .await;

    assert_eq!(result.tool_calls_made(), vec!["list_projects".to_string()]);
    assert!(
        result.tool_errors().is_empty(),
        "the fixture must deserialize"
    );
    assert!(result.requested_paths.contains(&PROJECTS.to_string()));
    assert!(result.assistant_text().contains("one project"));
}
