use env_common::logic::{destroy_infra, driftcheck_infra};
use env_defs::{CloudProvider, ExtraData};
use log::{error, info};

use crate::run::run_claim_file;
use crate::utils::current_region_handler;
use crate::{follow_execution, ClaimJobStruct};

pub async fn handle_plan(
    environment: &str,
    claim: &str,
    store_files: bool,
    destroy: bool,
    follow: bool,
) {
    if !follow {
        eprintln!("Error: Plan operations require --follow flag to be enabled.");
        eprintln!("Usage: infraweave plan {} {} --follow", environment, claim);
        std::process::exit(1);
    }

    run_claim_file(environment, claim, "plan", store_files, destroy, follow)
        .await
        .unwrap();
}

pub async fn handle_driftcheck(deployment_id: &str, environment: &str, remediate: bool) {
    match driftcheck_infra(
        &current_region_handler().await,
        deployment_id,
        environment,
        remediate,
        ExtraData::None,
    )
    .await
    {
        Ok(_) => {
            info!("Successfully requested drift check");
        }
        Err(e) => {
            error!("Failed to request drift check: {}", e);
            std::process::exit(1);
        }
    };
}

pub async fn handle_apply(environment: &str, claim: &str, store_files: bool, follow: bool) {
    match run_claim_file(environment, claim, "apply", store_files, false, follow).await {
        Ok(_) => {
            info!("Successfully applied claim");
        }
        Err(e) => {
            error!("Failed to apply claim: {}", e);
            std::process::exit(1);
        }
    };
}

pub async fn handle_destroy(
    deployment_id: &str,
    environment: &str,
    version: Option<&str>,
    store_files: bool,
    follow: bool,
) {
    let region_handler = current_region_handler().await;

    // Warn if user wants to store files but didn't enable following
    if store_files && !follow {
        eprintln!(
            "Warning: --store-files requires --follow to be enabled. Files will not be stored."
        );
        eprintln!("Add --follow to enable file storage.");
    }

    let job_id = match destroy_infra(
        &region_handler,
        deployment_id,
        environment,
        ExtraData::None,
        version,
    )
    .await
    {
        Ok(job_id) => {
            info!("Successfully requested destroying deployment");
            job_id
        }
        Err(e) => {
            error!("Failed to request destroying deployment: {}", e);
            std::process::exit(1);
        }
    };

    if follow {
        // Get region from the handler
        let region = region_handler.get_region();

        let job_struct = ClaimJobStruct {
            job_id,
            deployment_id: deployment_id.to_string(),
            environment: environment.to_string(),
            region: region.to_string(),
        };

        match follow_execution(&vec![job_struct], "destroy").await {
            Ok((overview, std_output, _violations)) => {
                info!("Successfully followed destroy operation");

                if store_files {
                    std::fs::write("overview.txt", overview)
                        .expect("Failed to write overview file");
                    println!("Overview written to overview.txt");

                    std::fs::write("std_output.txt", std_output)
                        .expect("Failed to write std output file");
                    println!("Std output written to std_output.txt");
                }
            }
            Err(e) => {
                error!("Failed to follow destroy operation: {}", e);
                std::process::exit(1);
            }
        }
    }
}
