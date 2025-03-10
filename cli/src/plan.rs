use std::{collections::HashMap, thread, time::Duration, vec};

use anyhow::Result;
use colored::Colorize;
use env_common::logic::is_deployment_plan_in_progress;
use env_defs::{CloudProvider, DeploymentResp};
use prettytable::{row, Table};

use log::error;

use crate::handler;

pub async fn follow_plan(
    job_ids: &Vec<(String, String, String)>,
) -> Result<(String, String, String), anyhow::Error> {
    // Keep track of statuses in a hashmap
    let mut statuses: HashMap<String, DeploymentResp> = HashMap::new();

    // Polling loop to check job statuses periodically until all are finished
    loop {
        let mut all_successful = true;

        for (job_id, deployment_id, environment) in job_ids {
            let (in_progress, job_id, deployment) = is_deployment_plan_in_progress(
                &handler().await,
                deployment_id,
                environment,
                job_id,
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
        }

        if all_successful {
            println!("All jobs are successful!");
            break;
        }

        thread::sleep(Duration::from_secs(10));
    }

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

    for (job_id, deployment_id, environment) in job_ids {
        overview_table.add_row(row![
            format!("{}\n({})", deployment_id, environment),
            statuses.get(job_id).unwrap().status,
            statuses.get(job_id).unwrap().job_id,
            format!(
                "{} policy violations",
                statuses
                    .get(job_id)
                    .unwrap()
                    .policy_results
                    .iter()
                    .filter(|p| p.failed)
                    .count()
            )
        ]);

        match handler()
            .await
            .get_change_record(environment, deployment_id, job_id, "PLAN")
            .await
        {
            Ok(change_record) => {
                println!(
                    "Change record for deployment {} in environment {}:\n{}",
                    deployment_id, environment, change_record.plan_std_output
                );
                std_output_table.add_row(row![
                    format!("{}\n({})", deployment_id, environment),
                    change_record.plan_std_output
                ]);
            }
            Err(e) => {
                error!("Failed to get change record: {}", e);
            }
        }

        if statuses.get(job_id).unwrap().status == "failed_policy" {
            println!(
                "Policy validation failed for deployment {} in {}",
                deployment_id, environment
            );
            for result in statuses
                .get(job_id)
                .unwrap()
                .policy_results
                .iter()
                .filter(|p| p.failed)
            {
                violations_table.add_row(row![
                    format!("{}\n({})", deployment_id, environment),
                    result.policy,
                    serde_json::to_string_pretty(&result.violations).unwrap()
                ]);
            }
            println!(
                "Policy results: {:?}",
                statuses.get(job_id).unwrap().policy_results
            );
        } else {
            println!(
                "Policy validation passed for deployment {:?}",
                statuses.get(job_id).unwrap()
            );
        }
    }

    overview_table.printstd();
    std_output_table.printstd();
    violations_table.printstd();

    Ok((
        overview_table.to_string(),
        std_output_table.to_string(),
        violations_table.to_string(),
    ))
}
