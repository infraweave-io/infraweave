

pub fn get_epoch() -> u128 {
    std::time::UNIX_EPOCH.elapsed().unwrap().as_millis()
}

pub fn get_timestamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}