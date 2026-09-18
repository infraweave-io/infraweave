use self_update::update::Release;

fn get_target_name() -> &'static str {
    match std::env::consts::OS {
        "linux" => match std::env::consts::ARCH {
            "x86_64" => "linux-amd64",
            "aarch64" => "linux-arm64",
            _ => panic!("Unsupported architecture"),
        },
        "macos" => match std::env::consts::ARCH {
            "x86_64" => "macos-amd64",
            "aarch64" => "macos-arm64",
            _ => panic!("Unsupported architecture"),
        },
        "windows" => match std::env::consts::ARCH {
            "x86_64" => "windows-amd64",
            _ => panic!("Unsupported architecture"),
        },
        _ => panic!("Unsupported operating system"),
    }
}

fn get_current_version() -> semver::Version {
    let version_str = env!("APP_VERSION");
    // Strip the 'v' prefix if present
    let version_str = version_str.strip_prefix('v').unwrap_or(version_str);
    semver::Version::parse(version_str).unwrap_or_else(|e| {
        eprintln!("Failed to parse current version '{}': {}", version_str, e);
        std::process::exit(1);
    })
}

fn get_latest_version(releases: &[Release], include_prerelease: bool) -> Option<semver::Version> {
    releases
        .iter()
        .filter_map(|release| {
            // Strip the 'v' prefix if present
            let version_str = release
                .version
                .strip_prefix('v')
                .unwrap_or(&release.version);
            let version = semver::Version::parse(version_str).ok()?;

            if include_prerelease {
                // Include all versions (stable and pre-releases)
                Some(version)
            } else {
                // Only include stable versions (no pre-releases)
                if version.pre.is_empty() {
                    Some(version)
                } else {
                    None
                }
            }
        })
        .max()
}

fn needs_upgrade(current: &semver::Version, latest: &semver::Version) -> bool {
    latest > current
}

pub async fn handle_upgrade(check_only: bool, include_prerelease: bool) {
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

    let latest_version = match get_latest_version(&releases, include_prerelease) {
        Some(version) => version,
        None => {
            let release_type = if include_prerelease {
                "releases"
            } else {
                "stable releases"
            };
            println!("No {} found", release_type);
            std::process::exit(1);
        }
    };
    let version_type = if include_prerelease && !latest_version.pre.is_empty() {
        "Latest version (including pre-release)"
    } else {
        "Latest stable version"
    };
    println!("{}: {}", version_type, latest_version);

    if needs_upgrade(&current_version, &latest_version) {
        println!(
            "An upgrade is available: {} -> {}",
            current_version, latest_version
        );

        if check_only {
            println!("Run 'infraweave upgrade' without --check to install the update.");
        } else {
            println!("Downloading and installing version {}...", latest_version);

            let target_name = get_target_name();
            let bin_name = if cfg!(windows) { "cli.exe" } else { "cli" };
            let bin_path = format!("cli-{}-v{}", target_name, latest_version);

            let status = tokio::task::spawn_blocking(move || {
                self_update::backends::github::Update::configure()
                    .repo_owner("infraweave-io")
                    .repo_name("infraweave")
                    .bin_name(bin_name)
                    .target(target_name)
                    .bin_path_in_archive(&bin_path)
                    .target_version_tag(&format!("v{}", latest_version))
                    .show_download_progress(true)
                    .current_version(&current_version.to_string())
                    .no_confirm(true)
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
