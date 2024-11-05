use thiserror::Error;

#[derive(Error, Debug)]
pub enum ModuleError {
    #[error("Module version {0} already exists")]
    ModuleVersionExists(String),

    #[error("Module track {0} must be one of 'stable', 'rc', 'beta', 'alpha', 'dev'")]
    InvalidTrack(String),

    #[error("Track '{0}' must match be one of the allowed tracks: 'rc', 'beta', 'alpha', 'dev', 'stable'. And match the pre-release version '{1}'")]
    InvalidTrackPrereleaseVersion(String, String),

    #[error("Pushing to stable track should not specify pre-release version, only major.minor.patch")]
    InvalidStableVersion,

    #[error("Invalid module schema: {0}")]
    InvalidModuleSchema(String),

    #[error("Failed to upload module: {0}")]
    UploadModuleError(String),

    #[error("Failed to zip module: {0}")]
    ZipError(String),
    
    #[error("Other error occurred: {0}")]
    Other(#[from] anyhow::Error),
}