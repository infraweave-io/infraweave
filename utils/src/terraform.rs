use env_defs::TfLockProvider;

pub fn get_provider_url_key(
    tf_lock_provider: &TfLockProvider,
    target: &str,
    category: &str,
) -> (String, String) {
    let parts: Vec<&str> = tf_lock_provider.source.split('/').collect();
    // parts: ["registry.terraform.io", "hashicorp", "aws"]
    let namespace = parts[1];
    let provider = parts[2];

    let prefix = format!(
        "terraform-provider-{provider}_{version}",
        provider = provider,
        version = tf_lock_provider.version
    );
    let file = match category {
        // "index_json" => format!("index.json"),
        "provider_binary" => format!("{prefix}_{target}.zip"),
        "shasum" => format!("{prefix}_SHA256SUMS"),
        "signature" => format!("{prefix}_SHA256SUMS.72D7468F.sig"), // New Hashicorp signature after incident HCSEC-2021-12 (v0.15.1 and later)
        _ => panic!("Invalid category"),
    };

    let download_url = format!(
        "https://releases.hashicorp.com/terraform-provider-{provider}/{version}/{file}",
        version = tf_lock_provider.version,
    );
    let key = format!("registry.terraform.io/{namespace}/{provider}/{file}",);
    (download_url, key)
}
