mod support;

use infraweave_chat::llm::{LlmClient, LlmContent, LlmMessage, LlmResponse, LlmUsage, StopReason};
use support::fake_llm::FakeLlm;

#[tokio::test]
async fn records_what_the_agent_sent_and_replays_the_script() {
    let fake = FakeLlm::new(vec![LlmResponse {
        content: vec![LlmContent::Text {
            text: "ok".to_string(),
        }],
        stop_reason: StopReason::EndTurn,
        usage: LlmUsage::default(),
    }]);

    let resp = fake
        .converse("system text", &[LlmMessage::user_text("hello")], &[])
        .await
        .expect("scripted response");

    assert_eq!(resp.stop_reason, StopReason::EndTurn);

    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].system, "system text");
    assert_eq!(calls[0].messages.len(), 1);
}

#[tokio::test]
async fn running_out_of_script_is_a_clear_error() {
    let fake = FakeLlm::new(vec![]);
    let err = fake
        .converse("s", &[LlmMessage::user_text("hi")], &[])
        .await
        .expect_err("empty script should error");
    assert!(err.to_string().contains("ran out of scripted responses"));
}
