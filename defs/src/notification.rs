use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct NotificationData {
    pub subject: String,            // Used to identify the type of notification
    pub message: serde_json::Value, // Value of the notification
}
