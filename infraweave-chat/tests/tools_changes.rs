mod support;

use serde_json::{json, Value};
use support::fixtures::{change_record, one_project, resource_change, PROJECTS, PROJECT_ID};
use support::scenario::Scenario;
use support::script::{result_sent_on_call, text_response, tool_call};

const BASE: &str = "/api/v1/change_records/123456789012/us-west-2/prod/payments-api";
const MUTATES: &str =
    "/api/v1/change_records/123456789012/us-west-2/prod/payments-api?change_type=mutate&limit=10";
const PLANS: &str =
    "/api/v1/change_records/123456789012/us-west-2/prod/payments-api?change_type=plan&limit=10";
const ONE_MUTATE: &str =
    "/api/v1/change_record/123456789012/us-west-2/prod/payments-api/job-77/mutate";
const ONE_PLAN: &str = "/api/v1/change_record/123456789012/us-west-2/prod/payments-api/job-77/plan";

fn scenario(message: &str) -> Scenario {
    Scenario::new(message)
        .with_session_defaults(PROJECT_ID, "us-west-2", "prod", "stable")
        .with_api_route(PROJECTS, one_project(&["us-west-2"]))
}

fn many(prefix: &str, change_type: &str, count: u64, first_epoch: u64) -> Value {
    Value::Array(
        (0..count)
            .map(|i| {
                change_record(
                    &format!("{prefix}-{i}"),
                    change_type,
                    "2.3.0",
                    first_epoch + i,
                    "1 to add",
                )
            })
            .collect(),
    )
}

