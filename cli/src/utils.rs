use env_common::interface::GenericCloudHandler;

pub fn get_environment(environment_arg: &str) -> String {
    if !environment_arg.contains('/') {
        format!("cli/{}", environment_arg)
    } else {
        environment_arg.to_string()
    }
}

pub async fn current_region_handler() -> GenericCloudHandler {
    GenericCloudHandler::default().await
}
