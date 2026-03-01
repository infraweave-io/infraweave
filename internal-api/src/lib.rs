mod api_common;
#[cfg(feature = "aws")]
pub mod auth_handler;
#[cfg(feature = "aws")]
pub mod aws_handlers;
#[cfg(feature = "azure")]
pub mod azure_handlers;
mod common;
pub mod handlers;
pub mod http_router;
#[cfg(feature = "local")]
pub mod local_setup;
pub mod otel_tracing;
mod queries;

pub use common::CloudRuntime;
