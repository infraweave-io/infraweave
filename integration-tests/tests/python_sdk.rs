//! Integration test for the compiled `infraweave` Python SDK. Starts the local
//! backend (DynamoDB + MinIO via testcontainers, seeded with the aws-5 provider
//! and the s3bucketsimple module), then builds and exercises the compiled module
//! through pytest inside Docker — no host Python/maturin needed.
//!
//! The module runs in `aws_direct` mode and reaches the backend over its Docker
//! bridge IP, avoiding flaky host port-forwarding. (`http` mode discovers no
//! modules at import: NoCloudProvider::get_all_latest_module returns nothing.)
//!
//! apply()/destroy() block until the deployment is terminal. Instead of running
//! the real terraform_runner, the loop below impersonates it (start_runner is
//! short-circuited via TEST_MODE): it watches for the submitted deployment,
//! checks its payload, and marks it Successful so the SDK call returns.
//!
//! Opt-in (requires Docker): make python-sdk-integration-test

#[cfg(test)]
mod python_sdk_tests {
    use bollard::container::LogOutput;
    use bollard::models::{ContainerCreateBody, HostConfig};
    use bollard::query_parameters::{
        CreateContainerOptionsBuilder, ListContainersOptionsBuilder, LogsOptionsBuilder,
        RemoveContainerOptionsBuilder, StartContainerOptions, WaitContainerOptions,
    };
    use bollard::Docker;
    use env_common::interface::GenericCloudHandler;
    use env_defs::{CloudProvider, CloudProviderCommon, DeploymentStatus};
    use futures::StreamExt;
    use integration_tests::scaffold::ALL_IMAGES;
    use std::path::Path;
    use std::process::Command;
    use tokio::io::AsyncWriteExt;

    const IMAGE: &str = "infraweave-py-itest";
    const CONTAINER_NAME: &str = "infraweave-py-itest-run";
    const CARGO_VOLUME: &str = "infraweave-py-itest-cargo";
    const TARGET_VOLUME: &str = "infraweave-py-itest-target";

    // Must match what `test_local_sdk.py::test_deployment_lifecycle` submits.
    const APPLY_ENV: &str = "python/dev"; // namespace "dev" is normalized to "python/dev"
    const APPLY_DEPLOYMENT_ID: &str = "s3bucketsimple/pytest-bucket"; // "<module>/<name>"
    const APPLY_VERSION: &str = "1.0.0";
    const APPLY_BUCKET_NAME: &str = "pytest-bucket-abc123";

    /// Safety net for crashed runs: remove the (fixed-name) runner container and
    /// any leftover DynamoDB/MinIO backend containers. We match the image repo
    /// ourselves (ignoring the tag, and suffix-matching so a DOCKER_IMAGE_MIRROR
    /// registry prefix still matches) since the daemon's `ancestor` filter needs
    /// an exact repo:tag.
    async fn cleanup_stale_containers(docker: &Docker) {
        let opts = ListContainersOptionsBuilder::default().all(true).build();
        let Ok(containers) = docker.list_containers(Some(opts)).await else {
            return;
        };
        let rm = RemoveContainerOptionsBuilder::default()
            .force(true)
            .v(true)
            .build();
        for c in containers {
            let image = c.image.as_deref().unwrap_or_default();
            let repo = image.split(':').next().unwrap_or(image);
            let is_backend = ALL_IMAGES
                .iter()
                .any(|img| repo == *img || repo.ends_with(&format!("/{img}")));
            // The daemon prefixes container names with '/'.
            let is_runner = c
                .names
                .iter()
                .flatten()
                .any(|n| n.trim_start_matches('/') == CONTAINER_NAME);
            if let (true, Some(id)) = (is_backend || is_runner, c.id) {
                let _ = docker.remove_container(&id, Some(rm.clone())).await;
            }
        }
    }

