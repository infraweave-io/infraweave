use std::env;

use aws_lambda_events::event::sqs::SqsEvent;
use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use env_defs::{CheckRunOutput, CloudProvider, ExtraData};
use env_utils::setup_logging;
use gitops::{
    get_project_id_for_repository_path, get_securestring_aws, handle_check_run_event,
    handle_package_publish_event, handle_process_push_event, handle_validate_github_event,
    post_check_run_from_payload,
};
use lambda_runtime::{service_fn, Error, LambdaEvent};
use log::info;
use serde_json::Value;

async fn handle_sqs_run_response(sqs_event: SqsEvent) -> Result<Value, Error> {
    for record in sqs_event.records {
        let body_str = record.body.expect("No body found");
        let body: Value = serde_json::from_str(&body_str)?;
        let subject = body
            .get("Subject")
            .expect("No subject found")
            .as_str()
            .expect("Subject is not a string");

        let message = body
            .get("Message")
            .expect("No message found")
            .as_str()
            .expect("Message is not a string");

        println!("Subject: {:?}", subject);
        println!("Message: {:?}", message);

        let payload = match serde_json::from_str::<Value>(message) {
            Ok(payload) => payload,
            Err(e) => {
                println!("Failed to parse message payload: {:?}", e);
                return Ok(
                    serde_json::json!({ "status": format!("Failed to parse message payload: {:?}. Skipping this event.", e) }),
                );
            }
        };

        println!("Processing {} event", subject);
        match subject {
            "validated_github_event" => {
                process_validated_github_event(payload).await?;
            }
            "runner_event" => {
                process_runner_event(payload).await?;
            }
            "publish_module_event" => {
                process_validated_github_event(payload).await?;
            }
            unknownn_event => {
                info!(
                    "Non-handled message with subject {}: {:?}",
                    unknownn_event, payload
                );
            }
        }
    }
    Ok(serde_json::json!({ "status": "SQS messages processed" }))
}

async fn process_validated_github_event(payload: Value) -> Result<Value, Error> {
    if let Some(event_type) = &payload
        .get("headers")
        .and_then(|headers| headers.get("x-github-event"))
        .and_then(|value| value.as_str())
    {
        match *event_type {
            "push" => {
                return match handle_process_push_event(&payload).await {
                    Ok(response) => Ok(response),
                    Err(e) => {
                        println!("Error handling push event: {}", e);
                        Ok(
                            serde_json::json!({ "status": format!("Error handling push event: {}", e) }),
                        )
                    }
                }
            }
            "check_run" => {
                return match handle_check_run_event(&payload).await {
                    Ok(response) => Ok(response),
                    Err(e) => {
                        println!("Error handling check_run event: {}", e);
                        Ok(
                            serde_json::json!({ "status": format!("Error handling check_run event: {}", e) }),
                        )
                    }
                }
            }
            "registry_package" => {
                return match handle_package_publish_event(&payload).await {
                    Ok(response) => Ok(response),
                    Err(e) => {
                        println!("Error handling registry_package event: {}", e);
                        Ok(
                            serde_json::json!({ "status": format!("Error handling registry_package event: {}", e) }),
                        )
                    }
                }
            }
            _ => {
                println!("Unsupported event type: {}", event_type);
                println!("Event: {}", serde_json::to_string(&payload).unwrap());
                return Ok(
                    serde_json::json!({ "status": format!("Unsupported event type: {}", event_type) }),
                );
            }
        }
    } else {
        println!("No x-github-event header found in payload");
    }

    Ok(
        serde_json::json!({ "status": format!("Unknown validated github event in processor: {}", payload) }),
    )
}

async fn process_runner_event(payload: Value) -> Result<Value, Error> {
    let event: ExtraData = match serde_json::from_value(payload.clone()) {
        Ok(event) => event,
        Err(e) => {
            println!("Failed to parse payload: {:?}", e);
            ExtraData::None
        }
    };

    println!("Extra Data Content: {:?}", event);

    match event {
        ExtraData::GitHub(mut github_event) => {
            println!("GitHub Event: {:?}", github_event);

            let project_id =
                get_project_id_for_repository_path(&github_event.repository.full_name).await?;
            let region = &github_event.job_details.region;
            println!(
                "Found project id: {}, region: {} for path: {}",
                project_id, region, &github_event.repository.full_name
            );
            let handler = GenericCloudHandler::workload(&project_id, region).await;

            let status = github_event.job_details.status.as_str();

            let information = if status == "success" {
                github_event.check_run.conclusion = Some("success".into());

                let change_record = handler
                    .get_change_record(
                        &github_event.job_details.environment,
                        &github_event.job_details.deployment_id,
                        &github_event.job_details.job_id,
                        &github_event.job_details.change_type,
                    )
                    .await
                    .expect("Failed to get change record");
                change_record.plan_std_output
            } else {
                github_event.check_run.conclusion = Some("failure".into());
                github_event.job_details.error_text.clone()
            };

            // Process GitHub event.
            github_event.check_run.status = "completed".into();
            github_event.check_run.conclusion = if status == "success" {
                Some("success".into())
            } else {
                Some("failure".into())
            };

            github_event.check_run.completed_at = chrono::Utc::now().to_rfc3339().into();
            github_event.check_run.output = Some(CheckRunOutput {
                title: format!("{} job completed", github_event.job_details.change_type),
                summary: format!("Job completed with {}", status),
                text: Some(format!(
                    r#"
# Job Details
File: **{}**
Deployment ID: **{}**
Environment: **{}**

## Information

```diff
{}
```
                "#,
                    github_event.job_details.file_path,
                    github_event.job_details.deployment_id,
                    github_event.job_details.environment,
                    information
                )),
                annotations: None,
            });
            println!("Payload: {:?}", github_event);

            let private_key_pem_ssm_key = env::var("GITHUB_PRIVATE_KEY_PARAMETER_STORE_KEY")
                .expect("GITHUB_PRIVATE_KEY_PARAMETER_STORE_KEY environment variable not set");
            let private_key_pem = get_securestring_aws(&private_key_pem_ssm_key).await?; // Read here to avoid multiple reads of the same secret
                                                                                         // https://docs.github.com/en/rest/checks/runs?apiVersion=2022-11-28#update-a-check-run
            match post_check_run_from_payload(github_event, &private_key_pem).await {
                Ok(resp) => {
                    info!("Check run posted: {}", resp);
                }
                Err(e) => {
                    info!("Error posting check run: {}", e);
                }
            }
        }
        ExtraData::GitLab(_gitlab_event) => {
            // Process GitLab event.
        }
        ExtraData::None => {
            // No event data found.
            println!("Cannot map ExtraData");
        }
    }

    Ok(serde_json::json!({ "status": "Runner event processed" }))
}

