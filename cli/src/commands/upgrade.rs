use self_update::update::Release;

fn get_current_version() -> semver::Version {
    let version_str = env!("APP_VERSION");
    semver::Version::parse(version_str).unwrap_or_else(|e| {
        eprintln!("Failed to parse current version '{}': {}", version_str, e);
        std::process::exit(1);
    })
}

fn get_latest_stable_version(releases: &[Release]) -> Option<semver::Version> {
    releases
        .iter()
        .filter_map(|release| {
            let version = semver::Version::parse(&release.version).ok()?;
            // Only include stable versions (no pre-release, beta etc..)
            if version.pre.is_empty() {
                Some(version)
            } else {
                None
            }
        })
        .max()
}

fn needs_upgrade(current: &semver::Version, latest: &semver::Version) -> bool {
    latest > current
}

pub async fn handle_upgrade(check_only: bool) {
    let current_version = get_current_version();
    println!("Current version: {}", current_version);

    // self_update is blocking and creates its own runtime, so we need to spawn it in a blocking task
    let result = tokio::task::spawn_blocking(|| {
        self_update::backends::github::ReleaseList::configure()
            .repo_owner("infraweave-io")
            .repo_name("infraweave")
            .build()
            .unwrap()
            .fetch()
    })
    .await;

    let releases = match result {
        Ok(Ok(releases)) => releases,
        Ok(Err(e)) => {
            println!("error: {}", e);
            std::process::exit(1);
        }
        Err(e) => {
            println!("error spawning task: {}", e);
            std::process::exit(1);
        }
    };

    let latest_version = match get_latest_stable_version(&releases) {
        Some(version) => version,
        None => {
            println!("No stable releases found");
            std::process::exit(1);
        }
    };

    println!("Latest stable version: {}", latest_version);

    if needs_upgrade(&current_version, &latest_version) {
        println!(
            "An upgrade is available: {} -> {}",
            current_version, latest_version
        );

        if check_only {
            println!("Run 'infraweave upgrade' without --check to install the update.");
        } else {
            println!("Downloading and installing version {}...", latest_version);

            let status = tokio::task::spawn_blocking(move || {
                self_update::backends::github::Update::configure()
                    .repo_owner("infraweave-io")
                    .repo_name("infraweave")
                    .bin_name("cli")
                    .target_version_tag(&format!("v{}", latest_version))
                    .show_download_progress(true)
                    .current_version(&current_version.to_string())
                    .build()
                    .and_then(|updater| updater.update())
            })
            .await;

            match status {
                Ok(Ok(status)) => {
                    println!("✓ Successfully upgraded to version {}", status.version());
                    println!("Please restart the CLI to use the new version.");
                }
                Ok(Err(e)) => {
                    eprintln!("Failed to upgrade: {}", e);
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Error during upgrade: {}", e);
                    std::process::exit(1);
                }
            }
        }
    } else {
        println!("You are already on the latest version!");
    }
}
