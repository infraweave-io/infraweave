use axum::body::Body;
use axum::http::Request;
use base64::{engine::general_purpose, Engine as _};
use env_common::interface::initialize_project_id_and_region;
use internal_api::aws_handlers as handlers;
use internal_api::http_router;
use internal_api::otel_tracing;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use log::{debug, info, warn};
use serde_json::Value;
use tower_http::trace::TraceLayer;
use tracing::{instrument, Span};

#[instrument(
    skip(event),
    fields(request_id, user_id, http_method, http_path, event_type)
)]
async fn unified_handler(event: LambdaEvent<Value>) -> Result<Value, Error> {
    let (payload, context) = event.into_parts();

    // Record request ID for easy debugging in CloudWatch Logs Insights
    let span = Span::current();
    span.record("request_id", &context.request_id.as_str());

    // Check if this is a direct invocation (has "event" field) or API Gateway request
    if let Some(event_type) = payload.get("event").and_then(|v| v.as_str()) {
        span.record("event_type", &event_type);
        // Direct Lambda invocation
        info!("Detected direct Lambda invocation: {}", event_type);

        let result = match event_type {
            "insert_db" => handlers::insert_db(&payload).await,
            "transact_write" => handlers::transact_write(&payload).await,
            "read_db" => handlers::read_db(&payload).await,
            "upload_file_base64" => handlers::upload_file_base64(&payload).await,
            "upload_file_url" => handlers::upload_file_url(&payload).await,
            "generate_presigned_url" => handlers::generate_presigned_url(&payload).await,
            "start_runner" => handlers::start_runner(&payload).await,
            "get_job_status" => handlers::get_job_status(&payload).await,
            "read_logs" => handlers::read_logs(&payload).await,
            "publish_notification" => handlers::publish_notification(&payload).await,
            "get_environment_variables" => handlers::get_environment_variables(&payload).await,
            _ => {
                warn!("Unknown event: {}", event_type);
                Err(anyhow::anyhow!("Unknown event: {}", event_type))
            }
        };

        return result.map_err(|e| Error::from(e.to_string()));
    }

    // This is an API Gateway HTTP request
    info!("Detected API Gateway HTTP request");

    // Extract HTTP request details from API Gateway event
    let http_method = payload
        .get("httpMethod")
        .or_else(|| {
            payload
                .get("requestContext")
                .and_then(|rc| rc.get("http"))
                .and_then(|h| h.get("method"))
        })
        .and_then(|v| v.as_str())
        .unwrap_or("GET");

    let path = payload
        .get("path")
        .or_else(|| payload.get("rawPath"))
        .and_then(|v| v.as_str())
        .unwrap_or("/");

    // Build full URI with query string parameters
    let uri = if let Some(query_params) = payload
        .get("queryStringParameters")
        .and_then(|v| v.as_object())
    {
        let query_string: Vec<String> = query_params
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|val| format!("{}={}", k, val)))
            .collect();
        if !query_string.is_empty() {
            format!("{}?{}", path, query_string.join("&"))
        } else {
            path.to_string()
        }
    } else {
        path.to_string()
    };

    info!("HTTP {} {}", http_method, uri);

    // Record HTTP context in span for debugging
    let span = Span::current();
    span.record("http_method", &http_method);
    span.record("http_path", &uri.as_str());

    // Build Axum request
    let mut request_builder = Request::builder().method(http_method).uri(&uri);

    // Add headers if present
    if let Some(headers) = payload.get("headers").and_then(|h| h.as_object()) {
        for (key, value) in headers {
            // SECURITY: Do not allow client to inject the auth user header
            if key.eq_ignore_ascii_case("x-auth-user") {
                continue;
            }
            if let Some(val_str) = value.as_str() {
                request_builder = request_builder.header(key.as_str(), val_str);
            }
        }
    }

    // Try to extract user identity from API Gateway context
    if let Some(request_context) = payload.get("requestContext") {
        // Debug: Log the full request context for IAM troubleshooting
        debug!(
            "Request context: {}",
            serde_json::to_string_pretty(request_context).unwrap_or_else(|_| "{}".to_string())
        );

        // First try JWT/Cognito claims (for Cognito-authenticated requests)
        let (maybe_user, maybe_allowed_projects) = request_context
            .get("authorizer")
            .map(|auth| {
                // Handle both direct claims and nested jwt.claims
                let claims = if let Some(c) = auth.get("claims") {
                    Some(c)
                } else if let Some(jwt) = auth.get("jwt") {
                    jwt.get("claims")
                } else {
                    None
                };

                let user = claims.and_then(|c| {
                    // Prefer Subject ID (sub) as the primary stable identifier (UUID)
                    c.get("sub")
                        .or_else(|| c.get("cognito:username"))
                        .or_else(|| c.get("email"))
                        .and_then(|v| v.as_str())
                });

                let allowed_projects = claims.and_then(|c| {
                    // Extract custom:allowed_projects claim
                    c.get("custom:allowed_projects").and_then(|v| v.as_str())
                });

                (user, allowed_projects)
            })
            .unwrap_or((None, None));

        if let Some(user) = maybe_user {
            info!("Authenticated user (Cognito JWT): {}", user);
            request_builder = request_builder.header("x-auth-user", user);

            // Record user ID in span for debugging
            span.record("user_id", &user);

            // Add allowed_projects header if present in JWT claims
            if let Some(allowed_projects) = maybe_allowed_projects {
                info!(
                    "Adding allowed_projects from JWT claims: {}",
                    allowed_projects
                );
                request_builder = request_builder.header("x-allowed-projects", allowed_projects);
            }
        } else {
            // Try IAM authentication (for IAM-signed requests to token bridge)
            // IAM auth info is in requestContext.authorizer.iam
            if let Some(authorizer) = request_context.get("authorizer") {
                if let Some(iam) = authorizer.get("iam") {
                    info!(
                        "IAM object: {}",
                        serde_json::to_string_pretty(iam).unwrap_or_else(|_| "{}".to_string())
                    );

                    let iam_user = iam
                        .get("userArn")
                        .or_else(|| iam.get("userId"))
                        .and_then(|v| v.as_str());

                    if let Some(user) = iam_user {
                        info!("Authenticated user (IAM): {}", user);
                        request_builder = request_builder.header("x-auth-user", user);
                    } else {
                        warn!("No user identity found in IAM authorizer");
                        warn!(
                            "Available IAM keys: {:?}",
                            iam.as_object().map(|o| o.keys().collect::<Vec<_>>())
                        );
                    }
                } else {
                    warn!("No IAM object found in authorizer");
                }
            } else {
                warn!("No authorizer found in request context");
            }
        }
    }

    // Add body if present
    let body = if let Some(body_str) = payload.get("body").and_then(|b| b.as_str()) {
        Body::from(body_str.to_string())
    } else {
        Body::empty()
    };

    let axum_request = request_builder.body(body).map_err(|e| {
        warn!("Failed to build request: {}", e);
        Error::from(format!("Failed to build request: {}", e))
    })?;

    // Process with Axum router
    let router = http_router::create_router().layer(TraceLayer::new_for_http());

    use tower::ServiceExt;
    let axum_response = match router.oneshot(axum_request).await {
        Ok(response) => response,
        Err(e) => {
            warn!("Router error: {:?}", e);
            return Err(Error::from(format!("Router error: {:?}", e)));
        }
    };

    info!("Got response with status: {}", axum_response.status());

    // Convert Axum response to API Gateway format
    let (parts, axum_body) = axum_response.into_parts();
    let body_bytes = axum::body::to_bytes(axum_body, usize::MAX)
        .await
        .map_err(|e| {
            warn!("Failed to read response body: {}", e);
            Error::from(format!("Failed to read response body: {}", e))
        })?;

    // Check Content-Type to determine if we should base64 encode
    let content_type = parts
        .headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let content_encoding = parts
        .headers
        .get("content-encoding")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let is_binary = (content_type.starts_with("application/")
        && !content_type.starts_with("application/json"))
        || content_type.starts_with("binary/")
        || !content_encoding.is_empty();

    let (response_body, is_base64_encoded) = if is_binary {
        let b64 = general_purpose::STANDARD.encode(&body_bytes);
        info!(
            "Binary/Compressed response (Content-Type: {}, Encoding: {}). Raw size: {} bytes. Base64 size: {}",
            content_type,
            content_encoding,
            body_bytes.len(),
            b64.len()
        );
        (b64, true)
    } else {
        let text = String::from_utf8_lossy(&body_bytes).to_string();
        info!(
            "Text response (Content-Type: {}). Length: {}. Body: {}",
            content_type,
            text.len(),
            text
        );
        (text, false)
    };

    // Build API Gateway response format
    let mut headers_map = serde_json::Map::new();
    for (key, value) in parts.headers.iter() {
        if let Ok(val_str) = value.to_str() {
            headers_map.insert(key.to_string(), Value::String(val_str.to_string()));
        }
    }

    // Add trace ID to response headers for authenticated requests only
    // Check if request was authenticated by looking at the request context
    let is_authenticated = payload
        .get("requestContext")
        .and_then(|rc| rc.get("authorizer"))
        .is_some();

    info!("Auth check: is_authenticated={}", is_authenticated);

    if is_authenticated {
        if let Ok(trace_id) = std::env::var("_X_AMZN_TRACE_ID") {
            // Extract just the Root trace ID (format: Root=1-xxx-xxx;Parent=...;Sampled=...)
            if let Some(root_part) = trace_id.split(';').next() {
                if let Some(id) = root_part.strip_prefix("Root=") {
                    headers_map.insert("X-Trace-Id".to_string(), Value::String(id.to_string()));
                }
            }
        }
    } else {
        info!("Skipping X-Trace-Id header - request not authenticated");
    }

    let api_gateway_response = serde_json::json!({
        "statusCode": parts.status.as_u16(),
        "headers": headers_map,
        "body": response_body,
        "isBase64Encoded": is_base64_encoded
    });

    info!("Returning API Gateway response");
    Ok(api_gateway_response)
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Initialize OTEL tracing first
    if let Err(e) = otel_tracing::init_tracing("internal-api") {
        eprintln!("Failed to initialize OpenTelemetry: {}", e);
        // Fall back to env_logger if OTEL init fails
        env_logger::init();
    }

    initialize_project_id_and_region().await;

    info!(
        "Starting unified internal-api Lambda handler (supports both direct invocation and HTTP)"
    );

    let result = lambda_runtime::run(service_fn(unified_handler)).await;

    // Shutdown tracing gracefully
    otel_tracing::shutdown_tracing();

    result
}
