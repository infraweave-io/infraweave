mod support;

use serde_json::json;
use support::fixtures::{
    deployment, event, job_status, log_page, one_project, PROJECTS, PROJECT_ID,
};
use support::scenario::Scenario;
use support::script::{result_sent_on_call, text_response, tool_call};

const LOGS: &str = "/api/v1/logs/123456789012/us-west-2/abc-123";

#[tokio::test]
async fn get_deployment_logs_returns_the_runner_output() {
    let result = Scenario::new("show me the logs for job abc-123")
        .with_session_defaults(PROJECT_ID, "us-west-2", "prod", "stable")
        .with_api_route(PROJECTS, one_project(&["us-west-2"]))
        .with_api_route(
            LOGS,
            log_page(
                &[
                    "Initializing Terraform...",
                    "Error: creating S3 Bucket: BucketAlreadyOwnedByYou",
                ],
                None,
            ),
        )
        .with_script(vec![
            tool_call(
                "call-1",
                "get_deployment_logs",
                json!({ "job_id": "abc-123" }),
            ),
            text_response("The bucket already exists."),
        ])
        .run()
        .await;

    assert!(
        result.tool_errors().is_empty(),
        "{:?}",
        result.tool_result_blocks()
    );
    let (content, is_error) = result_sent_on_call(&result.calls, 1, "call-1");
    assert!(!is_error);
    assert!(content.contains("BucketAlreadyOwnedByYou"));
    assert!(content.contains("abc-123"));
}

#[tokio::test]
async fn get_deployment_logs_follows_forward_tokens_to_the_end_of_the_stream() {
    // CloudWatch pages forward from the head, and the failure is at the tail,
    // so stopping after the first page would hide the error.
    let result = Scenario::new("show me the logs for job abc-123")
        .with_session_defaults(PROJECT_ID, "us-west-2", "prod", "stable")
        .with_api_route(PROJECTS, one_project(&["us-west-2"]))
        .with_api_route(LOGS, log_page(&["Initializing Terraform..."], Some("f/1")))
        .with_api_route(
            "/api/v1/logs/123456789012/us-west-2/abc-123?next_token=f%2F1",
            log_page(&["Error: BucketAlreadyOwnedByYou"], Some("f/2")),
        )
        .with_api_route(
            "/api/v1/logs/123456789012/us-west-2/abc-123?next_token=f%2F2",
            log_page(&[], Some("f/2")),
        )
        .with_script(vec![
            tool_call(
                "call-1",
                "get_deployment_logs",
                json!({ "job_id": "abc-123" }),
            ),
            text_response("The bucket already exists."),
        ])
        .run()
        .await;

    assert!(
        result.tool_errors().is_empty(),
        "{:?}",
        result.tool_result_blocks()
    );
    let (content, _) = result_sent_on_call(&result.calls, 1, "call-1");
    assert!(content.contains("Initializing Terraform"));
    assert!(content.contains("BucketAlreadyOwnedByYou"));
    let log_requests = result
        .requested_paths
        .iter()
        .filter(|p| p.starts_with(LOGS))
        .count();
    assert_eq!(log_requests, 3, "{:?}", result.requested_paths);
}

#[tokio::test]
async fn get_deployment_logs_keeps_the_tail_of_a_long_log() {
    let lines: Vec<String> = (1..=500).map(|i| format!("line-{i:04}")).collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let result = Scenario::new("show me the logs for job abc-123")
        .with_session_defaults(PROJECT_ID, "us-west-2", "prod", "stable")
        .with_api_route(PROJECTS, one_project(&["us-west-2"]))
        .with_api_route(LOGS, log_page(&refs, None))
        .with_script(vec![
            tool_call(
                "call-1",
                "get_deployment_logs",
                json!({ "job_id": "abc-123" }),
            ),
            text_response("Here is the end of the log."),
        ])
        .run()
        .await;

    assert!(
        result.tool_errors().is_empty(),
        "{:?}",
        result.tool_result_blocks()
    );
    let (content, _) = result_sent_on_call(&result.calls, 1, "call-1");
    assert!(content.contains("line-0500"), "the last line must survive");
    assert!(!content.contains("line-0001"), "the head should be dropped");
    assert!(
        content.contains("500"),
        "the total line count should be reported"
    );
}

