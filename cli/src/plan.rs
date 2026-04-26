use std::{collections::HashMap, time::Duration};

use anyhow::Result;
use colored::Colorize;
use env_common::{
    interface::{get_region_env_var, GenericCloudHandler},
    logic::{is_deployment_in_progress, is_deployment_plan_in_progress, PROJECT_ID},
};
use env_defs::{
    pretty_print_resource_changes, CloudProvider, DeploymentResp, DeploymentStatus,
    InfraChangeRecord,
};
use http_client::{
    http_check_deployment_progress as http_check_progress, http_get_change_record,
    http_is_deployment_plan_in_progress as http_is_plan_in_progress, is_http_mode_enabled,
};
use log::{debug, error};
use prettytable::{row, Table};

use crate::ClaimJobStruct;

fn require_project_id() -> Result<&'static str> {
    PROJECT_ID
        .get()
        .map(|s| s.as_str())
        .ok_or_else(|| anyhow::anyhow!("PROJECT_ID is not set - pass --project in HTTP mode"))
}

fn short_id(job_id: &str) -> &str {
    job_id.split('/').last().unwrap_or(job_id)
}

const POLL_INTERVAL: Duration = Duration::from_secs(10);

pub struct SummaryTables {
    pub overview: String,
    pub std_output: String,
    pub violations: String,
}

async fn fetch_progress(
    operation: &str,
    http_mode: bool,
    cj: &ClaimJobStruct,
) -> Result<(bool, Option<DeploymentResp>)> {
    let is_plan = operation == "plan";
    if http_mode {
        let project_id = require_project_id()?;
        let ClaimJobStruct {
            region,
            deployment_id,
            environment,
            job_id,
        } = cj;
        let (in_progress, _, deployment) = if is_plan {
            http_is_plan_in_progress(project_id, region, deployment_id, environment, job_id).await
        } else {
            http_check_progress(project_id, region, deployment_id, environment, job_id).await
        };
        return Ok((in_progress, deployment));
    }
    let handler = GenericCloudHandler::region(&cj.region).await;
    if is_plan {
        let (in_progress, _, deployment) = is_deployment_plan_in_progress(
            &handler,
            &cj.deployment_id,
            &cj.environment,
            &cj.job_id,
        )
        .await;
        return Ok((in_progress, deployment));
    }
    let (in_progress, _, _, deployment) =
        is_deployment_in_progress(&handler, &cj.deployment_id, &cj.environment, false, false).await;
    Ok((in_progress, deployment))
}

