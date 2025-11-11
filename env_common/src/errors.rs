use thiserror::Error;

#[derive(Error, Debug)]
pub enum ModuleError {
    #[error("Module version {0} already exists: {1}")]
    ModuleVersionExists(String, String),

    #[error("Module track {0} must be one of 'stable', 'rc', 'beta', 'alpha', 'dev'")]
    InvalidTrack(String),

    #[error("Track '{0}' must match the pre-release version '{1}', and be one of the allowed tracks: 'rc', 'beta', 'alpha', 'dev', 'stable'.\nE.g. if you publish to the 'dev' track it must match the format MAJOR.MINOR.PATCH-dev[+BUILD]")]
    InvalidTrackPrereleaseVersion(String, String),

    #[error(
        "Pushing to stable track should not specify pre-release version, only major.minor.patch"
    )]
    InvalidStableVersion,

    #[error("Invalid module schema: {0}")]
    InvalidModuleSchema(String),

    #[error("The lockfile should not be present in the re-usable module directory, it is instead generated everytime a module is published. Please remove it and try again")]
    TerraformLockfileExists(),

    #[error("The lockfile is missing")]
    TerraformNoLockfile(anyhow::Error),

    #[error(".terraform.lock.hcl file exists but is empty.")]
    TerraformLockfileEmpty,

    #[error("Failed to upload module: {0}")]
    UploadModuleError(String),

    #[error("Failed to zip module: {0}")]
    ZipError(String),

    #[error("Module example has invalid variable: {0}")]
    InvalidExampleVariable(String),

    #[error("Stack validation error: {0}")]
    ValidationError(String),

    #[error("Module version is not set: {0}")]
    ModuleVersionNotSet(String),

    #[error("Namespace should not be set for deployment claim inside Stack: {0}")]
    StackModuleNamespaceIsSet(String),

    #[error("In the claim for \"{0}\", variable \"{1}\" is set to {2}, however no output/variable named \"{3}\" could be found for \"{4}\"")]
    OutputKeyNotFound(String, String, String, String, String),

    #[error("The source claim \"{0}\" has an invalid reference variable \"{3}\" in claim \"{2}\" with kind \"{1}\"")]
    StackClaimReferenceNotFound(String, String, String, String),

    #[error("There is a duplicate claim name \"{0}\" in the stack")]
    DuplicateClaimNames(String),

    #[error("There is a circular dependency in the stack between: {0:?}")]
    CircularDependency(Vec<String>),

    #[error("The stack claim \"{1}\" of kind \"{0}\" has an invalid reference \"{2}\" to itself")]
    SelfReferencingClaim(String, String, String),

    #[error("The manifest \"{0}\" is missing the \"version\" field")]
    ModuleVersionMissing(String),

    #[error("The module version \"{0}\" for \"{1}\" could not be found")]
    ModuleVersionNotFound(String, String),

    #[error("No providers defined in the module manifest for \"{0}\"")]
    NoProvidersDefined(String),

    #[error("No terraform required_providers defined in a '.tf' file for \"{0}\"")]
    NoRequiredProvidersDefined(String),

    #[error("Invalid variable naming: {0}")]
    InvalidVariableNaming(String),

    #[error("Reference \"{0}\" could not be resolved using key \"{1}\"")]
    UnresolvedReference(String, String),

    #[error("Other error occurred: {0}")]
    Other(#[from] anyhow::Error),
}
