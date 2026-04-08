use colored::Colorize;

use super::{exit_on_err, fetch_all_projects};
use crate::current_region_handler;
use env_defs::CloudProvider;
use http_client::is_http_mode_enabled;

// ── Public handlers ─────────────────────────────────────────────────────────

pub async fn handle_get_current() {
    if is_http_mode_enabled() {
        eprintln!(
            "{}",
            "Error: 'project get' is not supported in HTTP mode".red()
        );
        std::process::exit(1);
    }
    let project = exit_on_err(current_region_handler().await.get_current_project().await);
    println!(
        "Project: {}",
        serde_json::to_string_pretty(&project).unwrap()
    );
}

pub async fn handle_get_all() {
    let projects = exit_on_err(fetch_all_projects().await);

    if projects.is_empty() {
        println!("No projects found.");
    } else {
        println!("{:<20} {:<50}", "Project ID", "Name");
        println!("{}", "-".repeat(70));
        for project in projects {
            println!(
                "{:<20} {:<50}",
                project.project_id,
                if project.name.is_empty() {
                    "(no name)"
                } else {
                    &project.name
                }
            );
        }
    }
}
