use axum::{
    http::StatusCode,
    response::{IntoResponse, Json},
};
use log::error;
use serde_json::{json, Value};
use std::{error::Error as StdError, fmt};

use env_common::errors::ModuleError;

#[derive(Debug)]
struct BadRequestError(String);

impl fmt::Display for BadRequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl StdError for BadRequestError {}

#[derive(Debug)]
struct ConflictError(String);

impl fmt::Display for ConflictError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl StdError for ConflictError {}

pub(crate) fn bad_request(message: impl Into<String>) -> anyhow::Error {
    BadRequestError(message.into()).into()
}

pub(crate) fn conflict(message: impl Into<String>) -> anyhow::Error {
    ConflictError(message.into()).into()
}

fn status_code_for_module_error(e: &ModuleError) -> StatusCode {
    match e {
        ModuleError::ModuleVersionExists(_, _) => StatusCode::CONFLICT,
        ModuleError::ValidationError(_) => StatusCode::CONFLICT,
        ModuleError::InvalidTrack(_)
        | ModuleError::InvalidTrackPrereleaseVersion(_, _)
        | ModuleError::InvalidStableVersion
        | ModuleError::InvalidModuleSchema(_)
        | ModuleError::InvalidExampleVariable(_)
        | ModuleError::InvalidVariableNaming(_)
        | ModuleError::InvalidOutputNaming(_)
        | ModuleError::InvalidReference(_, _)
        | ModuleError::ModuleVersionNotSet(_)
        | ModuleError::ModuleVersionMissing(_)
        | ModuleError::DuplicateClaimNames(_)
        | ModuleError::CircularDependency(_)
        | ModuleError::SelfReferencingClaim(_, _, _)
        | ModuleError::StackModuleNamespaceIsSet(_)
        | ModuleError::TerraformLockfileExists()
        | ModuleError::TerraformLockfileEmpty
        | ModuleError::TerraformNoLockfile(_)
        | ModuleError::NoProvidersDefined(_)
        | ModuleError::NoRequiredProvidersDefined(_)
        | ModuleError::OutputKeyNotFound(_, _, _, _, _)
        | ModuleError::StackClaimReferenceNotFound(_, _, _, _)
        | ModuleError::UnresolvedReference(_, _) => StatusCode::BAD_REQUEST,
        ModuleError::ModuleVersionNotFound(_, _) => StatusCode::NOT_FOUND,
        ModuleError::UploadModuleError(_)
        | ModuleError::ZipError(_)
        | ModuleError::PublishError(_)
        | ModuleError::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub(crate) async fn handle_result(result: anyhow::Result<Value>) -> impl IntoResponse {
    match result {
        Ok(mut response) => {
            if let Some(obj) = response.as_object_mut() {
                if obj.contains_key("Items") {
                    let items = obj.remove("Items").unwrap();
                    let mut headers = axum::http::HeaderMap::new();

                    if let Some(next_token) = obj.remove("next_token") {
                        if let Some(token_str) = next_token.as_str() {
                            if let Ok(val) = axum::http::HeaderValue::from_str(token_str) {
                                headers.insert("x-next-token", val);
                            }
                        }
                    }

                    return (StatusCode::OK, headers, Json(items)).into_response();
                }
            }

            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => {
            let err_msg = e.to_string();
            let status = if let Some(module_err) = e.downcast_ref::<ModuleError>() {
                status_code_for_module_error(module_err)
            } else if e.downcast_ref::<BadRequestError>().is_some() {
                StatusCode::BAD_REQUEST
            } else if e.downcast_ref::<ConflictError>().is_some() {
                StatusCode::CONFLICT
            } else if err_msg.to_lowercase().contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };

            if status == StatusCode::INTERNAL_SERVER_ERROR {
                error!("Request failed: {:?}", e);
            }

            let response_msg = if status == StatusCode::INTERNAL_SERVER_ERROR {
                format!("{:?}", e)
            } else {
                err_msg
            };

            (
                status,
                Json(json!({
                    "error": response_msg
                })),
            )
                .into_response()
        }
    }
}
