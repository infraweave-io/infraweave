use env_common::logic::{destroy_infra, driftcheck_infra};
use env_defs::ExtraData;
use log::{error, info};

use crate::run::run_claim_file;
use crate::utils::current_region_handler;

pub async fn handle_plan(environment: &str, claim: &str, store_plan: bool, destroy: bool) {
    run_claim_file(environment, claim, "plan", store_plan, destroy)
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

pub async fn handle_apply(environment: &str, claim: &str) {
    match run_claim_file(environment, claim, "apply", false, false).await {
        Ok(_) => {
            info!("Successfully applied claim");
        }
        Err(e) => {
            error!("Failed to apply claim: {}", e);
            std::process::exit(1);
        }
    };
}

pub async fn handle_destroy(deployment_id: &str, environment: &str, version: Option<&str>) {
    match destroy_infra(
        &current_region_handler().await,
        deployment_id,
        environment,
        ExtraData::None,
        version,
    )
    .await
    {
        Ok(_) => {
            info!("Successfully requested destroying deployment");
        }
        Err(e) => {
            error!("Failed to request destroying deployment: {}", e);
            std::process::exit(1);
        }
    };
}
