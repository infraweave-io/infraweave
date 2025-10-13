use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use axum_server::tls_rustls::RustlsConfig;
use env_common::interface::GenericCloudHandler;
use kube::api::DynamicObject;
use rustls::pki_types::CertificateDer;
use rustls::ServerConfig;
use rustls_pemfile::certs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufReader, Cursor};
use std::path::PathBuf;
use std::sync::Arc;

use crate::validation::validate_claim;

/// Kubernetes AdmissionReview request structure
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdmissionReview {
    pub api_version: String,
    pub kind: String,
    pub request: AdmissionRequest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdmissionRequest {
    pub uid: String,
    pub kind: GroupVersionKind,
    pub resource: GroupVersionResource,
    pub operation: String,
    pub object: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupVersionKind {
    pub group: String,
    pub version: String,
    pub kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupVersionResource {
    pub group: String,
    pub version: String,
    pub resource: String,
}

/// Kubernetes AdmissionReview response structure
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdmissionReviewResponse {
    pub api_version: String,
    pub kind: String,
    pub response: AdmissionResponse,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdmissionResponse {
    pub uid: String,
    pub allowed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Status {
    pub message: String,
}

#[derive(Clone)]
struct WebhookState {
    handler: GenericCloudHandler,
}

/// Handler for admission webhook requests
async fn validate_handler(
    State(state): State<Arc<WebhookState>>,
    Json(review): Json<AdmissionReview>,
) -> impl IntoResponse {
    // Parse the object from the request
    let claim: DynamicObject = match serde_json::from_value(review.request.object.clone()) {
        Ok(obj) => obj,
        Err(e) => {
            eprintln!("Failed to parse object: {:?}", e);
            return (
                StatusCode::OK,
                Json(AdmissionReviewResponse {
                    api_version: "admission.k8s.io/v1".to_string(),
                    kind: "AdmissionReview".to_string(),
                    response: AdmissionResponse {
                        uid: review.request.uid,
                        allowed: false,
                        status: Some(Status {
                            message: format!("Failed to parse object: {}", e),
                        }),
                    },
                }),
            );
        }
    };

    // Validate the claim using the real validation logic
    let (is_valid, message) = validate_claim(&state.handler, &claim).await;

    let response = AdmissionReviewResponse {
        api_version: "admission.k8s.io/v1".to_string(),
        kind: "AdmissionReview".to_string(),
        response: AdmissionResponse {
            uid: review.request.uid,
            allowed: is_valid,
            status: if is_valid {
                None
            } else {
                Some(Status { message })
            },
        },
    };

    (StatusCode::OK, Json(response))
}

/// Health check endpoint
async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

/// Creates and returns the webhook server router
pub fn create_webhook_router(handler: GenericCloudHandler) -> Router {
    let state = Arc::new(WebhookState { handler });

    Router::new()
        .route("/validate", post(validate_handler))
        .route("/health", axum::routing::get(health_handler))
        .with_state(state)
}

/// Starts the webhook server on the specified port with TLS support
pub async fn start_webhook_server(
    handler: GenericCloudHandler,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let app = create_webhook_router(handler);
    let addr = format!("0.0.0.0:{}", port);

    // Check if TLS certificates are available
    let cert_path = PathBuf::from("/etc/webhook/certs/tls.crt");
    let key_path = PathBuf::from("/etc/webhook/certs/tls.key");

    if cert_path.exists() && key_path.exists() {
        println!("Starting webhook server with TLS on {}", addr);

        // Load certificates
        let cert_pem = fs::read(&cert_path)?;
        let key_pem = fs::read(&key_path)?;

        // Parse certificates
        let certs: Vec<CertificateDer> =
            certs(&mut BufReader::new(Cursor::new(&cert_pem))).collect::<Result<Vec<_>, _>>()?;

        // Use rustls_pemfile::private_key which auto-detects format
        let mut key_reader = BufReader::new(Cursor::new(&key_pem));
        let private_key = rustls_pemfile::private_key(&mut key_reader)?
            .ok_or("No private key found in key file - file may be empty or not in PEM format")?;

        // Build TLS configuration
        let tls_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, private_key)?;

        let rustls_config = RustlsConfig::from_config(Arc::new(tls_config));

        // Start HTTPS server
        axum_server::bind_rustls(addr.parse()?, rustls_config)
            .serve(app.into_make_service())
            .await?;
    } else {
        println!(
            "TLS certificates not found, starting webhook server without TLS on {}",
            addr
        );
        println!("  Expected cert at: {:?}", cert_path);
        println!("  Expected key at: {:?}", key_path);

        // Fallback to HTTP for development/testing
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        axum::serve(listener, app).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    // Helper to create a test handler
    async fn create_test_handler() -> GenericCloudHandler {
        // For unit tests, we'll need a mock or test handler
        // This would require mocking infrastructure
        // For now, these tests are commented out - use integration tests instead
        unimplemented!("Use integration tests in integration-tests/tests/operator.rs")
    }

    #[tokio::test]
    #[ignore] // Requires test infrastructure setup
    async fn test_health_endpoint() {
        let handler = create_test_handler().await;
        let app = create_webhook_router(handler);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    #[ignore] // Requires test infrastructure setup
    async fn test_validate_endpoint() {
        let handler = create_test_handler().await;
        let app = create_webhook_router(handler);

        let admission_review = serde_json::json!({
            "apiVersion": "admission.k8s.io/v1",
            "kind": "AdmissionReview",
            "request": {
                "uid": "test-uid-123",
                "kind": {
                    "group": "infraweave.io",
                    "version": "v1",
                    "kind": "S3Bucket"
                },
                "resource": {
                    "group": "infraweave.io",
                    "version": "v1",
                    "resource": "s3buckets"
                },
                "operation": "CREATE",
                "object": {
                    "apiVersion": "infraweave.io/v1",
                    "kind": "S3Bucket",
                    "metadata": {
                        "name": "test-bucket",
                        "namespace": "default"
                    },
                    "spec": {
                        "moduleVersion": "1.0.0",
                        "region": "us-west-2",
                        "variables": {
                            "bucketName": "my-test-bucket"
                        }
                    }
                }
            }
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/validate")
                    .method("POST")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&admission_review).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let review_response: AdmissionReviewResponse = serde_json::from_slice(&body).unwrap();

        assert_eq!(review_response.response.uid, "test-uid-123");
    }
}
