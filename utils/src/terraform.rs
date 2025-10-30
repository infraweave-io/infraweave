use env_defs::TfLockProvider;
use serde_json::Value;

pub fn get_provider_url_key(
    tf_lock_provider: &TfLockProvider,
    target: &str,
    category: &str,
) -> (String, String) {
    let parts: Vec<&str> = tf_lock_provider.source.split('/').collect();
    // parts: ["registry.terraform.io", "hashicorp", "aws"]
    let namespace = parts[1];
    let provider = parts[2];

    let prefix = format!(
        "terraform-provider-{provider}_{version}",
        provider = provider,
        version = tf_lock_provider.version
    );
    let file = match category {
        // "index_json" => format!("index.json"),
        "provider_binary" => format!("{prefix}_{target}.zip"),
        "shasum" => format!("{prefix}_SHA256SUMS"),
        "signature" => format!("{prefix}_SHA256SUMS.72D7468F.sig"), // New Hashicorp signature after incident HCSEC-2021-12 (v0.15.1 and later)
        _ => panic!("Invalid category"),
    };

    let download_url = format!(
        "https://releases.hashicorp.com/terraform-provider-{provider}/{version}/{file}",
        version = tf_lock_provider.version,
    );
    let key = format!("registry.terraform.io/{namespace}/{provider}/{file}",);
    (download_url, key)
}

#[derive(Debug, Clone)]
pub struct DestructiveChange {
    pub address: String,
    pub action: String, // "delete" or "replace"
}

pub fn plan_get_destructive_changes(plan_json: &Value) -> Vec<DestructiveChange> {
    plan_json
        .get("resource_changes")
        .and_then(|v| v.as_array())
        .map(|changes| {
            changes
                .iter()
                .filter_map(extract_destructive_change)
                .collect()
        })
        .unwrap_or_default()
}

fn extract_destructive_change(resource_change: &Value) -> Option<DestructiveChange> {
    // Terraform JSON structure: resource_change.change.actions
    // where "change" is a field containing the actual change details
    let actions = resource_change.get("change")?.get("actions")?.as_array()?;

    // Only process if this change includes a delete action
    if !actions.iter().any(|a| a.as_str() == Some("delete")) {
        return None;
    }

    let address = resource_change
        .get("address")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let action = if actions.len() > 1 {
        "replace"
    } else {
        "delete"
    }
    .to_string();

    Some(DestructiveChange { address, action })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_plan_get_destructive_changes_with_delete() {
        let plan_json = json!({
            "resource_changes": [{
                "address": "aws_s3_bucket.example",
                "change": {
                    "actions": ["delete"]
                }
            }]
        });
        let changes = plan_get_destructive_changes(&plan_json);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].address, "aws_s3_bucket.example");
        assert_eq!(changes[0].action, "delete");
    }

    #[test]
    fn test_plan_get_destructive_changes_with_replace() {
        let plan_json = json!({
            "resource_changes": [{
                "address": "aws_instance.web",
                "change": {
                    "actions": ["delete", "create"]
                }
            }]
        });
        let changes = plan_get_destructive_changes(&plan_json);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].address, "aws_instance.web");
        assert_eq!(changes[0].action, "replace");
    }

    #[test]
    fn test_plan_get_destructive_changes_mixed() {
        let plan_json = json!({
            "resource_changes": [
                {
                    "address": "aws_s3_bucket.old",
                    "change": {
                        "actions": ["delete"]
                    }
                },
                {
                    "address": "aws_instance.web",
                    "change": {
                        "actions": ["delete", "create"]
                    }
                },
                {
                    "address": "aws_s3_bucket.new",
                    "change": {
                        "actions": ["create"]
                    }
                }
            ]
        });
        let changes = plan_get_destructive_changes(&plan_json);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].address, "aws_s3_bucket.old");
        assert_eq!(changes[0].action, "delete");
        assert_eq!(changes[1].address, "aws_instance.web");
        assert_eq!(changes[1].action, "replace");
    }

    #[test]
    fn test_plan_get_destructive_changes_no_destructive() {
        let plan_json = json!({
            "resource_changes": [{
                "address": "aws_s3_bucket.new",
                "change": {
                    "actions": ["create"]
                }
            }]
        });
        let changes = plan_get_destructive_changes(&plan_json);
        assert_eq!(changes.len(), 0);
    }
}