    /// Impersonates the terraform_runner: when the deployment is in-progress,
    /// checks the SDK submitted the expected payload, then marks it Successful so
    /// the blocking apply()/destroy() poll loop returns. Returns the job_id it
    /// completed this tick (each command — apply, destroy — is a distinct job), or
    /// None if nothing was in-progress. Uses get_deployment (exact-PK) since
    /// get_all_deployments' begins_with(PK, …) query is rejected.
    async fn complete_busy_deployment(handler: &GenericCloudHandler) -> Option<String> {
        let mut dep = match handler
            .get_deployment(APPLY_DEPLOYMENT_ID, APPLY_ENV, true)
            .await
        {
            Ok(Some(dep)) if dep.status.is_busy() => dep,
            _ => return None, // not submitted yet, already terminal, or backend not ready
        };

        assert_eq!(
            dep.module_version, APPLY_VERSION,
            "submitted claim has unexpected module version"
        );
        assert_eq!(
            dep.variables.get("bucket_name").and_then(|v| v.as_str()),
            Some(APPLY_BUCKET_NAME),
            "submitted claim has unexpected variables: {}",
            dep.variables
        );

        dep.status = DeploymentStatus::Successful;
        dep.error_text = String::new();
        if dep.output.is_null() {
            dep.output = serde_json::json!({});
        }
        let job_id = dep.job_id.clone();
        if let Err(e) = handler.set_deployment(&dep, false).await {
            eprintln!("fake runner: failed to mark deployment Successful: {}", e);
        }
        Some(job_id)
    }

