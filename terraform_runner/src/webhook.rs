use reqwest::Client;
use serde_json::json;

pub async fn post_webhook(webhook_url: &str, message: &str) -> Result<(), anyhow::Error> {
    let client = Client::new();

    let payload = json!({
        "text": message,
    });

    let response = client.post(webhook_url).json(&payload).send().await?;

    if response.status().is_success() {
        println!("Message sent successfully!");
    } else {
        println!("Failed to send message: {}", response.status());
    }

    Ok(())
}