async fn fetch_change_record(
    http_mode: bool,
    region: &str,
    environment: &str,
    deployment_id: &str,
    job_id: &str,
    record_type: &str,
) -> Result<InfraChangeRecord> {
    if http_mode {
        let project_id = require_project_id()?;
        let value = http_get_change_record(
            project_id,
            region,
            environment,
            deployment_id,
            job_id,
            record_type,
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
        serde_json::from_value::<InfraChangeRecord>(value).map_err(|e| anyhow::anyhow!(e))
    } else {
        GenericCloudHandler::region(region)
            .await
            .get_change_record(environment, deployment_id, job_id, record_type)
            .await
    }
}

/// Prints the status line (only on transitions) and returns whether the job failed.
///
/// When `quiet` is true, progress prints for in-progress and successful jobs are
/// suppressed - the caller is expected to render its own summary. Failures are
/// still surfaced so errors aren't hidden.
fn report_job(
    in_progress: bool,
    job_id: &str,
    deployment: Option<&DeploymentResp>,
    last_status: &mut HashMap<String, DeploymentStatus>,
    failure_errors: &mut Vec<String>,
    quiet: bool,
) -> bool {
    let short = short_id(job_id);

    // Synthesize a status for the observation. When no deployment is returned we
    // fall back to a sentinel: Initiated (non-final) if still in progress,
    // otherwise Failed (final). These are only used for `is_final()` branching
    // and dedup - never shown to the user.
    let observed = deployment
        .map(|d| d.status.clone())
        .unwrap_or(if in_progress {
            DeploymentStatus::Initiated
        } else {
            DeploymentStatus::Failed
        });

    if !observed.is_final() {
        if !last_status.contains_key(job_id) {
            if !quiet {
                println!("Job {} is {}...", short.cyan(), "running".cyan().bold());
            }
            last_status.insert(job_id.to_string(), observed);
        }
        return false;
    }

    let (failed, message) = match deployment {
        Some(dep) if dep.status == DeploymentStatus::Successful => (
            false,
            format!(
                "Job {} {}",
                short.green(),
                "completed successfully".green().bold()
            ),
        ),
        Some(dep) => {
            let mut msg = format!("Job {} {}", short.red(), "failed".red().bold());
            if !dep.error_text.is_empty() {
                msg.push_str(&format!(
                    "\n   {}: {}",
                    "Error".red().bold(),
                    dep.error_text.red()
                ));
                if !failure_errors.iter().any(|e| e == &dep.error_text) {
                    failure_errors.push(dep.error_text.clone());
                }
            }
            (true, msg)
        }
        None => (
            true,
            format!("Job {} {}", short.red(), "failed".red().bold()),
        ),
    };

    let already_final = last_status
        .get(job_id)
        .is_some_and(DeploymentStatus::is_final);
    if !already_final {
        if failed || !quiet {
            println!("{}", message);
        }
        last_status.insert(job_id.to_string(), observed);
    }
    failed
}

async fn poll_until_done(
    job_ids: &[ClaimJobStruct],
    operation: &str,
    http_mode: bool,
    quiet: bool,
) -> Result<HashMap<String, DeploymentResp>> {
    let mut statuses: HashMap<String, DeploymentResp> = HashMap::new();
    let mut last_status: HashMap<String, DeploymentStatus> = HashMap::new();
    let mut failure_errors: Vec<String> = Vec::new();

    loop {
        let mut all_finished = true;
        let mut any_failed = false;

        for cj in job_ids {
            let (in_progress, deployment) = fetch_progress(operation, http_mode, cj).await?;
            if report_job(
                in_progress,
                &cj.job_id,
                deployment.as_ref(),
                &mut last_status,
                &mut failure_errors,
                quiet,
            ) {
                any_failed = true;
            }
            if in_progress {
                all_finished = false;
            }
            if let Some(dep) = deployment {
                statuses.insert(cj.job_id.clone(), dep);
            }
        }

        if all_finished {
            if any_failed {
                println!(
                    "\n{}",
                    format!("Some {} jobs failed!", operation).red().bold()
                );
                if !failure_errors.is_empty() {
                    println!("\n{}", "Failure reasons:".red().bold());
                    for (i, error) in failure_errors.iter().enumerate() {
                        println!("  {}. {}", i + 1, error.red());
                    }
                }
                return Err(anyhow::anyhow!("One or more jobs failed"));
            }
            if !quiet {
                println!(
                    "\n{}",
                    format!("All {} jobs completed successfully!", operation)
                        .green()
                        .bold()
                );
            }
            return Ok(statuses);
        }

        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn render_summary(
    job_ids: &[ClaimJobStruct],
    operation: &str,
    http_mode: bool,
    statuses: &HashMap<String, DeploymentResp>,
) -> SummaryTables {
    let mut overview = Table::new();
    overview.add_row(row![
        "Deployment id\n(Environment)".purple().bold(),
        "Status".blue().bold(),
        "Job id".green().bold(),
        "Description".red().bold(),
    ]);
    let mut overview_has_rows = false;

    let mut std_output = Table::new();
    std_output.add_row(row![
        "Deployment id\n(Environment)".purple().bold(),
        "Std output".blue().bold()
    ]);
    let mut std_output_has_rows = false;

    let mut violations = Table::new();
    violations.add_row(row![
        "Deployment id\n(Environment)".purple().bold(),
        "Policy".blue().bold(),
        "Violations".red().bold()
    ]);
    let mut violations_has_rows = false;

    for cj in job_ids {
        let Some(deployment) = statuses.get(&cj.job_id) else {
            continue;
        };
        let (deployment_id, environment, job_id, region) =
            (&cj.deployment_id, &cj.environment, &cj.job_id, &cj.region);

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

        overview.add_row(row![
            format!("{}\n({})", deployment_id, environment),
            deployment.status,
            deployment.job_id,
            format!("{} policy violations", violation_count)
        ]);
        overview_has_rows = true;

        println!("{}", "=".repeat(80));

        if deployment.status != DeploymentStatus::FailedInit {
            let record_type = operation.to_uppercase();
            debug!(
                "Fetching change record for job {} in region {} (type: {})",
                job_id, region, record_type
            );
            match fetch_change_record(
                http_mode,
                region,
                environment,
                deployment_id,
                job_id,
                &record_type,
            )
            .await
            {
                Ok(change_record) => {
                    println!("\nOutput:\n{}", change_record.plan_std_output);
                    std_output.add_row(row![
                        format!("{}\n({})", deployment_id, environment),
                        change_record.plan_std_output
                    ]);
                    std_output_has_rows = true;
                    println!(
                        "Changes: \n{}",
                        pretty_print_resource_changes(&change_record.resource_changes)
                    );
                }
                Err(e) => error!("Failed to get change record: {}", e),
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

        if deployment.status == DeploymentStatus::FailedPolicy {
            println!("\nPolicy Validation Failed:");
            for result in deployment.policy_results.iter().filter(|p| p.failed) {
                println!("  Policy: {}", result.policy);
                println!(
                    "  Violations: {}",
                    serde_json::to_string_pretty(&result.violations).unwrap()
                );
                violations.add_row(row![
                    format!("{}\n({})", deployment_id, environment),
                    result.policy,
                    serde_json::to_string_pretty(&result.violations).unwrap()
                ]);
                violations_has_rows = true;
            }
        } else if !deployment.policy_results.is_empty() {
            println!("\nPolicy Validation: Passed");
        }
    }

    let render = |table: Table, has_rows: bool| {
        if has_rows {
            table.to_string()
        } else {
            String::new()
        }
    };
    SummaryTables {
        overview: render(overview, overview_has_rows),
        std_output: render(std_output, std_output_has_rows),
        violations: render(violations, violations_has_rows),
    }
}

pub async fn follow_execution(
    job_ids: &[ClaimJobStruct],
    operation: &str, // "plan", "apply", or "destroy"
) -> Result<SummaryTables> {
    let http_mode = is_http_mode_enabled();
    let statuses = poll_until_done(job_ids, operation, http_mode, false).await?;
    Ok(render_summary(job_ids, operation, http_mode, &statuses).await)
}

pub struct DriftOutcome {
    pub deployment_status: DeploymentStatus,
    pub resource_changes: Vec<env_defs::SanitizedResourceChange>,
}

/// Follow a driftcheck job to completion and return its compact change record.
///
/// Driftcheck runs a `plan -refresh-only` (or `apply` when remediating) under
/// the hood, but callers typically want the compact sanitized diff rather than
/// the full plan summary tables produced by `follow_execution`.
pub async fn follow_driftcheck(job: &ClaimJobStruct, remediate: bool) -> Result<DriftOutcome> {
    let operation = if remediate { "apply" } else { "plan" };
    let http_mode = is_http_mode_enabled();
    let statuses = poll_until_done(std::slice::from_ref(job), operation, http_mode, true).await?;
    let deployment = statuses
        .get(&job.job_id)
        .ok_or_else(|| anyhow::anyhow!("No deployment record returned for drift check"))?;

    let change_record = fetch_change_record(
        http_mode,
        &job.region,
        &job.environment,
        &job.deployment_id,
        &job.job_id,
        &operation.to_uppercase(),
    )
    .await?;

    Ok(DriftOutcome {
        deployment_status: deployment.status.clone(),
        resource_changes: change_record.resource_changes,
    })
}
