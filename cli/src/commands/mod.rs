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

use colored::Colorize;

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
