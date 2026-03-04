use async_trait::async_trait;
use azure_core::credentials::{AccessToken, TokenCredential, TokenRequestOptions};
use azure_core::error::Result;
use reqwest::Client;
use serde::Deserialize;
use std::time::{Duration, UNIX_EPOCH};

#[derive(Debug, Deserialize)]
struct ImdsTokenResponse {
    access_token: String,
    expires_on: String,
}

#[derive(Debug)]
pub struct CustomImdsCredential {
    client: Client,
}

impl CustomImdsCredential {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
        }
    }
}

#[async_trait]
impl TokenCredential for CustomImdsCredential {
    async fn get_token(
        &self,
        scopes: &[&str],
        _options: Option<TokenRequestOptions<'_>>,
    ) -> Result<AccessToken> {
        let resource = scopes[0].trim_end_matches("/.default");
        let url = format!(
            "http://169.254.169.254/metadata/identity/oauth2/token?api-version=2019-08-01&resource={}",
            resource
        );

        let response = self
            .client
            .get(&url)
            .header("Metadata", "true")
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();

        let imds_resp: ImdsTokenResponse = response.json().await.unwrap();
        let expires_on_secs: u64 = imds_resp.expires_on.parse().unwrap_or(0);
        let expires_on = UNIX_EPOCH + Duration::from_secs(expires_on_secs);

        Ok(AccessToken {
            token: imds_resp.access_token.into(),
            expires_on: expires_on.into(),
        })
    }
}
