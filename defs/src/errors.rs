use thiserror::Error;

#[derive(Error, Debug)]
pub enum CloudHandlerError {
    #[error("Currently there is no available runner to handle the reqeust")]
    NoAvailableRunner(),

    #[error("You are not authenticated to make this request: {0}")]
    Unauthenticated(String),

    #[error("The response is missing a payload")]
    MissingPayload(),

    #[error("An error occured when serving the request: {0}")]
    OtherError(String),

    #[error("Please make sure to set the platform environment, for example: \"export INFRAWEAVE_ENV=dev\"")]
    MissingEnvironment(),

    #[error("A job for this deployment is already in progress: {0}")]
    JobAlreadyInProgress(String),
}
