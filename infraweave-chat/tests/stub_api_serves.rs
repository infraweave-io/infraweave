mod support;

use infraweave_tools::ApiClient;
use serde_json::json;
use support::stub_api::StubApi;

#[tokio::test]
async fn serves_canned_json_and_records_requested_paths() {
    let stub = StubApi::start(vec![(
        "/api/v1/projects",
        json!([{ "project_id": "123456789012", "regions": ["us-west-2"] }]),
    )])
    .await;

    let api = ApiClient::new(stub.endpoint(), "test-token").expect("client builds");
    let value = api
        .get_json("/api/v1/projects")
        .await
        .expect("stub should serve projects");

    assert_eq!(value[0]["project_id"], "123456789012");
    assert_eq!(stub.requests(), vec!["/api/v1/projects".to_string()]);
}

#[tokio::test]
async fn unknown_paths_return_404_so_missing_fixtures_are_obvious() {
    let stub = StubApi::start(vec![]).await;
    let api = ApiClient::new(stub.endpoint(), "test-token").expect("client builds");

    // get_optional maps 404 to Ok(None) - that is the contract tools rely on.
    let missing = api
        .get_optional("/api/v1/deployment/p/r/e/d")
        .await
        .expect("404 should not be an error for get_optional");
    assert!(missing.is_none());
}

#[tokio::test]
async fn a_route_without_a_query_does_not_match_a_request_with_one() {
    let stub = StubApi::start(vec![("/api/v1/modules", json!([]))]).await;
    let api = ApiClient::new(stub.endpoint(), "test-token").expect("client builds");

    // Routes match the exact path and query; wrong query params must not still
    // get the fixture.
    let missing = api
        .get_optional("/api/v1/modules?track=dev")
        .await
        .expect("404 should not be an error for get_optional");
    assert!(missing.is_none());
}

#[tokio::test]
async fn requests_without_a_fixture_are_recorded_as_unmatched() {
    let stub = StubApi::start(vec![("/api/v1/projects", json!([]))]).await;
    let api = ApiClient::new(stub.endpoint(), "test-token").expect("client builds");

    api.get_json("/api/v1/projects").await.expect("served");
    let _ = api.get_optional("/api/v1/deployment/p/r/e/d").await;

    assert_eq!(
        stub.unmatched(),
        vec!["/api/v1/deployment/p/r/e/d".to_string()]
    );
}
