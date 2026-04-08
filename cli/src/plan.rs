use std::{collections::HashMap, time::Duration, vec};

use anyhow::Result;
use env_common::{
    interface::{get_region_env_var, GenericCloudHandler},
    logic::{is_deployment_in_progress, is_deployment_plan_in_progress, PROJECT_ID},
};
use env_defs::{pretty_print_resource_changes, CloudProvider, DeploymentResp, DeploymentStatus, InfraChangeRecord};
use http_client::{
    http_check_deployment_progress as http_check_progress, http_get_change_record,
    http_is_deployment_plan_in_progress as http_is_plan_in_progress, is_http_mode_enabled,
};
use prettytable::{row, Table};

use log::error;

use crate::ClaimJobStruct;

/// Return the PROJECT_ID or bail with a clear error.
fn require_project_id() -> Result<&'static str> {
    PROJECT_ID
        .get()
        .map(|s| s.as_str())
        .ok_or_else(|| anyhow::anyhow!("PROJECT_ID is not set – pass --project in HTTP mode"))
}

pub async fn follow_execution(
    job_ids: &Vec<ClaimJobStruct>,
    operation: &str, // "plan", "apply", or "destroy"
) -> Result<(String, String, String), anyhow::Error> {
    use colored::Colorize;

    let http_mode = is_http_mode_enabled();

    // Keep track of statuses in a hashmap
    let mut statuses: HashMap<String, DeploymentResp> = HashMap::new();
    let mut last_status: HashMap<String, String> = HashMap::new();

    // Polling loop to check job statuses periodically until all are finished
    let mut failure_errors: Vec<String> = Vec::new();

    loop {
        let mut all_finished = true;
        let mut any_failed = false;

        for claim_job in job_ids {
            if operation == "plan" {
                let (in_progress, job_id, deployment) = if http_mode {
                    let project_id = require_project_id()?;
                    http_is_plan_in_progress(
                        project_id,
                        &claim_job.region,
                        &claim_job.deployment_id,
                        &claim_job.environment,
                        &claim_job.job_id,
                    )
                    .await
                } else {
                    is_deployment_plan_in_progress(
                        &GenericCloudHandler::region(&claim_job.region).await,
                        &claim_job.deployment_id,
                        &claim_job.environment,
                        &claim_job.job_id,
                    )
                    .await
                };

                // Extract short job ID for display (last part of ARN)
                let short_job_id = job_id.split('/').last().unwrap_or(&job_id);

                if in_progress {
                    // Only print status if it changed
                    let status_key = format!("{}:in_progress", job_id);
                    if last_status.get(&job_id) != Some(&status_key) {
                        println!(
                            "{} Task {} is {}...",
                            "⏳".cyan(),
                            short_job_id.cyan(),
                            "running".cyan().bold()
                        );
                        last_status.insert(job_id.clone(), status_key);
                    }
                    all_finished = false;
                } else {
                    // Task completed - check if it succeeded or failed
                    if let Some(dep) = &deployment {
                        // Status can be "successful", "completed", "success"
                        let status_msg = if dep.status == "completed"
                            || dep.status == "success"
                            || dep.status == "successful"
                        {
                            format!(
                                "{} Task {} {}",
                                "✓".green(),
                                short_job_id.green(),
                                "completed successfully".green().bold()
                            )
                        } else {
                            any_failed = true;
                            let mut msg = format!(
                                "{} Task {} {}",
                                "✗".red(),
                                short_job_id.red(),
                                "failed".red().bold()
                            );
                            if !dep.error_text.is_empty() {
                                msg.push_str(&format!(
                                    "\n   {}: {}",
                                    "Error".red().bold(),
                                    dep.error_text.red()
                                ));
                                // Store error for final summary
                                if !failure_errors.iter().any(|e| e == &dep.error_text) {
                                    failure_errors.push(dep.error_text.clone());
                                }
                            }
                            msg
                        };

                        let status_key = format!("{}:done", job_id);
                        if last_status.get(&job_id) != Some(&status_key) {
                            println!("{}", status_msg);
                            last_status.insert(job_id.clone(), status_key);
                        }
                    } else {
                        // No deployment record and task not in progress = failed
                        any_failed = true;
                        let status_key = format!("{}:failed", job_id);
                        if last_status.get(&job_id) != Some(&status_key) {
                            println!(
                                "{} Task {} {}",
                                "✗".red(),
                                short_job_id.red(),
                                "failed".red().bold()
                            );
                            last_status.insert(job_id.clone(), status_key);
                        }
                    }
                }

                if let Some(dep) = deployment {
                    statuses.insert(job_id.clone(), dep);
                }
            } else {
                // Apply/Destroy: Use polling logic similar to Plan, checking ECS task status
                let (in_progress, validated_job_id, deployment) = if http_mode {
                    let project_id = require_project_id()?;
                    http_check_progress(
                        project_id,
                        &claim_job.region,
                        &claim_job.deployment_id,
                        &claim_job.environment,
                        &claim_job.job_id,
                    )
                    .await
                } else {
                    let (in_progress, _, _, deployment) = is_deployment_in_progress(
                        &GenericCloudHandler::region(&claim_job.region).await,
                        &claim_job.deployment_id,
                        &claim_job.environment,
                        false,
                        false,
                    )
                    .await;
                    (in_progress, claim_job.job_id.clone(), deployment)
                };

                // Extract short job ID for display
                let short_job_id = validated_job_id
                    .split('/')
                    .last()
                    .unwrap_or(&validated_job_id);

                if in_progress {
                    let status_key = format!("{}:in_progress", validated_job_id);
                    if last_status.get(&validated_job_id) != Some(&status_key) {
                        println!(
                            "{} Task {} is {}...",
                            "⏳".cyan(),
                            short_job_id.cyan(),
                            "running".cyan().bold()
                        );
                        last_status.insert(validated_job_id.clone(), status_key);
                    }
                    all_finished = false;
                } else {
                    // Task completed - check if it succeeded or failed
                    if let Some(dep) = &deployment {
                        let status_msg = if dep.status == "completed"
                            || dep.status == "success"
                            || dep.status == "successful"
                        {
                            format!(
                                "{} Task {} {}",
                                "✓".green(),
                                short_job_id.green(),
                                "completed successfully".green().bold()
                            )
                        } else {
                            any_failed = true;
                            let mut msg = format!(
                                "{} Task {} {}",
                                "✗".red(),
                                short_job_id.red(),
                                "failed".red().bold()
                            );
                            if !dep.error_text.is_empty() {
                                msg.push_str(&format!(
                                    "\n   {}: {}",
                                    "Error".red().bold(),
                                    dep.error_text.red()
                                ));
                                // Store error for final summary
                                if !failure_errors.iter().any(|e| e == &dep.error_text) {
                                    failure_errors.push(dep.error_text.clone());
                                }
                            }
                            msg
                        };

                        let status_key = format!("{}:done", validated_job_id);
                        if last_status.get(&validated_job_id) != Some(&status_key) {
                            println!("{}", status_msg);
                            last_status.insert(validated_job_id.clone(), status_key);
                        }
                    } else {
                        // Failed but no deployment record updated?
                        any_failed = true;
                        let status_key = format!("{}:failed", validated_job_id);
                        if last_status.get(&validated_job_id) != Some(&status_key) {
                            println!(
                                "{} Task {} {}",
                                "✗".red(),
                                short_job_id.red(),
                                "failed".red().bold()
                            );
                            last_status.insert(validated_job_id.clone(), status_key);
                        }
                    }
                }

                if let Some(dep) = deployment {
                    // Use job_id (short) as key
                    statuses.insert(validated_job_id.clone(), dep);
                }
            }
        }

        if all_finished {
            if any_failed {
                println!("\n{} Some {} jobs failed!", "✗".red(), operation);
                if !failure_errors.is_empty() {
                    println!("\n{}", "Failure reasons:".red().bold());
                    for (i, error) in failure_errors.iter().enumerate() {
                        println!("  {}. {}", i + 1, error.red());
                    }
                }
                return Err(anyhow::anyhow!("One or more jobs failed"));
            } else {
                println!(
                    "\n{} All {} jobs completed successfully!",
                    "✓".green(),
                    operation
                );
            }
            break;
        }

        tokio::time::sleep(Duration::from_secs(10)).await;
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

        // Normalize to short job ID to match the keys stored in statuses map
        let job_id_short = job_id.split('/').last().unwrap_or(job_id);

        if let Some(deployment) = statuses
            .get(job_id_short)
            .or_else(|| statuses.get(job_id.as_str()))
        {
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

            // Get change record for the operation (only if job didn't fail during init)
            if deployment.status != DeploymentStatus::FailedInit {
                let record_type = operation.to_uppercase();
                log::debug!(
                    "Fetching change record for job {} in region {} (type: {})",
                    job_id,
                    region,
                    record_type
                );
                let change_result = if http_mode {
                    let project_id = require_project_id()?;
                    match http_get_change_record(
                        project_id,
                        region,
                        environment,
                        deployment_id,
                        job_id,
                        &record_type,
                    )
                    .await
                    {
                        Ok(value) => serde_json::from_value::<InfraChangeRecord>(value)
                            .map_err(|e| anyhow::anyhow!(e)),
                        Err(e) => Err(anyhow::anyhow!(e)),
                    }
                } else {
                    GenericCloudHandler::region(region)
                        .await
                        .get_change_record(environment, deployment_id, job_id, &record_type)
                        .await
                };
                match change_result {
                    Ok(change_record) => {
                        println!("\nOutput:\n{}", change_record.plan_std_output);
                        std_output_table.add_row(row![
                            format!("{}\n({})", deployment_id, environment),
                            change_record.plan_std_output
                        ]);
                        println!(
                            "Changes: \n{}",
                            pretty_print_resource_changes(&change_record.resource_changes)
                        );
                    }
                    Err(e) => {
                        error!("Failed to get change record: {}", e);
                    }
                }
            } else {
                println!("\nJob failed during initialization. Check job logs for details:");
                println!(
                    "  {}={} infraweave get-logs {}",
                    get_region_env_var(),
                    region,
                    job_id
                );
            }

            // Display policy violations for all operations
            if deployment.status == DeploymentStatus::FailedPolicy {
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
