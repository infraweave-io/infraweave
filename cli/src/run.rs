use anyhow::Result;
use env_common::logic::run_claim;
use env_defs::ExtraData;
use serde::Deserialize;
use std::vec;

use crate::{follow_plan, handler};

pub async fn run_claim_file(
    environment: &str,
    claim: &str,
    command: &str,
    store_plan: bool,
) -> Result<(), anyhow::Error> {
    // Read claim yaml file:
    let file_content = std::fs::read_to_string(claim).expect("Failed to read claim file");

    // Parse multiple YAML documents
    let claims: Vec<serde_yaml::Value> = serde_yaml::Deserializer::from_str(&file_content)
        .map(|doc| serde_yaml::Value::deserialize(doc).unwrap_or("".into()))
        .collect();

    // job_id, deployment_id, environment
    let mut job_ids: Vec<(String, String, String)> = Vec::new();

    log::info!("Applying {} claims in file", claims.len());
    for yaml in claims.iter() {
        let flags = vec![];
        let (job_id, deployment_id) = match run_claim(
            &handler().await,
            yaml,
            environment,
            command,
            flags,
            ExtraData::None,
        )
        .await
        {
            Ok((job_id, deployment_id)) => (job_id, deployment_id),
            Err(e) => {
                println!("Failed to run a manifest in claim {}: {}", claim, e);
                continue;
            }
        };
        job_ids.push((job_id, deployment_id, environment.to_string()));
    }

    for (job_id, deployment_id, environment) in &job_ids {
        println!(
            "Started {} job: {} in {} (job id: {})",
            command, deployment_id, environment, job_id
        );
    }

    if job_ids.is_empty() {
        println!("No jobs to run");
        return Ok(());
    }

    if command == "plan" {
        let (overview, std_output, violations) = match follow_plan(&job_ids).await {
            Ok((overview, std_output, violations)) => (overview, std_output, violations),
            Err(e) => {
                println!("Failed to follow plan: {}", e);
                return Err(e);
            }
        };
        if store_plan {
            std::fs::write("overview.txt", overview).expect("Failed to write plan overview file");
            println!("Plan overview written to overview.txt");

            std::fs::write("std_output.txt", std_output)
                .expect("Failed to write plan std output file");
            println!("Plan std output written to std_output.txt");

            std::fs::write("violations.txt", violations)
                .expect("Failed to write plan violations file");
            println!("Plan violations written to violations.txt");
        }
    }

    Ok(())
}
