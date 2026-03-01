use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishJob {
    pub job_id: String,
    pub status: PublishJobStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<PublishJobResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at: i64, // Unix timestamp
    pub ttl: i64,        // Unix timestamp for DynamoDB TTL (7 days from creation)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PublishJobStatus {
    Processing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishJobResult {
    pub track: String,
    pub module_name: String,
    pub version: String,
}

impl PublishJob {
    pub fn new(job_id: String) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            job_id,
            status: PublishJobStatus::Processing,
            result: None,
            error: None,
            created_at: now,
            ttl: now + 7 * 24 * 60 * 60, // 7 days
        }
    }

    pub fn complete(mut self, track: String, module_name: String, version: String) -> Self {
        self.status = PublishJobStatus::Completed;
        self.result = Some(PublishJobResult {
            track,
            module_name,
            version,
        });
        self
    }

    pub fn fail(mut self, error: String) -> Self {
        self.status = PublishJobStatus::Failed;
        self.error = Some(error);
        self
    }
}

pub fn get_publish_job_identifier(job_id: &str) -> String {
    format!("publish-job#{}", job_id)
}
