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

pub fn plan_has_destructive_changes(plan_json: &Value) -> bool {
    if let Some(resource_changes) = plan_json.get("resource_changes") {
        if let Some(changes_array) = resource_changes.as_array() {
            for change in changes_array {
                if let Some(change_obj) = change.get("change") {
                    if let Some(actions) = change_obj.get("actions") {
                        if let Some(actions_array) = actions.as_array() {
                            // Check if any action is "delete" (includes both destroy and replace)
                            for action in actions_array {
                                if let Some(action_str) = action.as_str() {
                                    if action_str == "delete" {
                                        return true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_plan_has_destructive_changes_with_delete() {
        let plan_json = json!({
            "resource_changes": [{
                "change": {
                    "actions": ["delete"]
                }
            }]
        });
        assert_eq!(plan_has_destructive_changes(&plan_json), true);
    }

    #[test]
    fn test_plan_has_destructive_changes_with_replace() {
        let plan_json = json!({
            "resource_changes": [{
                "change": {
                    "actions": ["delete", "create"]
                }
            }]
        });
        assert_eq!(plan_has_destructive_changes(&plan_json), true);
    }

    #[test]
    fn test_plan_has_destructive_changes_with_create_only() {
        let plan_json = json!({
            "resource_changes": [{
                "change": {
                    "actions": ["create"]
                }
            }]
        });
        assert_eq!(plan_has_destructive_changes(&plan_json), false);
    }

    #[test]
    fn test_plan_has_destructive_changes_no_resource_changes() {
        let plan_json = json!({});
        assert_eq!(plan_has_destructive_changes(&plan_json), false);
    }
}
