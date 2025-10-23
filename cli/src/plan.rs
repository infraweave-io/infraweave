use std::{collections::HashMap, thread, time::Duration, vec};

use anyhow::Result;
use colored::Colorize;
use env_common::{
    interface::GenericCloudHandler,
    logic::{is_deployment_in_progress, is_deployment_plan_in_progress},
};
use env_defs::{CloudProvider, DeploymentResp};
use prettytable::{row, Table};

use log::error;

use crate::ClaimJobStruct;

pub async fn follow_execution(
    job_ids: &Vec<ClaimJobStruct>,
    operation: &str, // "plan", "apply", or "destroy"
) -> Result<(String, String, String), anyhow::Error> {
    // Keep track of statuses in a hashmap
    let mut statuses: HashMap<String, DeploymentResp> = HashMap::new();

    // Polling loop to check job statuses periodically until all are finished
    loop {
        let mut all_successful = true;

        for claim_job in job_ids {
            if operation == "plan" {
                let (in_progress, job_id, deployment) = is_deployment_plan_in_progress(
                    &GenericCloudHandler::region(&claim_job.region).await,
                    &claim_job.deployment_id,
                    &claim_job.environment,
                    &claim_job.job_id,
                )
                .await;

                if in_progress {
                    println!(
                        "Status of job {}: {}",
                        job_id,
                        if in_progress {
                            "in progress"
                        } else {
                            "completed"
                        }
                    );
                    all_successful = false;
                }

                statuses.insert(job_id.clone(), deployment.unwrap().clone());
            } else {
                let (in_progress, job_id, status, deployment) = is_deployment_in_progress(
                    &GenericCloudHandler::region(&claim_job.region).await,
                    &claim_job.deployment_id,
                    &claim_job.environment,
                )
                .await;

                if in_progress {
                    println!(
                        "Status of job {}: {} ({})",
                        job_id,
                        if in_progress {
                            "in progress"
                        } else {
                            "completed"
                        },
                        status
                    );
                    all_successful = false;
                }

                if let Some(dep) = deployment {
                    statuses.insert(job_id.clone(), dep);
                }
            }
        }

        if all_successful {
            println!("All {} jobs are successful!", operation);
            break;
        }

        thread::sleep(Duration::from_secs(10));
    }

    // Build table strings for store_files feature (for plan) and backward compatibility
    let mut overview_table = Table::new();
    overview_table.add_row(row![
        "Deployment id\n(Environment)".purple().bold(),
        "Status".blue().bold(),
        "Job id".green().bold(),
        "Description".red().bold(),
    ]);

    let mut std_output_table = Table::new();
    std_output_table.add_row(row![
        "Deployment id\n(Environment)".purple().bold(),
        "Std output".blue().bold()
    ]);

    let mut violations_table = Table::new();
    violations_table.add_row(row![
        "Deployment id\n(Environment)".purple().bold(),
        "Policy".blue().bold(),
        "Violations".red().bold()
    ]);

    // Print results for each job
    for claim_job in job_ids {
        let deployment_id = &claim_job.deployment_id;
        let environment = &claim_job.environment;
        let job_id = &claim_job.job_id;
        let region = &claim_job.region;

        if let Some(deployment) = statuses.get(job_id) {
            println!("\n{}", "=".repeat(80));
            println!(
                "Deployment: {} (Environment: {})",
                deployment_id, environment
            );
            println!("Job ID: {}", deployment.job_id);
            println!("Status: {}", deployment.status);

            let violation_count = deployment
                .policy_results
                .iter()
                .filter(|p| p.failed)
                .count();
            println!("Policy Violations: {}", violation_count);

            overview_table.add_row(row![
                format!("{}\n({})", deployment_id, environment),
                deployment.status,
                deployment.job_id,
                format!("{} policy violations", violation_count)
            ]);

            println!("{}", "=".repeat(80));

            // Get change record for the operation
            let record_type = operation.to_uppercase();
            match GenericCloudHandler::region(region)
                .await
                .get_change_record(environment, deployment_id, job_id, &record_type)
                .await
            {
                Ok(change_record) => {
                    println!("\nOutput:\n{}", change_record.plan_std_output);
                    std_output_table.add_row(row![
                        format!("{}\n({})", deployment_id, environment),
                        change_record.plan_std_output
                    ]);
                }
                Err(e) => {
                    error!("Failed to get change record: {}", e);
                }
            }

            // Display policy violations for all operations
            if deployment.status == "failed_policy" {
                println!("\nPolicy Validation Failed:");
                for result in deployment.policy_results.iter().filter(|p| p.failed) {
                    println!("  Policy: {}", result.policy);
                    println!(
                        "  Violations: {}",
                        serde_json::to_string_pretty(&result.violations).unwrap()
                    );
                    violations_table.add_row(row![
                        format!("{}\n({})", deployment_id, environment),
                        result.policy,
                        serde_json::to_string_pretty(&result.violations).unwrap()
                    ]);
                }
            } else if !deployment.policy_results.is_empty() {
                println!("\nPolicy Validation: Passed");
            }
        }
    }

    Ok((
        overview_table.to_string(),
        std_output_table.to_string(),
        violations_table.to_string(),
    ))
}