    #[tokio::test]
    async fn test_python_sdk_against_local_backend() {
        if std::env::var("INFRAWEAVE_RUN_PY_SDK_TEST").is_err() {
            eprintln!(
                "Skipping Python SDK integration test. Set INFRAWEAVE_RUN_PY_SDK_TEST=1 \
                 to enable (requires Docker)."
            );
            return;
        }

        // Seeding resolves paths relative to the workspace root.
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("integration-tests must be inside the workspace");
        std::env::set_current_dir(workspace_root).expect("Failed to set CWD to workspace root");

        let docker = Docker::connect_with_local_defaults().expect("connect to Docker daemon");
        cleanup_stale_containers(&docker).await;

        // Starts DynamoDB + MinIO, bootstraps tables/buckets, seeds the provider +
        // module, and sets CLOUD_PROVIDER / AWS creds / endpoints in our env. Held
        // until the end of the test; dropping it removes the backend containers.
        let infra = internal_api::local_setup::start_local_infrastructure()
            .await
            .expect("Failed to start local infrastructure");

        env_common::interface::initialize_project_id_and_region().await;

        // Reach the backend by its bridge IP + internal port (the test container
        // joins the same default bridge).
        let dynamo_ip = infra
            .dynamodb
            .get_bridge_ip_address()
            .await
            .expect("dynamodb bridge ip");
        let minio_ip = infra
            .minio
            .get_bridge_ip_address()
            .await
            .expect("minio bridge ip");

        // The container doesn't inherit our env, so forward the vars
        // start_local_infrastructure set, then add the bridge endpoints and test
        // flags below.
        let mut env: Vec<String> = [
            "CLOUD_PROVIDER",
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_REGION",
            "ACCOUNT_ID",
            "INFRAWEAVE_ENV",
            "AWS_S3_FORCE_PATH_STYLE",
        ]
        .iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| format!("{k}={v}")))
        .collect();
        env.extend([
            format!("DYNAMODB_ENDPOINT=http://{dynamo_ip}:8000"),
            format!("MINIO_ENDPOINT=http://{minio_ip}:9000"),
            // TEST_MODE short-circuits start_runner (no ECS); the loop below plays
            // the runner. INFRAWEAVE_TEST_APPLY enables the deployment lifecycle
            // test, which only completes because of that loop.
            "TEST_MODE=1".into(),
            "INFRAWEAVE_TEST_APPLY=1".into(),
            // Keep cargo's registry/target in volumes so reruns don't recompile
            // from scratch, and Linux artifacts off the host.
            "CARGO_HOME=/cargo".into(),
            "CARGO_TARGET_DIR=/target".into(),
        ]);

        // Build the toolchain image (layers cache across runs).
        let build = Command::new("docker")
            .args([
                "build",
                "-f",
                "infraweave_py/Dockerfile.itest",
                "-t",
                IMAGE,
                "infraweave_py",
            ])
            .current_dir(workspace_root)
            .status()
            .expect("Failed to run `docker build`");
        assert!(build.success(), "docker build failed");

        // Build the cdylib + run pytest in the container. Mount the workspace
        // read-only so the build/test can't write to the host tree.
        let created = docker
            .create_container(
                Some(
                    CreateContainerOptionsBuilder::default()
                        .name(CONTAINER_NAME)
                        .build(),
                ),
                ContainerCreateBody {
                    image: Some(IMAGE.to_string()),
                    env: Some(env),
                    host_config: Some(HostConfig {
                        binds: Some(vec![
                            format!("{}:/workspace:ro", workspace_root.display()),
                            format!("{}:/cargo", CARGO_VOLUME),
                            format!("{}:/target", TARGET_VOLUME),
                        ]),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .await
            .expect("Failed to create test container");
        let id = created.id;
        docker
            .start_container(&id, None::<StartContainerOptions>)
            .await
            .expect("Failed to start test container");

        // Stream the container's maturin/pytest output to our stdout (shown under
        // --nocapture). The follow stream ends when the container stops.
        let logs_task = {
            let docker = docker.clone();
            let id = id.clone();
            tokio::spawn(async move {
                let opts = LogsOptionsBuilder::default()
                    .follow(true)
                    .stdout(true)
                    .stderr(true)
                    .build();
                let mut stream = docker.logs(&id, Some(opts));
                let mut out = tokio::io::stdout();
                while let Some(frame) = stream.next().await {
                    match frame {
                        Ok(LogOutput::StdOut { message })
                        | Ok(LogOutput::StdErr { message })
                        | Ok(LogOutput::Console { message }) => {
                            let _ = out.write_all(&message).await;
                            let _ = out.flush().await;
                        }
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("container log stream error: {}", e);
                            break;
                        }
                    }
                }
            })
        };

        // Play the runner until the container exits: once a second, complete any
        // in-progress deployment so the SDK's blocking calls can return, while
        // concurrently waiting for the container to stop.
        let handler = GenericCloudHandler::default().await;
        let mut completed_jobs: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut wait = docker.wait_container(&id, None::<WaitContainerOptions>);
        let exit_code: i64 = loop {
            tokio::select! {
                // .next() is cancel-safe, so re-polling it after the timer wins is fine.
                waited = wait.next() => break match waited {
                    Some(Ok(resp)) => resp.status_code,
                    // wait_container maps a non-zero exit to this error.
                    Some(Err(bollard::errors::Error::DockerContainerWaitError { code, .. })) => code,
                    Some(Err(e)) => panic!("Failed to wait on test container: {}", e),
                    None => 0, // stream ended without a response → container stopped cleanly
                },
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                    if let Some(job_id) = complete_busy_deployment(&handler).await {
                        completed_jobs.insert(job_id);
                    }
                }
            }
        };

        // Container stopped, so the follow stream has ended; drain it.
        let _ = logs_task.await;

        // No auto-remove (it races the exit-code read), so clean up here.
        let _ = docker
            .remove_container(
                &id,
                Some(
                    RemoveContainerOptionsBuilder::default()
                        .force(true)
                        .v(true)
                        .build(),
                ),
            )
            .await;

        assert_eq!(
            exit_code, 0,
            "Python SDK pytest failed (exit code: {})",
            exit_code
        );
        // Sanity: the fake runner actually completed at least one in-progress
        // deployment (so the payload assertions above ran).
        assert!(
            !completed_jobs.is_empty(),
            "harness never saw an in-progress deployment — the SDK apply path was \
             not exercised (did the deployment lifecycle test run?)"
        );

        // Explicitly confirm both commands ran — not just two jobs, but an apply
        // *and* a destroy. Each event records the command verbatim in its `event`
        // field; get_events uses an exact-PK query, so it works on the local backend.
        let commands: std::collections::HashSet<String> = handler
            .get_events(APPLY_DEPLOYMENT_ID, APPLY_ENV)
            .await
            .expect("failed to read deployment events")
            .into_iter()
            .map(|e| e.event)
            .collect();
        assert!(
            commands.contains("apply"),
            "no apply event recorded for the deployment; events seen: {commands:?}"
        );
        assert!(
            commands.contains("destroy"),
            "no destroy event recorded — the context-manager auto-destroy did not \
             run; events seen: {commands:?}"
        );
    }
}
