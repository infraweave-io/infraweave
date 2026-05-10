use std::env;

pub fn get_env_var(key: &str) -> String {
    match env::var(key) {
        Ok(val) => val,
        Err(_) => {
            log::error!("Environment variable {} is not set", key);
            std::process::exit(1);
        }
    }
}
