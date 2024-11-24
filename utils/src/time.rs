pub fn get_epoch() -> u128 {
    std::time::UNIX_EPOCH.elapsed().unwrap().as_millis()
}

pub fn get_timestamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn epoch_to_timestamp(epoch: u128) -> String {
    // Convert milliseconds to seconds and nanoseconds separately
    let seconds = (epoch / 1000) as i64;
    let nanoseconds = ((epoch % 1000) * 1_000_000) as u32;

    // Create a DateTime<Utc> from the seconds and nanoseconds
    let datetime = chrono::TimeZone::timestamp_opt(&chrono::Utc, seconds, nanoseconds).unwrap();

    // Format to RFC 3339 with milliseconds
    datetime.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_epoch_to_timestamp() {
        let epoch = 1617000000000;
        let expected = "2021-03-29T06:40:00.000Z";
        assert_eq!(epoch_to_timestamp(epoch), expected);
    }
}
