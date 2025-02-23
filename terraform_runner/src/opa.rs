use env_common::interface::GenericCloudHandler;
use env_defs::CloudProvider;
use std::path::Path;

use crate::cmd::{run_generic_command, CommandResult};

pub async fn run_opa_command(
    max_output_lines: usize,
    policy_name: &str,
    rego_files: &Vec<String>,
) -> Result<CommandResult, anyhow::Error> {
    println!("Running opa eval on policy {}", policy_name);

    let mut exec = tokio::process::Command::new("opa");
    exec.arg("eval").arg("--format").arg("pretty");

    for rego_file in rego_files {
        println!("Adding arg to opa command --data {}", rego_file);
        exec.arg("--data");
        exec.arg(rego_file);
    }

    exec.arg("--input")
        .arg("./tf_plan.json")
        .arg("--data")
        .arg("./env_data.json")
        .arg("--data")
        .arg("./policy_input.json")
        .arg("data.infraweave")
        .current_dir(Path::new("./"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped()); // Capture stdout

    println!("Running opa command...");
    // Print command
    println!("{:?}", exec);

    run_generic_command(&mut exec, max_output_lines).await
}

pub async fn download_policy(policy: &env_defs::PolicyResp) {
    println!("Downloading policy for {}...", policy.policy);

    let handler = GenericCloudHandler::default().await;
    let url = match handler.get_policy_download_url(&policy.s3_key).await {
        Ok(url) => url,
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    };

    match env_utils::download_zip(&url, Path::new("policy.zip")).await {
        Ok(_) => {
            println!("Downloaded policy successfully");
        }
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    }

    let metadata = std::fs::metadata("policy.zip").unwrap();
    println!("Size of policy.zip: {:?} bytes", metadata.len());

    match env_utils::unzip_file(Path::new("policy.zip"), Path::new("./")) {
        Ok(_) => {
            println!("Unzipped policy successfully");
        }
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    }
}

pub fn get_all_rego_filenames_in_cwd() -> Vec<String> {
    let rego_files: Vec<String> = std::fs::read_dir("./")
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .map(|s| s.ends_with(".rego"))
                .unwrap_or(false)
        })
        .map(|entry| entry.path().to_str().unwrap().to_string())
        .collect();
    rego_files
}
