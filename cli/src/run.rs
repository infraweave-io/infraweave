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
    let mut errors: Vec<String> = Vec::new();

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
                let error_msg = format!("Failed to run a manifest in claim {}: {}", claim, e);
                eprintln!("{}", error_msg);
                errors.push(error_msg);
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
        if !errors.is_empty() {
            return Err(anyhow::anyhow!("All claims failed:\n{}", errors.join("\n")));
        }
        println!("No jobs to run");
        return Ok(());
    }

    // Warn if user wants to store files but opted out of following
    if store_files && !follow {
        eprintln!(
            "Warning: --store-files requires streaming progress (don't pass --no-follow). Files will not be stored."
        );
    }

    if follow {
        let tables = match follow_execution(&job_ids, command).await {
            Ok(tables) => tables,
            Err(e) => {
                println!("Failed to follow {}: {}", command, e);
                return Err(e);
            }
        };

        if store_files {
            if !tables.overview.is_empty() {
                std::fs::write("overview.txt", tables.overview)
                    .expect("Failed to write overview file");
                println!("Overview written to overview.txt");
            }

            if !tables.std_output.is_empty() {
                std::fs::write("std_output.txt", tables.std_output)
                    .expect("Failed to write std output file");
                println!("Std output written to std_output.txt");
            }

            if command == "plan" && !tables.violations.is_empty() {
                std::fs::write("violations.txt", tables.violations)
                    .expect("Failed to write violations file");
                println!("Violations written to violations.txt");
            }
        }
    }

    Ok(())
}
