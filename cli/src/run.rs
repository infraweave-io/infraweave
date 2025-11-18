use anyhow::Result;
use env_common::{interface::GenericCloudHandler, logic::run_claim};
use env_defs::{DeploymentManifest, ExtraData};
use serde::Deserialize;
use std::vec;

use crate::{follow_execution, ClaimJobStruct};

pub async fn run_claim_file(
    environment: &str,
    claim: &str,
    command: &str,
    store_files: bool,
    destroy: bool,
    follow: bool,
) -> Result<(), anyhow::Error> {
    // Read claim yaml file:
    let file_content = std::fs::read_to_string(claim).expect("Failed to read claim file");

    // Parse multiple YAML documents
    let claims: Vec<serde_yaml::Value> = serde_yaml::Deserializer::from_str(&file_content)
        .map(|doc| serde_yaml::Value::deserialize(doc).unwrap_or("".into()))
        .collect();

    // job_id, deployment_id, environment
    let mut job_ids: Vec<ClaimJobStruct> = Vec::new();

    let reference_fallback: String = match hostname::get() {
        Ok(hostname) => hostname.to_string_lossy().to_string(),
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to get hostname: {}", e));
        }
    };

    log::info!("Applying {} claims in file", claims.len());
    for yaml in claims.iter() {
        let flags = if destroy {
            vec!["-destroy".to_string()]
        } else {
            vec![]
        };
        let deployment_manifest: DeploymentManifest = serde_yaml::from_value(yaml.clone())?;
        let region = &deployment_manifest.spec.region;
        let (job_id, deployment_id) = match run_claim(
            &GenericCloudHandler::region(region).await,
            yaml,
            environment,
            command,
            flags,
            ExtraData::None,
            &reference_fallback,
        )
        .await
        {
            Ok((job_id, deployment_id, _)) => (job_id, deployment_id),
            Err(e) => {
                println!("Failed to run a manifest in claim {}: {}", claim, e);
                continue;
            }
        };
        job_ids.push(ClaimJobStruct {
            job_id,
            deployment_id,
            environment: environment.to_string(),
            region: region.to_string(),
        });
    }

    for claim_job in &job_ids {
        println!(
            "Started {} job: {} in {} (job id: {})",
            command, claim_job.deployment_id, claim_job.environment, claim_job.job_id
        );
    }

    if job_ids.is_empty() {
        println!("No jobs to run");
        return Ok(());
    }

    // Warn if user wants to store files but disabled following
    if store_files && !follow {
        eprintln!(
            "Warning: --store-files requires --follow to be enabled. Files will not be stored."
        );
        eprintln!("Add --follow to enable file storage.");
    }

    if follow {
        let (overview, std_output, violations) = match follow_execution(&job_ids, command).await {
            Ok((overview, std_output, violations)) => (overview, std_output, violations),
            Err(e) => {
                println!("Failed to follow {}: {}", command, e);
                return Err(e);
            }
        };

        if store_files {
            std::fs::write("overview.txt", overview).expect("Failed to write overview file");
            println!("Overview written to overview.txt");

            std::fs::write("std_output.txt", std_output).expect("Failed to write std output file");
            println!("Std output written to std_output.txt");

            if command == "plan" {
                std::fs::write("violations.txt", violations)
                    .expect("Failed to write violations file");
                println!("Violations written to violations.txt");
            }
        }
    }

    Ok(())
}
