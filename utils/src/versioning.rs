// Zero-pads the major, minor, and patch components of a semantic version.
// This is for making it possible to sort versions lexicographically.
// Preserves additional version information (e.g., pre-release, build metadata).
// Example: "1.2.3-alpha.1" -> "001.002.003-alpha.1"
// Example: "1.2.3" -> "001.002.003"

pub fn zero_pad_semver(ver_str: &str, pad_length: usize) -> Result<String, semver::Error> {
    // Parse the version string
    let version = semver::Version::parse(ver_str)?;

    // Zero-pad the major, minor, and patch components
    let major = format!("{:0width$}", version.major, width = pad_length);
    let minor = format!("{:0width$}", version.minor, width = pad_length);
    let patch = format!("{:0width$}", version.patch, width = pad_length);

    // Reconstruct the version string with zero-padding
    let mut reconstructed = format!("{}.{}.{}", major, minor, patch);

    // Append pre-release and build metadata if present
    if !version.pre.is_empty() {
        reconstructed.push_str(&format!("-{}", &version.pre));
    }
    if !&version.build.is_empty() {
        reconstructed.push_str(&format!("+{}", &version.build));
    }

    Ok(reconstructed)
}

pub fn get_version_track(ver_str: &str) -> Result<String, semver::Error> {
    let version = semver::Version::parse(ver_str)?;
    if version.pre.to_string().is_empty() {
        Ok("stable".to_string())
    } else {
        Ok(version.pre.to_string())
    }
}

pub fn semver_parse(ver_str: &str) -> Result<semver::Version, semver::Error> {
    semver::Version::parse(ver_str)
}

pub fn semver_parse_without_build(ver_str: &str) -> Result<semver::Version, semver::Error> {
    let version = semver::Version::parse(ver_str)?;
    Ok(strip_build_metadata(version))
}

fn strip_build_metadata(mut version: semver::Version) -> semver::Version {
    version.build = semver::BuildMetadata::EMPTY;
    version
}


#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_get_track() {
        let track = get_version_track("0.0.36-dev+test.10").unwrap();
        assert_eq!(track, "dev");

        let track = get_version_track("0.0.1-rc").unwrap();
        assert_eq!(track, "rc");

        let track = get_version_track("0.0.47").unwrap();
        assert_eq!(track, "stable");
    }
}