async fn validator_func(event: LambdaEvent<Value>) -> Result<Value, Error> {
    let (_generic_event, _context) = event.into_parts();

    if env::var("DEBUG").is_ok() {
        println!("Context: {}", serde_json::to_string(&_context).unwrap());
        println!("Event: {}", serde_json::to_string(&_generic_event).unwrap());
    }

    if let Some(event_type) = &_generic_event
        .get("headers")
        .and_then(|headers| headers.get("x-github-event"))
        .and_then(|value| value.as_str())
    {
        match event_type {
            &"push" | &"check_run" | &"registry_package" => {
                // add more supported events here
                return match handle_validate_github_event(&_generic_event).await {
                    Ok(response) => Ok(response),
                    Err(e) => {
                        println!("Error validating event type {}: {}", event_type, e);
                        Ok(
                            serde_json::json!({ "status": format!("Error validating event type {}: {}", event_type, e) }),
                        )
                    }
                };
            }
            _ => {
                println!("Unsupported event type: {}", event_type);
                println!("Event: {}", serde_json::to_string(&_generic_event).unwrap());
                return Ok(
                    serde_json::json!({ "status": format!("Unsupported event type: {}", event_type) }),
                );
            }
        }
    }

    // TODO: add support for gitlab and other git-providers
    Ok(serde_json::json!({ "status": "Request in validator without action" }))
}

async fn processor_func(event: LambdaEvent<Value>) -> Result<Value, Error> {
    let (_generic_event, _context) = event.into_parts();

    if env::var("DEBUG").is_ok() {
        println!("Context: {}", serde_json::to_string(&_context).unwrap());
        println!("Event: {}", serde_json::to_string(&_generic_event).unwrap());
    }

    if _generic_event.get("Records").is_some() {
        let sqs_event: SqsEvent = serde_json::from_value(_generic_event.clone())?;
        return match handle_sqs_run_response(sqs_event).await {
            Ok(response) => Ok(response),
            Err(e) => {
                println!("Error handling SQS event: {}", e);
                Ok(serde_json::json!({ "status": format!("Error handling SQS event: {}", e) }))
            }
        };
    }

    // TODO: add support for gitlab and other git-providers
    Ok(serde_json::json!({ "status": "Default event in processor, not sure what to do..." }))
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    setup_logging().unwrap();
    initialize_project_id_and_region().await;

    match env::var("RUN_MODE").as_deref() {
        Ok("VALIDATOR") => {
            info!("Running in VALIDATOR mode");
            // TODO: Add support for azure
            let fun = service_fn(validator_func);
            lambda_runtime::run(fun).await?;
        }
        Ok("PROCESSOR") => {
            info!("Running in PROCESSOR mode");
            // TODO: Add support for azure
            let fun = service_fn(processor_func);
            lambda_runtime::run(fun).await?;
        }
        Ok("OCI_POLLER") => {
            info!("Running in OCI_POLLER mode");
            let fun = service_fn(|_event: LambdaEvent<Value>| async move {
                let poll_interval_minutes = env::var("OCI_POLL_INTERVAL_MINUTES")
                    .expect("OCI_POLL_INTERVAL_MINUTES must be a valid number")
                    .parse::<u64>()
                    .unwrap();
                let github_org =
                    env::var("GITHUB_ORG").expect("GITHUB_ORG environment variable not set");
                let new_pkgs =
                    gitops::poll_and_process_new_packages(&github_org, poll_interval_minutes)
                        .await
                        .map_err(|e| Error::from(format!("Failed to poll packages: {}", e)))?;
                for (i, pkg) in new_pkgs.into_iter().enumerate() {
                    println!("New {}: {}", i, serde_json::to_value(pkg).unwrap());
                }
                Ok::<Value, Error>(serde_json::json!({ "status": "OCI polling completed" }))
            });
            lambda_runtime::run(fun).await?;
        }
        _ => {
            info!("No valid RUN_MODE is set, exiting without action...");
        }
    }

    Ok(())
}
