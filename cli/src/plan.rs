use std::{collections::HashMap, thread, time::Duration, vec};

use anyhow::Result;
use colored::Colorize;
use env_common::{
    interface::{get_region_env_var, GenericCloudHandler},
    logic::{is_deployment_in_progress, is_deployment_plan_in_progress},
};
use env_defs::{
    pretty_print_resource_changes, pretty_print_resource_changes_with_tense, CloudProvider,
    DeploymentResp,
};
use prettytable::{row, Table};

use log::error;

use crate::ClaimJobStruct;

/// Filter out internal terraform/tofu messages from the output
fn filter_terraform_output(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let mut filtered_lines = Vec::new();
    let mut skip_remaining = false;

    for line in lines {
        // Start skipping from "Saved the plan to:" onwards
        if line.contains("Saved the plan to:") {
            skip_remaining = true;
            continue;
        }

        if !skip_remaining {
            filtered_lines.push(line);
        }
    }

    filtered_lines.join("\n").trim_end().to_string()
}

pub async fn follow_execution(
    job_ids: &Vec<ClaimJobStruct>,
    operation: &str, // "plan", "apply", or "destroy"
) -> Result<(String, String, String), anyhow::Error> {
    // Keep track of statuses in a hashmap
    let mut statuses: HashMap<String, DeploymentResp> = HashMap::new();

    // Polling loop to check job statuses periodically until all are finished
    loop {
        let mut all_finished = true;

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
                    all_finished = false;
                }

                statuses.insert(job_id.clone(), deployment.unwrap().clone());
            } else {
                let (in_progress, _job_id, status, deployment) = is_deployment_in_progress(
                    &GenericCloudHandler::region(&claim_job.region).await,
                    &claim_job.deployment_id,
                    &claim_job.environment,
                    false,
                    false,
                )
                .await;

                if in_progress {
                    println!(
                        "Status of job {}: {} ({})",
                        &claim_job.job_id,
                        if in_progress {
                            "in progress"
                        } else {
                            "completed"
                        },
                        status
                    );
                    all_finished = false;
                }

                if let Some(dep) = deployment {
                    statuses.insert(claim_job.job_id.clone(), dep);
                }
            }
        }

        if all_finished {
            break;
        }

        thread::sleep(Duration::from_secs(10));
    }

    // Check if all jobs actually succeeded (not just finished)
    let in_progress_statuses = ["requested", "initiated"];
    let mut all_successful = true;
    let mut failed_jobs = Vec::new();

    for (job_id, deployment) in &statuses {
        // Skip jobs that are still in progress
        if in_progress_statuses.contains(&deployment.status.as_str()) {
            continue;
        }

        // Any non-successful final status is a failure
        if deployment.status != "successful" {
            all_successful = false;
            failed_jobs.push((job_id.clone(), deployment.status.clone()));
        }
    }

    if all_successful {
        println!("All {} jobs are successful!", operation);
    } else {
        println!("Some {} jobs failed. Check details below.", operation);
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

            // Get change record for the operation (only if job didn't fail during init)
            if deployment.status != "failed_init" {
                let record_type = operation.to_uppercase();
                let handler = GenericCloudHandler::region(region).await;
                match handler
                    .get_change_record(environment, deployment_id, job_id, &record_type)
                    .await
                {
                    Ok(change_record) => {
                        // The output label depends on the operation type
                        let output_label = match operation {
                            "plan" => "Terraform Plan Output:",
                            "apply" => "Terraform Apply Output:",
                            "destroy" => "Terraform Destroy Output:",
                            _ => "Terraform Output:",
                        };

                        // Get full output: from S3 if truncated, otherwise from DB
                        let full_output = if !change_record.plan_std_output_key.is_empty() {
                            // Output was truncated, fetch full version from S3
                            match handler
                                .generate_presigned_url(
                                    &change_record.plan_std_output_key,
                                    "change_records",
                                )
                                .await
                            {
                                Ok(presigned_url) => match reqwest::get(&presigned_url).await {
                                    Ok(response) => match response.text().await {
                                        Ok(content) => {
                                            println!("\n(Full output retrieved from storage)");
                                            content
                                        }
                                        Err(e) => {
                                            println!("\nWarning: Could not read response from storage: {}", e);
                                            println!("Showing truncated output (last ~50KB):\n");
                                            change_record.plan_std_output.clone()
                                        }
                                    },
                                    Err(e) => {
                                        println!(
                                            "\nWarning: Could not download from storage: {}",
                                            e
                                        );
                                        println!("Showing truncated output (last ~50KB):\n");
                                        change_record.plan_std_output.clone()
                                    }
                                },
                                Err(e) => {
                                    println!("\nWarning: Could not generate download URL: {}", e);
                                    println!("Showing truncated output (last ~50KB):\n");
                                    change_record.plan_std_output.clone()
                                }
                            }
                        } else {
                            change_record.plan_std_output.clone()
                        };

                        // Filter out internal terraform/tofu messages about planfile
                        let filtered_output = filter_terraform_output(&full_output);

                        println!("\n{}\n{}", output_label, filtered_output);
                        println!("\n{}", "=".repeat(80));
                        std_output_table.add_row(row![
                            format!("{}\n({})", deployment_id, environment),
                            filtered_output
                        ]);

                        // For apply/destroy, the changes show what was planned (not what happened)
                        // The actual results are in the terraform output above
                        if operation == "plan" {
                            println!(
                                "\nChanges: \n\n{}",
                                pretty_print_resource_changes(&change_record.resource_changes)
                            );
                        } else if operation == "apply" {
                            // For apply, label and tense depend on whether it succeeded
                            let use_past_tense = deployment.status == "successful";
                            let label = if use_past_tense {
                                "Changes Applied:"
                            } else {
                                "Planned Changes (failed to apply):"
                            };
                            println!(
                                "\n{} \n\n{}",
                                label,
                                pretty_print_resource_changes_with_tense(
                                    &change_record.resource_changes,
                                    use_past_tense
                                )
                            );
                        } else {
                            // For destroy, label and tense depend on whether it succeeded
                            let use_past_tense = deployment.status == "successful";
                            let label = if use_past_tense {
                                "Resources Destroyed:"
                            } else {
                                "Planned Destroys (failed):"
                            };
                            println!(
                                "\n{} \n\n{}",
                                label,
                                pretty_print_resource_changes_with_tense(
                                    &change_record.resource_changes,
                                    use_past_tense
                                )
                            );
                        }
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

            // Display error message if the job failed (after showing the plan output)
            if deployment.status != "successful" && !deployment.error_text.is_empty() {
                println!("\nError Details:\n{}", deployment.error_text);
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

    // Return error if any jobs failed
    if !all_successful {
        let failed_summary: Vec<String> = failed_jobs
            .iter()
            .map(|(job_id, status)| format!("{} ({})", job_id, status))
            .collect();
        return Err(anyhow::anyhow!(
            "{} job(s) failed: {}. See details above.",
            operation,
            failed_summary.join(", ")
        ));
    }

    Ok((
        overview_table.to_string(),
        std_output_table.to_string(),
        violations_table.to_string(),
    ))
}
