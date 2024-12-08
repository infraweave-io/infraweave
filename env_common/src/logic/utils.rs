use crate::errors::ModuleError;
use env_utils::semver_parse;
use log::info;

#[derive(PartialEq)]
pub enum ModuleType {
    Module,
    Stack,
}

pub fn ensure_track_matches_version(track: &str, version: &str) -> Result<(), ModuleError> {
    let manifest_version = semver_parse(version).unwrap();
    info!(
        "Manifest version: {}. Checking if this is the newest",
        manifest_version
    );
    match &manifest_version.pre.to_string() == track {
        true => {
            if track == "dev" || track == "alpha" || track == "beta" || track == "rc" {
                println!("Pushing to {} track", track);
            } else if track == "stable" {
                return Err(ModuleError::InvalidStableVersion);
            } else {
                return Err(ModuleError::InvalidTrack(track.to_string()));
            }
        }
        false => {
            if manifest_version.pre.to_string().is_empty() && track == "stable" {
                info!("Pushing to stable track");
            } else {
                return Err(ModuleError::InvalidTrackPrereleaseVersion(
                    track.to_string(),
                    manifest_version.pre.to_string(),
                ));
            }
        }
    };

    Ok(())
}