#[tokio::test]
async fn list_change_records_merges_plans_and_applies_newest_first() {
    let result = scenario("what changed on payments-api recently?")
        .with_api_route(
            MUTATES,
            json!([change_record(
                "job-apply",
                "apply",
                "2.3.1",
                20,
                "1 to add, 0 to change"
            )]),
        )
        .with_api_route(
            PLANS,
            json!([change_record(
                "job-plan",
                "plan",
                "2.4.0",
                30,
                "0 to add, 2 to change"
            )]),
        )
        .with_script(vec![
            tool_call(
                "call-1",
                "list_change_records",
                json!({ "deployment_id": "payments-api" }),
            ),
            text_response("One apply, then a newer plan."),
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
    for expected in ["job-apply", "job-plan", "2.3.1", "2.4.0", "apply", "plan"] {
        assert!(
            content.contains(expected),
            "missing `{expected}` in {content}"
        );
    }
    assert!(
        content.find("job-plan").unwrap() < content.find("job-apply").unwrap(),
        "the newer plan should come first: {content}"
    );
}

#[tokio::test]
async fn list_change_records_with_a_type_filter_fetches_only_that_type() {
    // MUTATES is not registered: the strict stub would panic if it were requested.
    let result = scenario("show me the plans for payments-api")
        .with_api_route(
            PLANS,
            json!([change_record("job-plan", "plan", "2.4.0", 30, "x")]),
        )
        .with_script(vec![
            tool_call(
                "call-1",
                "list_change_records",
                json!({ "deployment_id": "payments-api", "change_type": "plan" }),
            ),
            text_response("One plan."),
        ])
        .run()
        .await;

    assert!(
        result.tool_errors().is_empty(),
        "{:?}",
        result.tool_result_blocks()
    );
    let (content, _) = result_sent_on_call(&result.calls, 1, "call-1");
    assert!(content.contains("job-plan"));
    assert!(result.requested_paths.contains(&PLANS.to_string()));
}

#[tokio::test]
async fn list_change_records_maps_apply_to_the_mutate_query() {
    let result = scenario("show me the applies for payments-api")
        .with_api_route(
            MUTATES,
            json!([change_record("job-apply", "apply", "2.3.1", 20, "x")]),
        )
        .with_script(vec![
            tool_call(
                "call-1",
                "list_change_records",
                json!({ "deployment_id": "payments-api", "change_type": "apply" }),
            ),
            text_response("One apply."),
        ])
        .run()
        .await;

    assert!(
        result.tool_errors().is_empty(),
        "{:?}",
        result.tool_result_blocks()
    );
    assert!(result.requested_paths.contains(&MUTATES.to_string()));
}

#[tokio::test]
async fn list_change_records_caps_the_output_and_says_how_many_were_left_out() {
    let result = scenario("what changed on payments-api?")
        .with_api_route(MUTATES, many("apply", "apply", 7, 100))
        .with_api_route(PLANS, many("plan", "plan", 7, 200))
        .with_script(vec![
            tool_call(
                "call-1",
                "list_change_records",
                json!({ "deployment_id": "payments-api" }),
            ),
            text_response("Lots of changes."),
        ])
        .run()
        .await;

    assert!(
        result.tool_errors().is_empty(),
        "{:?}",
        result.tool_result_blocks()
    );
    let (content, _) = result_sent_on_call(&result.calls, 1, "call-1");
    assert_eq!(content.matches("job_id: `").count(), 10, "{content}");
    assert!(
        content.contains("14"),
        "the total should be reported: {content}"
    );
    assert!(
        content.contains("plan-6"),
        "the newest record must survive: {content}"
    );
    assert!(
        !content.contains("apply-0"),
        "the oldest record should be cut: {content}"
    );
}

#[tokio::test]
async fn list_change_records_reports_an_empty_history_without_erroring() {
    let result = scenario("what changed on payments-api?")
        .with_api_route(MUTATES, json!([]))
        .with_api_route(PLANS, json!([]))
        .with_script(vec![
            tool_call(
                "call-1",
                "list_change_records",
                json!({ "deployment_id": "payments-api" }),
            ),
            text_response("Nothing yet."),
        ])
        .run()
        .await;

    assert!(
        result.tool_errors().is_empty(),
        "{:?}",
        result.tool_result_blocks()
    );
    let (content, _) = result_sent_on_call(&result.calls, 1, "call-1");
    assert!(content.contains("No change records"));
    assert!(content.contains("payments-api"));
}

#[tokio::test]
async fn list_change_records_rejects_an_unknown_change_type_before_any_request() {
    let result = scenario("show me the drift records")
        .with_script(vec![
            tool_call(
                "call-1",
                "list_change_records",
                json!({ "deployment_id": "payments-api", "change_type": "drift" }),
            ),
            text_response("Which type?"),
        ])
        .run()
        .await;

    let (content, is_error) = result_sent_on_call(&result.calls, 1, "call-1");
    assert!(is_error);
    assert!(content.contains("change_type"));
    assert!(!result.requested_paths.iter().any(|p| p.starts_with(BASE)));
}

#[tokio::test]
async fn list_change_records_requires_a_deployment_id() {
    let result = scenario("what changed?")
        .with_script(vec![
            tool_call("call-1", "list_change_records", json!({})),
            text_response("Which deployment?"),
        ])
        .run()
        .await;

    let (content, is_error) = result_sent_on_call(&result.calls, 1, "call-1");
    assert!(is_error);
    assert!(content.contains("deployment_id"));
}

#[tokio::test]
async fn list_change_records_validates_the_region_before_the_main_request() {
    let result = scenario("what changed in europe?")
        .with_script(vec![
            tool_call(
                "call-1",
                "list_change_records",
                json!({ "deployment_id": "payments-api", "region": "eu" }),
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
async fn namespaced_environment_and_deployment_ids_keep_their_slashes_in_the_path() {
    // The router splits `env1/env2/dep1/dep2`, so real ids are two segments each.
    let plans = "/api/v1/change_records/123456789012/us-west-2/aws/prod/s3bucket/logs?change_type=plan&limit=10";
    let result = scenario("show the plans for s3bucket/logs")
        .with_api_route(plans, json!([]))
        .with_script(vec![
            tool_call(
                "call-1",
                "list_change_records",
                json!({
                    "deployment_id": "s3bucket/logs",
                    "environment_id": "aws/prod",
                    "change_type": "plan"
                }),
            ),
            text_response("None."),
        ])
        .run()
        .await;

    assert!(
        result.tool_errors().is_empty(),
        "{:?}",
        result.tool_result_blocks()
    );
    assert!(result.requested_paths.contains(&plans.to_string()));
}

#[tokio::test]
async fn describe_change_record_shows_the_plan_and_the_resource_changes() {
    let mut record = change_record(
        "job-77",
        "apply",
        "2.3.1",
        20,
        "Plan: 1 to add, 0 to change, 0 to destroy.",
    );
    record["resource_changes"] = json!([resource_change("aws_s3_bucket.logs", "create")]);
    let result = scenario("what did job-77 do?")
        .with_api_route(ONE_MUTATE, record)
        .with_script(vec![
            tool_call(
                "call-1",
                "describe_change_record",
                json!({ "deployment_id": "payments-api", "job_id": "job-77" }),
            ),
            text_response("It added the bucket."),
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
    assert!(content.contains("1 to add"), "{content}");
    assert!(content.contains("aws_s3_bucket.logs"), "{content}");
    assert!(content.contains("create"), "{content}");
    assert!(
        content.contains("tags.team"),
        "changed attribute paths are useful: {content}"
    );
}

#[tokio::test]
async fn describe_change_record_never_echoes_attribute_or_variable_values() {
    let mut record = change_record("job-77", "apply", "2.3.1", 20, "1 to add");
    record["resource_changes"] = json!([resource_change("aws_s3_bucket.logs", "create")]);
    let result = scenario("what did job-77 do?")
        .with_api_route(ONE_MUTATE, record)
        .with_script(vec![
            tool_call(
                "call-1",
                "describe_change_record",
                json!({ "deployment_id": "payments-api", "job_id": "job-77" }),
            ),
            text_response("Done."),
        ])
        .run()
        .await;

    let (content, _) = result_sent_on_call(&result.calls, 1, "call-1");
    assert!(
        !content.contains("hunter2"),
        "a secret reached the model: {content}"
    );
    assert!(
        content.contains("db_password"),
        "variable names are fine: {content}"
    );
}

#[tokio::test]
async fn describe_change_record_truncates_a_very_long_plan() {
    let plan = format!("{}TAIL-MARKER", "x".repeat(6000));
    let result = scenario("what did job-77 do?")
        .with_api_route(
            ONE_MUTATE,
            change_record("job-77", "apply", "2.3.1", 20, &plan),
        )
        .with_script(vec![
            tool_call(
                "call-1",
                "describe_change_record",
                json!({ "deployment_id": "payments-api", "job_id": "job-77" }),
            ),
            text_response("Long plan."),
        ])
        .run()
        .await;

    let (content, _) = result_sent_on_call(&result.calls, 1, "call-1");
    assert!(
        content.contains("truncated"),
        "{}",
        &content[..200.min(content.len())]
    );
    assert!(!content.contains("TAIL-MARKER"));
}

#[tokio::test]
async fn describe_change_record_falls_back_from_apply_to_plan_when_no_type_is_given() {
    // The model usually has a job_id but not its type. The mutate lookup 404s,
    // which is the expected miss, so this scenario opts in to unmatched requests.
    let result = scenario("what did job-77 do?")
        .with_api_route(
            ONE_PLAN,
            change_record("job-77", "plan", "2.4.0", 30, "Plan: 2 to change."),
        )
        .allow_unmatched_requests()
        .with_script(vec![
            tool_call(
                "call-1",
                "describe_change_record",
                json!({ "deployment_id": "payments-api", "job_id": "job-77" }),
            ),
            text_response("It was a plan."),
        ])
        .run()
        .await;

    assert!(
        result.tool_errors().is_empty(),
        "{:?}",
        result.tool_result_blocks()
    );
    let (content, _) = result_sent_on_call(&result.calls, 1, "call-1");
    assert!(content.contains("2 to change"), "{content}");
    let mutate = result.requested_paths.iter().position(|p| p == ONE_MUTATE);
    let plan = result.requested_paths.iter().position(|p| p == ONE_PLAN);
    assert!(
        mutate.is_some() && plan > mutate,
        "{:?}",
        result.requested_paths
    );
}

#[tokio::test]
async fn describe_change_record_with_an_explicit_type_makes_one_lookup() {
    let result = scenario("what did the plan job-77 do?")
        .with_api_route(
            ONE_PLAN,
            change_record("job-77", "plan", "2.4.0", 30, "Plan: 2 to change."),
        )
        .with_script(vec![
            tool_call(
                "call-1",
                "describe_change_record",
                json!({
                    "deployment_id": "payments-api",
                    "job_id": "job-77",
                    "change_type": "plan"
                }),
            ),
            text_response("It was a plan."),
        ])
        .run()
        .await;

    assert!(
        result.tool_errors().is_empty(),
        "{:?}",
        result.tool_result_blocks()
    );
    assert!(!result.requested_paths.contains(&ONE_MUTATE.to_string()));
}

#[tokio::test]
async fn describe_change_record_reports_a_missing_record_without_erroring() {
    let result = scenario("what did job-99 do?")
        .allow_unmatched_requests()
        .with_script(vec![
            tool_call(
                "call-1",
                "describe_change_record",
                json!({ "deployment_id": "payments-api", "job_id": "job-99" }),
            ),
            text_response("No such record."),
        ])
        .run()
        .await;

    assert!(
        result.tool_errors().is_empty(),
        "{:?}",
        result.tool_result_blocks()
    );
    let (content, _) = result_sent_on_call(&result.calls, 1, "call-1");
    assert!(content.contains("job-99"));
    assert!(content.contains("No change record"));
}

#[tokio::test]
async fn describe_change_record_requires_a_job_id() {
    let result = scenario("describe the change")
        .with_script(vec![
            tool_call(
                "call-1",
                "describe_change_record",
                json!({ "deployment_id": "payments-api" }),
            ),
            text_response("Which job?"),
        ])
        .run()
        .await;

    let (content, is_error) = result_sent_on_call(&result.calls, 1, "call-1");
    assert!(is_error);
    assert!(content.contains("job_id"));
}
