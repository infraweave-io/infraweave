use std::sync::OnceLock;

static PACKAGE: OnceLock<String> = OnceLock::new();
static USER_AGENT: OnceLock<String> = OnceLock::new();

/// Names the package that is making HTTP calls, for the `User-Agent` header.
///
/// Only needed where the executable name says nothing useful, such as a Lambda
/// (`bootstrap`) or the Python extension (`python3`). Must run before the first
/// call to [`http_user_agent`]; later calls are ignored.
pub fn set_http_user_agent_package(package: &str) {
    let _ = PACKAGE.set(package.to_string());
}

/// `User-Agent` for outgoing HTTP requests, e.g.
/// `infraweave/1.2.3 (cli; macos; aarch64)`.
///
/// The package defaults to the running executable's name, since the calling
/// crates here are libraries and `CARGO_PKG_NAME` would only ever name them.
pub fn http_user_agent() -> &'static str {
    USER_AGENT.get_or_init(|| {
        let package = PACKAGE.get().cloned().unwrap_or_else(executable_name);
        format!(
            "infraweave/{} ({}; {}; {})",
            env!("CARGO_PKG_VERSION"),
            package,
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })
}

fn executable_name() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_includes_version_os_and_arch() {
        let ua = http_user_agent();
        assert!(ua.starts_with(concat!("infraweave/", env!("CARGO_PKG_VERSION"), " (")));
        assert!(ua.ends_with(&format!(
            "; {}; {})",
            std::env::consts::OS,
            std::env::consts::ARCH
        )));
    }
}
