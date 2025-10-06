use anyhow::Result;
use tokio::sync::mpsc;

use super::app::{Deployment, Module};

/// Messages sent from background tasks to the UI thread
#[derive(Debug, Clone)]
pub enum BackgroundMessage {
    // Data loading results
    ModulesLoaded(Result<Vec<Module>, String>),
    StacksLoaded(Result<Vec<Module>, String>),
    DeploymentsLoaded(Result<Vec<Deployment>, String>),

    // Detail loading results
    ModuleDetailLoaded(Result<String, String>),
    StackDetailLoaded(Result<String, String>),
    DeploymentDetailLoaded(Result<Option<env_defs::DeploymentResp>, String>),

    // Version loading results
    ModuleVersionsLoaded(Result<(String, String, usize, Vec<String>, Vec<Module>), String>),
    StackVersionsLoaded(Result<(String, String, usize, Vec<String>, Vec<Module>), String>),
    ModalVersionsLoaded(Result<Vec<Module>, String>),

    // Events and logs
    DeploymentEventsLoaded(Result<(String, String, Vec<env_defs::EventData>), String>),
    JobLogsLoaded(Result<(String, Vec<env_defs::LogData>), String>),

    // Actions
    DeploymentReapplied(Result<(String, String, String), String>),
    DeploymentDestroyed(Result<String, String>),
}

/// Create a channel for background task communication
pub fn create_channel() -> (
    mpsc::UnboundedSender<BackgroundMessage>,
    mpsc::UnboundedReceiver<BackgroundMessage>,
) {
    mpsc::unbounded_channel()
}

/// Helper to spawn a background task and send result via channel
pub fn spawn_task<F, T>(
    sender: mpsc::UnboundedSender<BackgroundMessage>,
    future: F,
    mapper: impl FnOnce(Result<T, String>) -> BackgroundMessage + Send + 'static,
) where
    F: std::future::Future<Output = Result<T>> + Send + 'static,
    T: Send + 'static,
{
    tokio::spawn(async move {
        let result = future.await.map_err(|e| e.to_string());
        let _ = sender.send(mapper(result));
    });
}
