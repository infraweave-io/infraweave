//! Emulators shared by all tests of a fixture within one test run.
//!
//! nextest runs every test in its own process, so the first process to take
//! the fixture's file lock starts the emulators, publishes the fixture and
//! writes the endpoints to a state file; later processes attach to them. The
//! containers outlive that first process; the run's reaper removes them once
//! the last test process has exited (see `reaper`).

use crate::reaper::{ensure_reaper, lock, run_dir};
use crate::scaffold::{
    bootstrap_buckets, bootstrap_tables, configure_env, generate_random_test_id,
    integration_tests_dir, is_azure, start_emulators, Endpoints, TestContext,
};
use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use futures::future::join_all;
use std::future::Future;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

const PROVIDERS: &[&str] = &["aws-5", "aws-5-us-east-1", "helm-3", "kubernetes-2"];
const PROVIDER_VERSION: &str = "0.1.2";

/// Module versions to publish, one chain per module; versions within a chain
/// are published in order since older versions are rejected.
type ModuleChains = &'static [&'static [(&'static str, &'static str)]];

const S3BUCKET_DEV: &[(&str, &str)] = &[
    ("s3bucket-dev", "0.1.2-dev+test.10"),
    ("s3bucket-dev", "0.1.3-dev+test.10"),
];

/// Modules the stack tests build on.
const STACK_MODULES: ModuleChains = &[
    S3BUCKET_DEV,
    &[
        ("route53record", "0.1.2-dev+test.10"),
        ("route53record", "0.1.3-dev+test.10"),
    ],
    &[("nginx-ingress", "0.5.5-dev+test.1")],
    &[("route53alias", "0.5.5-dev+test.1")],
    &[("s3bucket-web", "0.1.2-dev+test.10")],
    &[("eks", "0.5.5-dev+test.1")],
    &[("webapp", "0.5.5-dev+test.1")],
    &[("s3bucket-with-backup", "0.5.5-dev+test.1")],
    &[("iam-role-with-policy", "0.1.0-dev+test.1")],
];

/// Modules the runner tests' claims and stacks use.
const RUNNER_MODULES: ModuleChains = &[S3BUCKET_DEV];

/// Stacks the runner tests deploy.
const RUNNER_STACKS: &[(&str, &str)] = &[("bucketcollection-stack-vars", "0.1.0-dev+test.1")];

#[derive(Clone, Copy, Debug)]
pub enum Fixture {
    /// All test providers.
    Providers,
    /// All test providers plus the modules the stack tests build on.
    ProvidersAndModules,
    /// All test providers plus the modules and stacks the runner tests deploy.
    Runner,
}

impl Fixture {
    fn name(self) -> &'static str {
        match self {
            Fixture::Providers => "providers",
            Fixture::ProvidersAndModules => "modules",
            Fixture::Runner => "runner",
        }
    }

    fn modules(self) -> ModuleChains {
        match self {
            Fixture::Providers => &[],
            Fixture::ProvidersAndModules => STACK_MODULES,
            Fixture::Runner => RUNNER_MODULES,
        }
    }

    fn stacks(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Fixture::Providers | Fixture::ProvidersAndModules => &[],
            Fixture::Runner => RUNNER_STACKS,
        }
    }

    async fn publish(self, handler: &GenericCloudHandler) {
        let fixtures_dir = &integration_tests_dir();

        join_all(PROVIDERS.iter().map(|provider| async move {
            let path = fixtures_dir.join("providers").join(provider);
            env_common::publish_provider(handler, path.to_str().unwrap(), Some(PROVIDER_VERSION))
                .await
                .unwrap_or_else(|e| panic!("Failed to publish provider {provider}: {e:?}"));
        }))
        .await;

        join_all(self.modules().iter().map(|chain| async move {
            for (module, version) in chain.iter() {
                let path = fixtures_dir.join("modules").join(module);
                env_common::publish_module(
                    handler,
                    path.to_str().unwrap(),
                    "dev",
                    Some(version),
                    None,
                )
                .await
                .unwrap_or_else(|e| panic!("Failed to publish module {module}@{version}: {e:?}"));
            }
        }))
        .await;

        join_all(self.stacks().iter().map(|(stack, version)| async move {
            let path = fixtures_dir.join("stacks").join(stack);
            env_common::publish_stack(handler, path.to_str().unwrap(), "dev", Some(version), None)
                .await
                .unwrap_or_else(|e| panic!("Failed to publish stack {stack}@{version}: {e:?}"));
        }))
        .await;
    }
}

/// Runs `function_to_test` against emulators shared with the other tests of
/// `fixture` in this run. Tests must not depend on being alone in the backend.
pub async fn shared_scaffold<F, Fut>(fixture: Fixture, function_to_test: F)
where
    F: FnOnce(TestContext) -> Fut,
    Fut: Future<Output = ()>,
{
    let endpoints = ensure_shared(fixture).await;
    configure_env(&endpoints);
    initialize_project_id_and_region().await;

    let context = TestContext::from_endpoints(&endpoints).await;
    function_to_test(context).await;
}

async fn ensure_shared(fixture: Fixture) -> Endpoints {
    let provider = if is_azure() { "azure" } else { "aws" };
    let key = format!("{provider}-{}", fixture.name());
    let dir = run_dir();
    let state_path = dir.join(format!("{key}.json"));
    // Attaching processes must hold a reaper connection too, or the last test
    // running would see the shared emulators reaped under it.
    ensure_reaper().await;
    let lock = lock(&dir, &key).await;

    if let Some(endpoints) = read_state(&state_path) {
        if is_alive(&endpoints) {
            return endpoints;
        }
        // Reaped while no test process was connected; seed again.
        let _ = std::fs::remove_file(&state_path);
    }

    // Unique names: a seeding attempt that failed may have left its lock but
    // removed its containers, and the next process retries.
    let emulators = start_emulators(&format!(
        "shared-{}-{}",
        fixture.name(),
        generate_random_test_id()
    ))
    .await;
    configure_env(&emulators.endpoints);
    initialize_project_id_and_region().await;

    let context = TestContext::from_endpoints(&emulators.endpoints).await;
    bootstrap_tables(&context).await;
    bootstrap_buckets(&context).await;
    fixture.publish(&context.api_handler).await;

    write_state(&state_path, &emulators.endpoints);
    drop(lock);

    let endpoints = emulators.endpoints.clone();
    // Keep the containers running for the other test processes.
    std::mem::forget(emulators);
    endpoints
}

/// Whether every emulator in `endpoints` still accepts connections.
fn is_alive(endpoints: &Endpoints) -> bool {
    [
        Some(&endpoints.aws_endpoint),
        endpoints.azure_endpoint.as_ref(),
        endpoints.function_endpoint.as_ref(),
    ]
    .into_iter()
    .flatten()
    .all(|url| {
        let host_port = url.trim_start_matches("http://").trim_end_matches('/');
        host_port
            .to_socket_addrs()
            .ok()
            .and_then(|mut addrs| addrs.next())
            .is_some_and(|addr| TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_ok())
    })
}

fn read_state(path: &Path) -> Option<Endpoints> {
    let content = std::fs::read_to_string(path).ok()?;
    Some(serde_json::from_str(&content).expect("Corrupt shared fixture state"))
}

fn write_state(path: &Path, endpoints: &Endpoints) {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string(endpoints).unwrap())
        .expect("Failed to write shared fixture state");
    std::fs::rename(&tmp, path).expect("Failed to write shared fixture state");
}
