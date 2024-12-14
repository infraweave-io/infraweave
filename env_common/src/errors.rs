use thiserror::Error;

#[derive(Error, Debug)]
pub enum ModuleError {
    #[error("Module version {0} already exists: {1}")]
    ModuleVersionExists(String, String),

    #[error("Module track {0} must be one of 'stable', 'rc', 'beta', 'alpha', 'dev'")]
    InvalidTrack(String),

    #[error("Track '{0}' must match the pre-release version '{1}', and be one of the allowed tracks: 'rc', 'beta', 'alpha', 'dev', 'stable'.")]
    InvalidTrackPrereleaseVersion(String, String),

    #[error(
        "Pushing to stable track should not specify pre-release version, only major.minor.patch"
    )]
    InvalidStableVersion,

    #[error("Invalid module schema: {0}")]
    InvalidModuleSchema(String),

    #[error(".terraform.lock.hcl file does not exist in the specified directory. To guarantee consistent behaviour by always using the same dependency versions, please run `terraform init` in the directory before proceeding.")]
    TerraformLockfileMissing,

    #[error("Failed to upload module: {0}")]
    UploadModuleError(String),

    #[error("Failed to zip module: {0}")]
    ZipError(String),

    #[error("Module example has invalid variable: {0}")]
    InvalidExampleVariable(String),

    #[error("Other error occurred: {0}")]
    Other(#[from] anyhow::Error),
}
