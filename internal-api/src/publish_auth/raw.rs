//! Raw identity extractor.
//!
//! This is the fallback for identities without a first-class extractor. It
//! exposes trusted raw facts to Rego and does not try to infer provider
//! semantics such as repository, project, workflow, actor names, or AWS roles.

use serde_json::Value;

use super::PublishIdentity;

pub(super) fn extract(claims: &Value) -> Option<PublishIdentity> {
    let claims_object = claims.as_object()?;

    let mut facts = serde_json::Map::new();
    facts.insert("claims".to_string(), Value::Object(claims_object.clone()));
    copy_claim(claims, &mut facts, "iss", "issuer");
    copy_claim(claims, &mut facts, "sub", "subject");
    copy_claim(claims, &mut facts, "aud", "audience");
    copy_claim(claims, &mut facts, "email", "email");

    Some(PublishIdentity { facts })
}

fn copy_claim(
    claims: &Value,
    target: &mut serde_json::Map<String, Value>,
    claim_key: &str,
    target_key: &str,
) {
    if let Some(value) = claims.get(claim_key) {
        target.insert(target_key.to_string(), value.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::super::IdentityProvider;
    use serde_json::json;
    use serde_json::Value;

    #[test]
    fn exposes_raw_claims_and_generic_aliases() {
        let claims = json!({
            "iss": "https://gitlab.com",
            "sub": "project_path:infraweave/modules:ref_type:branch:ref:main",
            "aud": "infraweave",
            "project_path": "infraweave/modules"
        });
        let id = IdentityProvider::Raw.extract(&claims).unwrap();

        assert_eq!(
            id.facts.get("issuer").and_then(Value::as_str),
            Some("https://gitlab.com")
        );
        assert_eq!(
            id.facts.get("subject").and_then(Value::as_str),
            Some("project_path:infraweave/modules:ref_type:branch:ref:main")
        );
        assert_eq!(
            id.facts
                .get("claims")
                .and_then(|claims| claims.pointer("/project_path"))
                .and_then(Value::as_str),
            Some("infraweave/modules")
        );
    }

    #[test]
    fn rejects_non_object_claims() {
        assert!(IdentityProvider::Raw.extract(&json!("nope")).is_none());
    }
}
