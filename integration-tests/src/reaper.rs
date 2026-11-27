//! Removes the containers of a test run once its last test process exits.
//!
//! One Ryuk container (testcontainers' reaper) runs per test run. Every test
//! process keeps a connection to it open and registers the run label; the OS
//! closes the connection however the process ends. When no process has been
//! connected for `RECONNECTION_TIMEOUT`, Ryuk removes every container with the
//! label, then removes itself. This also covers the shared fixture containers,
//! which outlive the process that started them.

use crate::scaffold::TEST_CONTAINER_LABEL;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;
use testcontainers::core::{IntoContainerPort, Mount};
use testcontainers::{runners::AsyncRunner, GenericImage, ImageExt};

/// Label carrying the test run a container belongs to.
pub const RUN_LABEL: &str = "infraweave.integration-test.run";

const RYUK_IMAGE: &str = "testcontainers/ryuk";
const RYUK_TAG: &str = "0.11.0";
const RYUK_PORT: u16 = 8080;
/// Long enough to bridge the gap between test processes in a run.
const RECONNECTION_TIMEOUT: &str = "30s";
/// Docker socket path as seen by the daemon (inside the VM on Colima/Docker Desktop).
const DOCKER_SOCKET_ENV: &str = "TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE";
/// Age after which a run that never recorded a reaper is considered abandoned.
const STALE_RUN_AGE: Duration = Duration::from_secs(600);

static RUN_ID: OnceLock<String> = OnceLock::new();
static CONNECTION: tokio::sync::OnceCell<TcpStream> = tokio::sync::OnceCell::const_new();

/// Identifies the current test run: nextest's run id, or a random token when
/// running under plain `cargo test` (one process per run).
pub fn run_id() -> &'static str {
    RUN_ID.get_or_init(|| {
        std::env::var("NEXTEST_RUN_ID")
            .unwrap_or_else(|_| format!("process-{:016x}", rand::random::<u64>()))
    })
}

/// Directory for cross-process coordination files of the integration tests.
pub(crate) fn state_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("infraweave-integration-tests");
    std::fs::create_dir_all(&dir).expect("Failed to create integration test state dir");
    dir
}

/// Coordination files of the current run; a later run removes them once this
/// one has finished (see `prune_finished_runs`).
pub(crate) fn run_dir() -> PathBuf {
    let dir = runs_dir().join(run_id());
    std::fs::create_dir_all(&dir).expect("Failed to create test run state dir");
    dir
}

fn runs_dir() -> PathBuf {
    state_dir().join("runs")
}

/// Takes an exclusive lock on `dir/name.lock`, shared by all test processes;
/// released on drop.
pub(crate) async fn lock(dir: &Path, name: &str) -> File {
    let path = dir.join(format!("{name}.lock"));
    tokio::task::spawn_blocking(move || {
        let file = File::create(&path).expect("Failed to create lock file");
        file.lock().expect("Failed to take lock");
        file
    })
    .await
    .unwrap()
}

/// Makes sure this run has a reaper and this process is registered with it.
/// Call before starting any container labelled with `RUN_LABEL`.
pub async fn ensure_reaper() {
    CONNECTION.get_or_init(connect_to_reaper).await;
}

async fn connect_to_reaper() -> TcpStream {
    let dir = run_dir();
    let _lock = lock(&dir, "ryuk").await;
    let port_file = dir.join("ryuk.port");

    // Ryuk exits after reaping, so a recorded port may be stale.
    if let Some(port) = std::fs::read_to_string(&port_file)
        .ok()
        .and_then(|p| p.trim().parse::<u16>().ok())
    {
        if let Some(connection) = register(port, 1) {
            return connection;
        }
    }

    prune_finished_runs();
    let port = start_ryuk().await;
    std::fs::write(&port_file, port.to_string()).expect("Failed to record reaper port");
    // Ryuk needs a moment to listen after the container starts.
    register(port, 40).expect("Failed to register with the reaper")
}

/// Removes the coordination files of runs whose reaper is gone. Best effort.
fn prune_finished_runs() {
    let Ok(entries) = std::fs::read_dir(runs_dir()) else {
        return;
    };
    for dir in entries.flatten().map(|entry| entry.path()) {
        if dir.file_name().is_some_and(|name| name == run_id()) {
            continue;
        }
        // Held while that run starts its reaper.
        let Ok(lock_file) = File::create(dir.join("ryuk.lock")) else {
            continue;
        };
        if lock_file.try_lock().is_err() {
            continue;
        }
        let finished = match std::fs::read_to_string(dir.join("ryuk.port")) {
            Ok(port) => port.trim().parse::<u16>().map_or(true, |port| {
                let addr = SocketAddr::from(([127, 0, 0, 1], port));
                TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_err()
            }),
            // No reaper recorded: a run that just started, or one that never got that far.
            Err(_) => dir
                .metadata()
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|modified| modified.elapsed().unwrap_or_default() > STALE_RUN_AGE),
        };
        if finished {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

async fn start_ryuk() -> u16 {
    let socket = std::env::var(DOCKER_SOCKET_ENV).unwrap_or("/var/run/docker.sock".to_string());
    let container = GenericImage::new(RYUK_IMAGE, RYUK_TAG)
        .with_exposed_port(RYUK_PORT.tcp())
        .with_mount(Mount::bind_mount(socket, "/var/run/docker.sock"))
        .with_env_var("RYUK_RECONNECTION_TIMEOUT", RECONNECTION_TIMEOUT)
        // Not the run label: the reaper must not wait on or remove itself.
        .with_label(TEST_CONTAINER_LABEL, "true")
        .with_host_config_modifier(|host_config| host_config.auto_remove = Some(true))
        .start()
        .await
        .expect("Failed to start the reaper");
    let port = container.get_host_port_ipv4(RYUK_PORT).await.unwrap();
    // Ryuk outlives this process and removes itself when done.
    std::mem::forget(container);
    port
}

/// Connects to Ryuk and registers this run's label, retrying `attempts` times.
fn register(port: u16, attempts: u32) -> Option<TcpStream> {
    for attempt in 0..attempts {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(250));
        }
        let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
            continue;
        };
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
        if writeln!(stream, "label={RUN_LABEL}={}", run_id()).is_err() {
            continue;
        }
        let mut ack = String::new();
        if BufReader::new(&stream).read_line(&mut ack).is_ok() && ack.trim() == "ACK" {
            stream.set_read_timeout(None).ok()?;
            return Some(stream);
        }
    }
    None
}
