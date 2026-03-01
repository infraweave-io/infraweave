use anyhow::{anyhow, Result};
use azure_core::credentials::TokenCredential;
use azure_identity::DeveloperToolsCredential;
use base64::{engine::general_purpose, Engine as _};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::Arc;

pub struct UserDelegationKey {
    pub oid: String,
    pub tid: String,
    pub start: String,
    pub expiry: String,
    pub service: String,
    pub version: String,
    pub value: String,
}

fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{}>", tag);
    let end_tag = format!("</{}>", tag);
    let start = xml.find(&start_tag)? + start_tag.len();
    let end = xml.find(&end_tag)?;
    if start >= end {
        return None;
    }
    Some(xml[start..end].to_string())
}

pub async fn get_user_delegation_key(
    storage_account: &str,
    expires_in: i64,
) -> Result<UserDelegationKey> {
    let credential = Arc::new(
        DeveloperToolsCredential::new(None)
            .map_err(|e| anyhow!("Failed to create Azure credentials: {}", e))?,
    );

    let token_response = credential
        .get_token(&["https://storage.azure.com/.default"], None)
        .await
        .map_err(|e| anyhow!("Failed to get token: {}", e))?;

    let now = chrono::Utc::now();
    let start = now - chrono::Duration::minutes(5); // Allow for clock skew
    let expiry = now + chrono::Duration::seconds(expires_in);

    let start_str = start.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let expiry_str = expiry.format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let xml_body = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?><KeyInfo><Start>{}</Start><Expiry>{}</Expiry></KeyInfo>",
        start_str, expiry_str
    );

    let client = reqwest::Client::new();
    let delegation_key_url = format!(
        "https://{}.blob.core.windows.net/?restype=service&comp=userdelegationkey",
        storage_account
    );

    let response = client
        .post(&delegation_key_url)
        .header("x-ms-version", "2021-06-08")
        .bearer_auth(token_response.token.secret())
        .body(xml_body)
        .send()
        .await?;

    if !response.status().is_success() {
        let text = response.text().await?;
        return Err(anyhow!("Failed to get user delegation key: {}", text));
    }

    let response_text = response.text().await?;

    Ok(UserDelegationKey {
        oid: extract_xml_tag(&response_text, "SignedOid")
            .ok_or_else(|| anyhow!("Missing SignedOid"))?,
        tid: extract_xml_tag(&response_text, "SignedTid")
            .ok_or_else(|| anyhow!("Missing SignedTid"))?,
        start: extract_xml_tag(&response_text, "SignedStart")
            .ok_or_else(|| anyhow!("Missing SignedStart"))?,
        expiry: extract_xml_tag(&response_text, "SignedExpiry")
            .ok_or_else(|| anyhow!("Missing SignedExpiry"))?,
        service: extract_xml_tag(&response_text, "SignedService")
            .ok_or_else(|| anyhow!("Missing SignedService"))?,
        version: extract_xml_tag(&response_text, "SignedVersion")
            .ok_or_else(|| anyhow!("Missing SignedVersion"))?,
        value: extract_xml_tag(&response_text, "Value").ok_or_else(|| anyhow!("Missing Value"))?,
    })
}

pub fn create_user_delegation_sas_url(
    storage_account: &str,
    container_name: &str,
    blob_name: &str,
    key: &UserDelegationKey,
) -> Result<String> {
    // 2. Generate SAS token using the User Delegation Key
    let signed_permissions = "r";
    let signed_start = "";
    let signed_expiry = &key.expiry; // SAS expiry matches key expiry
    let canonical_resource = format!("/blob/{}/{}/{}", storage_account, container_name, blob_name);
    let _signed_identifier = "";
    let signed_ip = "";
    let signed_protocol = "https";
    let signed_version = "2021-06-08";
    let signed_resource = "b";
    let signed_snapshot_time = "";
    let signed_encryption_scope = "";
    let signed_cache_control = "";
    let signed_content_disposition = "";
    let signed_content_encoding = "";
    let signed_content_language = "";
    let _signed_content_type = "";

    let signed_preauthorized_user_object_id = "";
    let signed_preauthorized_user_tenant_id = "";
    let signed_correlation_id = "";

    // String to sign for User Delegation SAS (version 2020-02-10 and later)
    let string_to_sign = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        signed_permissions,
        signed_start,
        signed_expiry,
        canonical_resource,
        key.oid,
        key.tid,
        key.start,
        key.expiry,
        key.service,
        key.version,
        signed_preauthorized_user_object_id,
        signed_preauthorized_user_tenant_id,
        signed_correlation_id,
        signed_ip,
        signed_protocol,
        signed_version,
        signed_resource,
        signed_snapshot_time,
        signed_encryption_scope,
        signed_cache_control,
        signed_content_disposition,
        signed_content_encoding,
        signed_content_language,
        _signed_content_type
    );

    // Decode key value and sign
    let key_bytes = general_purpose::STANDARD
        .decode(&key.value)
        .map_err(|e| anyhow!("Failed to decode key value: {}", e))?;

    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(&key_bytes)
        .map_err(|e| anyhow!("Failed to create HMAC: {}", e))?;
    mac.update(string_to_sign.as_bytes());
    let signature = general_purpose::STANDARD.encode(mac.finalize().into_bytes());

    // Build SAS query parameters
    let sas_params = format!(
        "sv={}&sr={}&sp={}&se={}&spr={}&skoid={}&sktid={}&skt={}&ske={}&sks={}&skv={}&sig={}",
        signed_version,
        signed_resource,
        signed_permissions,
        signed_expiry,
        signed_protocol,
        key.oid,
        key.tid,
        key.start,
        key.expiry,
        key.service,
        key.version,
        urlencoding::encode(&signature),
    );

    Ok(format!(
        "https://{}.blob.core.windows.net/{}/{}?{}",
        storage_account, container_name, blob_name, sas_params
    ))
}
