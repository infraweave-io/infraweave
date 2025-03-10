use env_defs::{CloudProvider, NotificationData};

use crate::interface::GenericCloudHandler;

pub async fn publish_notification(
    handler: &GenericCloudHandler,
    notification: NotificationData,
) -> Result<String, anyhow::Error> {
    let notification_value = serde_json::to_value(&notification).unwrap();

    let payload = serde_json::json!({
        "event": "publish_notification",
        "data": &notification_value
    });

    match handler.run_function(&payload).await {
        Ok(_) => Ok("".to_string()),
        Err(e) => Err(anyhow::anyhow!("Failed to publish notification: {}", e)),
    }
}
