pub mod admin;
pub mod auth;
pub mod claim;
pub mod deployment;
pub mod gitops;
pub mod mcp;
pub mod module;
pub mod policy;
pub mod project;
pub mod provider;
pub mod stack;
pub mod upgrade;

use anyhow::Result;
use colored::Colorize;
use env_defs::CloudProvider;
use http_client::{http_get_all_projects, is_http_mode_enabled};

use crate::current_region_handler;

// ── Shared transport-agnostic helpers ───────────────────────────────────────

pub async fn fetch_all_projects() -> Result<Vec<env_defs::ProjectData>> {
    if is_http_mode_enabled() {
        http_get_all_projects()
            .await?
            .into_iter()
            .map(|v| serde_json::from_value(v).map_err(Into::into))
            .collect()
    } else {
        Ok(current_region_handler().await.get_all_projects().await?)
    }
}

pub fn exit_on_err<T>(result: anyhow::Result<T>) -> T {
    match result {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}", format!("Error: {}", e).red());
            std::process::exit(1);
        }
    }
}

pub fn exit_on_none<T>(option: Option<T>, message: &str) -> T {
    match option {
        Some(v) => v,
        None => {
            eprintln!("{}", format!("Error: {}", message).red());
            std::process::exit(1);
        }
    }
}