#[tokio::test]
async fn get_deployment_logs_requires_a_job_id() {
    let result = Scenario::new("show me the logs")
        .with_session_defaults(PROJECT_ID, "us-west-2", "prod", "stable")
        .with_api_route(PROJECTS, one_project(&["us-west-2"]))
        .with_script(vec![
            tool_call("call-1", "get_deployment_logs", json!({})),
            text_response("Which job?"),
        ])
        .run()
        .await;

    let (content, is_error) = result_sent_on_call(&result.calls, 1, "call-1");
    assert!(is_error);
    assert!(content.contains("job_id"));
    assert!(
        !result
            .requested_paths
            .iter()
            .any(|p| p.starts_with("/api/v1/logs")),
        "no logs request without a job id"
    );
}

#[tokio::test]
async fn get_deployment_logs_validates_the_region_before_reading_logs() {
    let result = Scenario::new("show me the logs for job abc-123 in europe")
        .with_session_defaults(PROJECT_ID, "us-west-2", "prod", "stable")
        .with_api_route(PROJECTS, one_project(&["us-west-2"]))
        .with_script(vec![
            tool_call(
                "call-1",
                "get_deployment_logs",
                json!({ "job_id": "abc-123", "region": "eu" }),
            ),
            text_response("Which region?"),
        ])
        .run()
        .await;

    let (content, is_error) = result_sent_on_call(&result.calls, 1, "call-1");
    assert!(is_error);
    assert!(content.contains("invalid region"));
    assert_eq!(result.requested_paths, vec![PROJECTS.to_string()]);
}

#[tokio::test]
async fn get_job_status_reports_progress() {
    let result = Scenario::new("is job abc-123 done?")
        .with_session_defaults(PROJECT_ID, "us-west-2", "prod", "stable")
        .with_api_route(PROJECTS, one_project(&["us-west-2"]))
        .with_api_route(
            "/api/v1/job_status/123456789012/us-west-2/abc-123",
            job_status("STOPPED", 1, "Essential container in task exited"),
        )
        .with_script(vec![
            tool_call("call-1", "get_job_status", json!({ "job_id": "abc-123" })),
            text_response("It stopped with exit code 1."),
        ])
        .run()
        .await;

    assert!(
        result.tool_errors().is_empty(),
        "{:?}",
        result.tool_result_blocks()
    );
    let (content, _) = result_sent_on_call(&result.calls, 1, "call-1");
    assert!(content.contains("STOPPED"));
    assert!(content.contains("Essential container in task exited"));
    assert!(content.contains("exit_code"));
}

#[tokio::test]
async fn debug_deployment_names_the_job_id_so_logs_can_be_requested() {
    // debug_deployment points the model at the failing job's logs, so the
    // job id has to be in its output for get_deployment_logs to be reachable.
    let result = Scenario::new("why did payments-api fail?")
        .with_session_defaults(PROJECT_ID, "us-west-2", "prod", "stable")
        .with_api_route(PROJECTS, one_project(&["us-west-2"]))
        .with_api_route(
            "/api/v1/deployment/123456789012/us-west-2/prod/payments-api",
            deployment("job-77", "2.3.1", "failed"),
        )
        .with_api_route(
            "/api/v1/events/123456789012/us-west-2/prod/payments-api",
            json!([event("job-77", "failed", 1_757_000_000_000)]),
        )
        .with_script(vec![
            tool_call(
                "call-1",
                "debug_deployment",
                json!({ "deployment_id": "payments-api", "environment_id": "prod" }),
            ),
            text_response("It failed."),
        ])
        .run()
        .await;

    assert!(
        result.tool_errors().is_empty(),
        "{:?}",
        result.tool_result_blocks()
    );
    let (content, _) = result_sent_on_call(&result.calls, 1, "call-1");
    assert!(content.contains("job-77"));
}
