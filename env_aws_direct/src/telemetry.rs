//! AWS-specific OpenTelemetry span export: a SigV4-signing OTLP/HTTP client
//! pointed at the AWS X-Ray endpoint. The generic tracing setup (log
//! formatting, the OTLP/gRPC exporter, tracer provider wiring) lives in
//! `env_utils::otel_tracing`; this module only provides the X-Ray exporter.

use async_trait::async_trait;
use aws_credential_types::provider::ProvideCredentials;
use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningSettings};
use env_utils::otel_tracing::SpanExporter;
use opentelemetry_http::{Bytes, HttpClient, HttpError, Request, Response};
use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};
use std::fmt::Debug;
use std::time::{Duration, SystemTime};

#[derive(Debug)]
struct AwsSigV4HttpClient {
    service: &'static str,
    region: String,
    config: tokio::sync::OnceCell<aws_config::SdkConfig>,
    client: reqwest::Client,
}

impl AwsSigV4HttpClient {
    fn new(service: &'static str, region: String) -> Self {
        Self {
            service,
            region,
            config: tokio::sync::OnceCell::new(),
            client: reqwest::Client::new(),
        }
    }

    async fn sdk_config(&self) -> &aws_config::SdkConfig {
        self.config
            .get_or_init(|| async {
                aws_config::from_env()
                    .region(aws_config::Region::new(self.region.clone()))
                    .load()
                    .await
            })
            .await
    }
}

#[async_trait]
impl HttpClient for AwsSigV4HttpClient {
    async fn send_bytes(&self, request: Request<Bytes>) -> Result<Response<Bytes>, HttpError> {
        let config = self.sdk_config().await;
        let credentials_provider = config
            .credentials_provider()
            .ok_or("No AWS credentials provider found")?;
        let credentials = credentials_provider.provide_credentials().await?;
        let identity = aws_smithy_runtime_api::client::identity::Identity::new(credentials, None);

        let (parts, body) = request.into_parts();
        let method = parts.method.as_str().to_string();
        let url = parts.uri.to_string();
        let headers = parts
            .headers
            .iter()
            .filter_map(|(name, value)| {
                Some((name.as_str().to_string(), value.to_str().ok()?.to_string()))
            })
            .collect::<Vec<_>>();

        let signing_settings = SigningSettings::default();
        let signing_params = aws_sigv4::sign::v4::SigningParams::builder()
            .identity(&identity)
            .region(&self.region)
            .name(self.service)
            .time(SystemTime::now())
            .settings(signing_settings)
            .build()?;

        let signable = SignableRequest::new(
            &method,
            &url,
            headers
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
            SignableBody::Bytes(&body),
        )?;
        let (signing_instructions, _signature) =
            sign(signable, &signing_params.into())?.into_parts();

        let mut reqwest_request = self
            .client
            .request(reqwest::Method::from_bytes(method.as_bytes())?, &url);

        for (name, value) in headers {
            reqwest_request = reqwest_request.header(name, value);
        }

        for (name, value) in signing_instructions.headers() {
            reqwest_request = reqwest_request.header(name, value);
        }

        for (name, value) in signing_instructions.params() {
            reqwest_request = reqwest_request.query(&[(name, value.as_ref())]);
        }

        let mut response = reqwest_request.body(body).send().await?;
        let headers = std::mem::take(response.headers_mut());
        let mut http_response = Response::builder()
            .status(response.status())
            .body(response.bytes().await?)?;
        *http_response.headers_mut() = headers;

        Ok(http_response)
    }
}

fn aws_region_from_env() -> anyhow::Result<String> {
    std::env::var("TELEMETRY_AWS_REGION")
        .or_else(|_| std::env::var("AWS_REGION"))
        .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
        .map_err(|_| {
            anyhow::anyhow!(
                "TELEMETRY_EXPORTER=xray requires TELEMETRY_AWS_REGION, AWS_REGION, or AWS_DEFAULT_REGION"
            )
        })
}

/// Build a span exporter that ships spans to the AWS X-Ray OTLP/HTTP endpoint,
/// signing each request with SigV4. The region is resolved from
/// `TELEMETRY_AWS_REGION`, `AWS_REGION`, or `AWS_DEFAULT_REGION`.
pub fn xray_span_exporter() -> anyhow::Result<env_utils::otel_tracing::BoxedSpanExporter> {
    let region = aws_region_from_env()?;
    let endpoint = format!("https://xray.{region}.amazonaws.com/v1/traces");
    let exporter = SpanExporter::builder()
        .with_http()
        .with_endpoint(endpoint)
        .with_timeout(Duration::from_secs(3))
        .with_http_client(AwsSigV4HttpClient::new("xray", region))
        .build()?;
    Ok(env_utils::otel_tracing::BoxedSpanExporter::new(exporter))
}
