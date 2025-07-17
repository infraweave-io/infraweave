use anyhow::{Context, Result};
use base64::Engine;
use env_defs::{
    ArtifactType, Blob, IndexEntry, IndexJson, LayerDesc, LayoutFile, ModuleResp, OciArtifactSet,
    OciManifest,
};
use flate2::{write::GzEncoder, Compression};
use oci_distribution::Reference;
use regorus::Engine as RegoEngine;
use reqwest::{header, Client};
use sha2::Digest;
use std::{
    fs::File,
    io::{Cursor, Read},
    path::Path,
};
use tar::{Builder, EntryType, Header};

use crate::targz_to_zip_bytes;

pub type VerificationConfig = serde_json::Value;

fn header_for(path: &str, size: u64, kind: EntryType) -> Header {
    let mut h = Header::new_gnu();
    h.set_path(path).unwrap();
    h.set_size(size);
    h.set_entry_type(kind);
    h.set_mode(if kind == EntryType::Directory {
        0o755
    } else {
        0o644
    });
    h.set_uid(0);
    h.set_gid(0);
    h.set_mtime(0);
    h.set_cksum();
    h
}

/// Download IMAGE (tag *or* digest) from any OCI registry and save as separate tar.gz files:
/// - Main OCI artifact (manifest + layers)
/// - Attestation file (if found)
/// - Signature file (if found)
pub async fn save_oci_artifacts_separate(
    image: &str,
    token: &str,
    artifact_type: &ArtifactType,
) -> Result<(String, String)> {
    let reference: Reference = image.parse()?;
    let registry = reference.registry();
    let repo = &reference.repository().to_lowercase();
    let tag = reference
        .tag()
        .context("image reference lacks tag/digest")?;

    let (client, def_headers) = create_authenticated_client(registry, repo, tag, token).await?;

    let man_url = format!("https://{}/v2/{}/manifests/{}", registry, repo, tag);
    println!("🔧 Fetching manifest from: {}", man_url);
    println!("🔧 Repository path used: {}", repo);

    let resp = client
        .get(&man_url)
        .headers(def_headers.clone())
        .send()
        .await?;

    let status = resp.status();
    println!("🔧 Manifest response status: {}", status);

    if !status.is_success() {
        if let Ok(error_body) = resp.text().await {
            println!("🔧 Error response body: {}", error_body);
            println!("🔍 Registry: {}", registry);
            println!("🔍 Repository path used: {}", repo);
            println!("🔍 Request URL: {}", man_url);
        }
        anyhow::bail!("Failed to fetch manifest: HTTP {}", status);
    }

    resp.error_for_status_ref()?;

    let docker_digest = resp
        .headers()
        .get("Docker-Content-Digest")
        .and_then(|h| h.to_str().ok())
        .map(ToOwned::to_owned)
        .context("registry did not return Docker-Content-Digest header")?;
    let digest_hex = docker_digest
        .strip_prefix("sha256:")
        .context("digest did not start with sha256:")?;

    let manifest_bytes = resp.bytes().await?;

    /* ---- save main artifact -------------------------------------------- */

    // Use /tmp directory which is writable in serverless environments
    let artifact_path = format!("/tmp/{}.tar.gz", &tag);

    match artifact_type {
        ArtifactType::MainPackage => {
            save_main_artifact(
                &manifest_bytes,
                &docker_digest,
                registry,
                repo,
                &client,
                &def_headers,
                &artifact_path,
            )
            .await?;
        }
        ArtifactType::Attestation => {
            /* ---- fetch and save attestation and signature -------------------- */
            let _attestation_path = fetch_and_save_attestation(
                &client,
                &def_headers,
                registry,
                repo,
                digest_hex,
                &artifact_path,
            )
            .await?;
        }
        ArtifactType::Signature => {
            let _signature_path = fetch_and_save_signature(
                &client,
                &def_headers,
                registry,
                repo,
                digest_hex,
                &artifact_path,
            )
            .await?;
        }
        _ => anyhow::bail!("Unsupported artifact type for saving: {:?}", artifact_type),
    }

    println!("✔ Saved OCI artifacts with digest {}", &docker_digest);

    Ok((docker_digest.clone(), tag.to_string()))
}

/// Create authenticated HTTP client for any OCI registry
async fn create_authenticated_client(
    registry: &str,
    repo: &str,
    _tag: &str,
    token: &str,
) -> Result<(Client, header::HeaderMap)> {
    let client = Client::builder().build()?;
    let mut def_headers = header::HeaderMap::new();

    // Try different authentication methods based on registry
    if registry.contains("ghcr.io") {
        // GitHub Container Registry
        let repo_lower = repo.to_lowercase();
        let token_url = format!(
            "https://ghcr.io/token?service=ghcr.io&scope=repository:{}:pull",
            repo_lower // Always use lowercase for GHCR
        );

        println!("🔧 Requesting GHCR token from: {}", token_url);
        println!("🔧 Using GitHub App token for GHCR authentication");
        println!("🔧 Repository path (lowercase): {}", repo_lower);

        let pat = Some(token.to_string());
        let auth_header = pat.as_deref().map(|p| {
            let basic =
                base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{}", p));
            format!("Basic {}", basic)
        });

        println!("🔧 Auth header prepared (token length: {})", token.len());

        let token_resp = client
            .get(&token_url)
            .optional_header("Authorization", auth_header)
            .send()
            .await?;

        let status = token_resp.status();
        println!("🔧 GHCR token response status: {}", status);

        if !status.is_success() {
            if let Ok(error_body) = token_resp.text().await {
                println!("🔧 GHCR token error response: {}", error_body);
            }
            anyhow::bail!("Failed to get GHCR token: HTTP {}", status);
        }

        let token_json = token_resp.json::<serde_json::Value>().await?;

        let bearer = token_json["token"]
            .as_str()
            .context("token JSON lacked `token` field")?
            .to_owned();

        println!(
            "✅ Successfully obtained GHCR bearer token (length: {})",
            bearer.len()
        );

        def_headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", bearer).parse().unwrap(),
        );
    } else if registry.contains("docker.io") || registry.contains("registry-1.docker.io") {
        // Docker Hub
        let token_url = format!(
            "https://auth.docker.io/token?service=registry.docker.io&scope=repository:{}:pull",
            repo
        );

        let token_resp = client.get(&token_url).send().await?.error_for_status()?;
        let bearer = token_resp.json::<serde_json::Value>().await?["token"]
            .as_str()
            .context("token JSON lacked `token` field")?
            .to_owned();

        def_headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", bearer).parse().unwrap(),
        );
    } else {
        // Generic registry - try without auth first, add auth headers if available
        if let Ok(username) = std::env::var("REGISTRY_USERNAME") {
            if let Ok(password) = std::env::var("REGISTRY_PASSWORD") {
                let basic = base64::engine::general_purpose::STANDARD
                    .encode(format!("{}:{}", username, password));
                def_headers.insert(
                    header::AUTHORIZATION,
                    format!("Basic {}", basic).parse().unwrap(),
                );
            }
        }
    }

    def_headers.insert(
        header::ACCEPT,
        "application/vnd.oci.image.index.v1+json,\
         application/vnd.docker.distribution.manifest.list.v2+json,\
         application/vnd.oci.image.manifest.v1+json"
            .parse()?,
    );

    Ok((client, def_headers))
}

/// Save the main OCI artifact (manifest + all layers) as a tar.gz file
async fn save_main_artifact(
    manifest_bytes: &[u8],
    docker_digest: &str,
    registry: &str,
    repo: &str,
    client: &Client,
    def_headers: &header::HeaderMap,
    output_path: &str,
) -> Result<()> {
    let digest_hex = docker_digest
        .strip_prefix("sha256:")
        .context("digest did not start with sha256:")?;
    let manifest_size = manifest_bytes.len() as u64;

    /* ---- start tar.gz --------------------------------------------------- */
    let enc = GzEncoder::new(File::create(output_path)?, Compression::default());
    let mut tar = Builder::new(enc);

    let layout_bytes = serde_json::to_vec(&LayoutFile {
        image_layout_version: "1.0.0",
    })?;
    tar.append(
        &header_for("oci-layout", layout_bytes.len() as u64, EntryType::Regular),
        Cursor::new(&layout_bytes),
    )?;
    tar.append(
        &header_for("blobs/", 0, EntryType::Directory),
        Cursor::new(&[][..]),
    )?;
    tar.append(
        &header_for("blobs/sha256/", 0, EntryType::Directory),
        Cursor::new(&[][..]),
    )?;

    /* ---- write manifest blob ------------------------------------------- */
    let blob_path = format!("blobs/sha256/{}", digest_hex);
    tar.append(
        &header_for(&blob_path, manifest_size, EntryType::Regular),
        Cursor::new(manifest_bytes),
    )?;

    // Always handle single-platform manifest
    let manifest: OciManifest = serde_json::from_slice(manifest_bytes)?;
    let index_json = IndexJson {
        schema_version: 2,
        manifests: vec![IndexEntry {
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            digest: docker_digest.to_owned(),
            size: manifest_size,
        }],
    };
    let idx_bytes = serde_json::to_vec(&index_json)?;
    let idx_len = idx_bytes.len();
    tar.append(
        &header_for("index.json", idx_len as u64, EntryType::Regular),
        Cursor::new(&idx_bytes),
    )
    .context("Failed to append index.json")?;

    // Download and add the config blob (contains annotations)
    if let Some(config_digest) = manifest.config.get("digest").and_then(|d| d.as_str()) {
        let config_hex = config_digest.strip_prefix("sha256:").unwrap();
        let config_url = format!("https://{}/v2/{}/blobs/{}", registry, repo, config_digest);
        let mut config_headers = def_headers.clone();
        config_headers.insert(header::ACCEPT, "*/*".parse().unwrap());

        println!("🔧 Downloading config blob: {}", config_digest);
        let config_bytes = client
            .get(&config_url)
            .headers(config_headers)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;

        let config_size = config_bytes.len() as u64;
        tar.append(
            &header_for(
                &format!("blobs/sha256/{}", config_hex),
                config_size,
                EntryType::Regular,
            ),
            Cursor::new(&config_bytes),
        )?;
        println!("✓ Added config blob to archive");
    }

    // pull each layer so layout is self-contained
    for layer in &manifest.layers {
        let hex = layer.digest.strip_prefix("sha256:").unwrap();
        let url = format!("https://{}/v2/{}/blobs/{}", registry, repo, layer.digest);
        let mut blob_headers = def_headers.clone();
        blob_headers.insert(header::ACCEPT, "*/*".parse().unwrap());
        let bytes = client
            .get(&url)
            .headers(blob_headers)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        let actual_size = bytes.len() as u64;
        tar.append(
            &header_for(
                &format!("blobs/sha256/{}", hex),
                actual_size,
                EntryType::Regular,
            ),
            Cursor::new(&bytes),
        )?;
    }

    // Finalize tar and gzip
    tar.finish().context("failed to finish tar")?;
    let enc = tar
        .into_inner()
        .context("failed to retrieve encoder after finishing tar")?;
    enc.finish().context("failed to finish gzip encoder")?;

    println!("✓ Saved main artifact to {}", output_path);
    Ok(())
}

/// Fetch and save attestation as a separate tar.gz file
async fn fetch_and_save_attestation(
    client: &Client,
    def_headers: &header::HeaderMap,
    registry: &str,
    repo: &str,
    subject_digest: &str,
    output_path: &str,
) -> Result<Option<String>> {
    if let Some(blob) =
        fetch_attestation_blob(client, def_headers, registry, repo, subject_digest).await?
    {
        save_blob_as_tar(&blob, output_path, "attestation")?;
        println!("✓ Saved attestation to {}", output_path);
        Ok(Some(output_path.to_string()))
    } else {
        println!("ℹ️  No attestation found");
        Ok(None)
    }
}

/// Fetch and save signature as a separate tar.gz file
async fn fetch_and_save_signature(
    client: &Client,
    def_headers: &header::HeaderMap,
    registry: &str,
    repo: &str,
    subject_digest: &str,
    output_path: &str,
) -> Result<Option<String>> {
    if let Some(blob) =
        fetch_signature_blob(client, def_headers, registry, repo, subject_digest).await?
    {
        save_blob_as_tar(&blob, output_path, "signature")?;
        println!("✓ Saved signature to {}", output_path);
        Ok(Some(output_path.to_string()))
    } else {
        println!("ℹ️  No signature found");
        Ok(None)
    }
}

/// Save a blob (attestation or signature) as a tar.gz file
fn save_blob_as_tar(blob: &Blob, output_path: &str, blob_type: &str) -> Result<()> {
    let enc = GzEncoder::new(File::create(output_path)?, Compression::default());
    let mut tar = Builder::new(enc);

    // Save the blob content with metadata
    let filename = format!("{}.json", blob_type);
    tar.append(
        &header_for(&filename, blob.content.len() as u64, EntryType::Regular),
        Cursor::new(&blob.content),
    )?;

    // Save digest as metadata
    let digest_content = blob.digest.as_bytes();
    tar.append(
        &header_for(
            "digest.txt",
            digest_content.len() as u64,
            EntryType::Regular,
        ),
        Cursor::new(digest_content),
    )?;

    tar.finish().context("failed to finish tar")?;
    let enc = tar
        .into_inner()
        .context("failed to retrieve encoder after finishing tar")?;
    enc.finish().context("failed to finish gzip encoder")?;

    Ok(())
}

/// Fetch signature blob from registry
async fn fetch_signature_blob(
    client: &Client,
    def_headers: &header::HeaderMap,
    registry: &str,
    repo: &str,
    subject_digest: &str,
) -> Result<Option<Blob>> {
    // Try cosign signature tag patterns

    let tag_pattern = format!("sha256:{}", subject_digest);

    let manifest_url = format!("https://{}/v2/{}/manifests/{}", registry, repo, tag_pattern);

    let resp = client
        .get(&manifest_url)
        .headers(def_headers.clone())
        .send()
        .await?;

    if resp.status().is_success() {
        let manifest_bytes = resp.bytes().await?;
        let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes)?;

        if let Some(layers) = manifest["layers"].as_array() {
            for layer in layers {
                if let Some(media_type) = layer["mediaType"].as_str() {
                    if media_type.contains("cosign") || media_type.contains("signature") {
                        let layer_digest = layer["digest"].as_str().unwrap();
                        let layer_size = layer["size"].as_u64().unwrap();

                        let blob_url =
                            format!("https://{}/v2/{}/blobs/{}", registry, repo, layer_digest);
                        let mut blob_headers = def_headers.clone();
                        blob_headers.insert(header::ACCEPT, "*/*".parse().unwrap());

                        let bytes = client
                            .get(&blob_url)
                            .headers(blob_headers)
                            .send()
                            .await?
                            .error_for_status()?
                            .bytes()
                            .await?;

                        anyhow::ensure!(
                            bytes.len() as u64 == layer_size,
                            "signature size mismatch"
                        );

                        return Ok(Some(Blob {
                            digest: layer_digest.to_owned(),
                            content: bytes.to_vec(),
                        }));
                    }
                }
            }
        }
    }

    Ok(None)
}

/// Fetch attestation blob (updated from existing function)
async fn fetch_attestation_blob(
    client: &Client,
    def_headers: &header::HeaderMap,
    registry: &str,
    repo: &str,
    subject_digest: &str,
) -> Result<Option<Blob>> {
    let tag_pattern = format!("sha256:{}", subject_digest);

    let manifest_url = format!("https://{}/v2/{}/manifests/{}", registry, repo, tag_pattern);

    let resp = client
        .get(&manifest_url)
        .headers(def_headers.clone())
        .send()
        .await?;

    if resp.status().is_success() {
        let manifest_bytes = resp.bytes().await?;
        let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes)?;

        println!("🔧 Attestation manifest found for tag: {}", tag_pattern);
        println!(
            "🔧 Manifest content: {}",
            serde_json::to_string_pretty(&manifest).unwrap_or_default()
        );

        if let Some(layers) = manifest["layers"].as_array() {
            println!("🔧 Found {} layers in attestation manifest", layers.len());
            for (i, layer) in layers.iter().enumerate() {
                if let Some(media_type) = layer["mediaType"].as_str() {
                    println!("🔧 Layer {}: mediaType = {}", i, media_type);
                    if media_type.contains("dsse.envelope") || media_type.contains("cosign") {
                        let layer_digest = layer["digest"].as_str().unwrap();
                        let layer_size = layer["size"].as_u64().unwrap();

                        let blob_url =
                            format!("https://{}/v2/{}/blobs/{}", registry, repo, layer_digest);
                        let mut blob_headers = def_headers.clone();
                        blob_headers.insert(header::ACCEPT, "*/*".parse().unwrap());

                        let bytes = client
                            .get(&blob_url)
                            .headers(blob_headers)
                            .send()
                            .await?
                            .error_for_status()?
                            .bytes()
                            .await?;

                        anyhow::ensure!(
                            bytes.len() as u64 == layer_size,
                            "attestation size mismatch"
                        );

                        return Ok(Some(Blob {
                            digest: layer_digest.to_owned(),
                            content: bytes.to_vec(),
                        }));
                    }
                } else {
                    println!("🔧 Layer {} has no mediaType field", i);
                }
            }
        } else {
            println!("🔧 No layers found in attestation manifest");
        }
    } else {
        println!(
            "🔧 Failed to fetch attestation manifest for tag: {} (status: {})",
            tag_pattern,
            resp.status()
        );
    }

    println!("🔧 No attestation blob found after checking all patterns");
    Ok(None)
}

/// Verify OCI artifacts offline using previously saved tar.gz files
/// This function works with any OCI registry artifacts and does not require network access
pub fn verify_oci_artifacts_offline(
    artifact_set: &OciArtifactSet,
    config_path: Option<&str>,
) -> Result<()> {
    println!("🔍 Starting offline verification of OCI artifacts...");

    // Load verification configuration
    let config = if let Some(path) = config_path {
        if Path::new(path).exists() {
            let config_content = std::fs::read_to_string(path)?;
            serde_json::from_str(&config_content)?
        } else {
            anyhow::bail!("Configuration file not found: {}", path);
        }
    } else {
        load_verification_config()?
    };

    // 1. Verify main artifact integrity
    verify_main_artifact_offline(
        &format!("{}.tar.gz", artifact_set.tag_main),
        &artifact_set.digest,
    )
    .unwrap();

    // 2. Verify attestation
    verify_attestation_offline(
        &format!("{}.tar.gz", artifact_set.tag_attestation.as_ref().unwrap()),
        &artifact_set.digest,
        &config,
    )?;

    // 3. Verify signature
    verify_signature_offline(
        &format!("{}.tar.gz", artifact_set.tag_signature.as_ref().unwrap()),
        &artifact_set.digest,
        &config,
    )?;

    println!("✓ Offline verification completed successfully");
    Ok(())
}

/// Verify main artifact integrity from tar.gz file
fn verify_main_artifact_offline(artifact_path: &str, expected_digest: &str) -> Result<()> {
    println!(
        "🔍 Verifying main artifact integrity from {}",
        artifact_path
    );

    if !Path::new(artifact_path).exists() {
        anyhow::bail!("Artifact file not found: {}", artifact_path);
    }

    let tar_file = File::open(artifact_path)?;
    let decoder = flate2::read::GzDecoder::new(tar_file);
    let mut archive = tar::Archive::new(decoder);

    let mut manifest_bytes: Option<Vec<u8>> = None;
    let mut layer_count = 0;
    let mut config_digest: Option<String> = None;

    for entry_result in archive.entries()? {
        let mut entry = entry_result?;
        let path = entry.path()?.to_path_buf();
        let path_str = path.to_string_lossy();

        if path_str == "index.json" {
            let mut contents = Vec::new();
            entry.read_to_end(&mut contents)?;
            let index: serde_json::Value = serde_json::from_slice(&contents)?;
            println!("✓ Found index.json");

            // Verify the manifest digest in index matches expected
            if let Some(manifests) = index["manifests"].as_array() {
                if let Some(manifest) = manifests.first() {
                    if let Some(digest) = manifest["digest"].as_str() {
                        if digest != expected_digest {
                            anyhow::bail!("Manifest digest in index.json ({}) doesn't match expected digest ({})", digest, expected_digest);
                        }
                        println!("✓ Manifest digest in index.json matches expected digest");
                    }
                }
            }
        } else if path_str.starts_with("blobs/sha256/") && !path_str.ends_with('/') {
            let mut contents = Vec::new();
            entry.read_to_end(&mut contents)?;
            let filename = path.file_name().unwrap().to_string_lossy();

            // Check if this is the manifest blob
            let expected_hex = expected_digest
                .strip_prefix("sha256:")
                .unwrap_or(expected_digest);
            if filename == expected_hex {
                manifest_bytes = Some(contents.clone());
                println!("✓ Found manifest blob");

                // Parse manifest to get config digest
                let manifest: OciManifest = serde_json::from_slice(&contents)?;
                if let Some(config_dig) = manifest.config.get("digest").and_then(|d| d.as_str()) {
                    config_digest = Some(
                        config_dig
                            .strip_prefix("sha256:")
                            .unwrap_or(config_dig)
                            .to_string(),
                    );
                }

                // Verify manifest digest
                let computed_digest = sha2::Sha256::digest(&contents);
                let computed_hex = format!("{:x}", computed_digest);
                if computed_hex != expected_hex {
                    anyhow::bail!(
                        "Manifest digest mismatch: expected {}, computed {}",
                        expected_hex,
                        computed_hex
                    );
                }
                println!("✓ Manifest digest verified");
            } else {
                // Check if this is the config blob (don't count as layer)
                if let Some(ref config_hex) = config_digest {
                    if filename == *config_hex {
                        println!("✓ Found and verified config blob");
                        // Verify config digest
                        let computed_digest = sha2::Sha256::digest(&contents);
                        let computed_hex = format!("{:x}", computed_digest);
                        if computed_hex != filename {
                            anyhow::bail!(
                                "Config digest mismatch for {}: computed {}",
                                filename,
                                computed_hex
                            );
                        }
                        continue; // Skip counting this as a layer
                    }
                }

                // This is a layer blob
                layer_count += 1;
                // Verify layer digest
                let computed_digest = sha2::Sha256::digest(&contents);
                let computed_hex = format!("{:x}", computed_digest);
                if computed_hex != filename {
                    anyhow::bail!(
                        "Layer digest mismatch for {}: computed {}",
                        filename,
                        computed_hex
                    );
                }
            }
        }
    }

    if let Some(manifest_data) = manifest_bytes {
        let manifest: OciManifest = serde_json::from_slice(&manifest_data)?;
        println!(
            "✓ Found {} layers in manifest, verified {} layer files",
            manifest.layers.len(),
            layer_count
        );

        if manifest.layers.len() != layer_count {
            anyhow::bail!(
                "Layer count mismatch: manifest declares {} layers but found {} layer files",
                manifest.layers.len(),
                layer_count
            );
        }

        // Perform additional integrity checks
        verify_oci_artifact_integrity(&manifest_data, &manifest.layers, expected_digest)?;
    } else {
        anyhow::bail!("Manifest blob not found in artifact");
    }

    println!("✓ Main artifact integrity verification completed");
    Ok(())
}

/// Verify attestation from tar.gz file
fn verify_attestation_offline(
    attestation_path: &str,
    subject_digest: &str,
    _config: &VerificationConfig,
) -> Result<()> {
    println!("🔍 Verifying attestation from {}", attestation_path);

    let tar_file = File::open(attestation_path)?;
    let decoder = flate2::read::GzDecoder::new(tar_file);
    let mut archive = tar::Archive::new(decoder);

    let mut attestation_content: Option<Vec<u8>> = None;
    let mut stored_digest: Option<String> = None;

    for entry_result in archive.entries()? {
        let mut entry = entry_result?;
        let path = entry.path()?.to_path_buf();
        let path_str = path.to_string_lossy();

        if path_str == "attestation.json" {
            let mut contents = Vec::new();
            entry.read_to_end(&mut contents)?;
            attestation_content = Some(contents);
            println!("✓ Found attestation content");
        } else if path_str == "digest.txt" {
            let mut contents = Vec::new();
            entry.read_to_end(&mut contents)?;
            stored_digest = Some(String::from_utf8(contents)?);
            println!("✓ Found stored digest");
        }
    }

    if let (Some(content), Some(digest)) = (attestation_content, stored_digest) {
        let blob = Blob { digest, content };

        // Use existing verification function
        let subject_hex = subject_digest
            .strip_prefix("sha256:")
            .unwrap_or(subject_digest);
        verify_slsa_provenance_attestation(&blob, subject_hex)?;
        println!("✓ Attestation verification completed");
    } else {
        anyhow::bail!("Incomplete attestation data in archive");
    }

    Ok(())
}

/// Verify signature from tar.gz file
fn verify_signature_offline(
    signature_path: &str,
    subject_digest: &str,
    _config: &VerificationConfig,
) -> Result<()> {
    println!("🔍 Verifying signature from {}", signature_path);

    let tar_file = File::open(signature_path)?;
    let decoder = flate2::read::GzDecoder::new(tar_file);
    let mut archive = tar::Archive::new(decoder);

    let mut signature_content: Option<Vec<u8>> = None;
    let mut stored_digest: Option<String> = None;

    for entry_result in archive.entries()? {
        let mut entry = entry_result?;
        let path = entry.path()?.to_path_buf();
        let path_str = path.to_string_lossy();

        if path_str == "signature.json" {
            let mut contents = Vec::new();
            entry.read_to_end(&mut contents)?;
            signature_content = Some(contents);
            println!("✓ Found signature content");
        } else if path_str == "digest.txt" {
            let mut contents = Vec::new();
            entry.read_to_end(&mut contents)?;
            stored_digest = Some(String::from_utf8(contents)?);
            println!("✓ Found stored digest");
        }
    }

    if let (Some(content), Some(_digest)) = (signature_content, stored_digest) {
        // Basic signature validation
        if content.is_empty() {
            anyhow::bail!("Signature content is empty");
        }

        // Try to parse as JSON (cosign format)
        if let Ok(signature_json) = serde_json::from_slice::<serde_json::Value>(&content) {
            println!("✓ Signature is valid JSON");

            // Verify basic cosign signature structure
            if let Some(critical) = signature_json.get("critical") {
                if let Some(image) = critical.get("image") {
                    if let Some(docker_manifest_digest) = image.get("docker-manifest-digest") {
                        let sig_digest = docker_manifest_digest.as_str().unwrap_or("");
                        if sig_digest.contains(subject_digest) {
                            println!("✓ Signature references correct image digest");
                        } else {
                            anyhow::bail!(
                                "Signature references incorrect image digest: {} vs {}",
                                sig_digest,
                                subject_digest
                            );
                        }
                    }
                }
            }
        } else {
            println!("ℹ️  Signature is in binary format");
            if content.len() < 32 {
                println!(
                    "⚠️  Warning: Signature is unusually small ({} bytes)",
                    content.len()
                );
            }
        }

        println!("✓ Signature verification completed");
    } else {
        anyhow::bail!("Incomplete signature data in archive");
    }

    Ok(())
}

/// Load verification configuration from environment variable or use defaults
fn load_verification_config() -> Result<VerificationConfig> {
    // Default policy content

    // Default complete configuration JSON
    let default_config_str = std::env::var("ATTESTATION_POLICY").unwrap();
    let default_config = serde_json::from_str(&default_config_str).unwrap();

    Ok(default_config)
}

/// Verifies SLSA provenance attestation content and subject matching
fn verify_slsa_provenance_attestation(blob: &Blob, subject_digest: &str) -> Result<()> {
    // Remove commented debug code and clean up verify_attestation function
    println!(
        "🔍 Verifying attestation blob {} for subject {}",
        blob.digest, subject_digest
    );

    // Parse the DSSE envelope from the blob content
    let dsse_envelope: serde_json::Value =
        serde_json::from_slice(&blob.content).context("Failed to parse DSSE envelope JSON")?;

    // Extract the payload from the DSSE envelope
    if let Some(payload_b64) = dsse_envelope["payload"].as_str() {
        let payload_bytes = base64::engine::general_purpose::STANDARD
            .decode(payload_b64)
            .context("Failed to decode base64 payload")?;

        let payload_str =
            String::from_utf8(payload_bytes).context("Failed to convert payload to UTF-8")?;

        // Parse the payload as JSON to extract subject information
        let payload: serde_json::Value =
            serde_json::from_str(&payload_str).context("Failed to parse payload JSON")?;

        // Check if the subject matches our image digest
        if let Some(subject) = payload["subject"].as_array() {
            for subj in subject {
                if let Some(digest) = subj["digest"]["sha256"].as_str() {
                    if digest == subject_digest {
                        println!("✓ Subject digest matches: {}", digest);

                        // Additional verification: check the predicate type
                        if let Some(predicate_type) = payload["predicateType"].as_str() {
                            println!("✓ Predicate type: {}", predicate_type);

                            // Verify this is a supported SLSA provenance attestation
                            if predicate_type.contains("slsa.dev/provenance") {
                                let version = if predicate_type.contains("/v1") {
                                    "v1.0+"
                                } else if predicate_type.contains("/v0.2")
                                    || predicate_type == "slsa.dev/provenance"
                                {
                                    "v0.2"
                                } else {
                                    "unknown"
                                };
                                println!("✓ SLSA provenance version: {}", version);

                                let config = load_verification_config()?;

                                println!("🔍 Extracting SLSA provenance information...");

                                if config["policy_content"].as_str().is_some() {
                                    println!(
                                        "Using policy-based verification with embedded policy"
                                    );
                                    verify_with_policy(&payload, &config)?;
                                    println!("✓ Attestation verification passed!");
                                } else {
                                    println!("No policy content provided, not performing policy-based verification");
                                }

                                return Ok(());
                            } else {
                                anyhow::bail!("Unsupported predicate type: {}", predicate_type);
                            }
                        } else {
                            anyhow::bail!("Missing predicateType in attestation");
                        }
                    }
                }
            }
            anyhow::bail!(
                "No matching subject found in attestation for digest: {}",
                subject_digest
            );
        } else {
            anyhow::bail!("No subject found in attestation payload");
        }
    } else {
        anyhow::bail!("No payload found in DSSE envelope");
    }
}

/* ----------- tiny extension trait for optional header -------------------- */
trait OptHeader {
    fn optional_header(self, k: &'static str, v: Option<String>) -> Self;
}
impl OptHeader for reqwest::RequestBuilder {
    fn optional_header(mut self, k: &'static str, v: Option<String>) -> Self {
        if let Some(v) = v {
            self = self.header(k, v);
        }
        self
    }
}

/// Verifies attestation using a Rego policy from environment variable or default
pub fn verify_with_policy(
    payload: &serde_json::Value,
    config: &VerificationConfig,
) -> anyhow::Result<()> {
    // Get policy content from config (which now includes it from environment variable)
    let policy_content = config["policy_content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No policy content found in configuration"))?;

    println!(
        "📋 Using Rego policy from environment variable ({} characters)",
        policy_content.len()
    );

    // Create Rego engine
    let mut engine = RegoEngine::new();

    // Add the policy
    engine
        .add_policy(
            "verification_policy".to_string(),
            policy_content.to_string(),
        )
        .map_err(|e| anyhow::anyhow!("Failed to load policy: {}", e))?;

    // Create input data for the policy
    let input = create_policy_input(payload, config)?;

    // Convert JSON to Rego value
    let input_value = json_to_rego_value(&input)?;

    // Set input data
    engine.set_input(input_value);

    // println!("🔍 Debugging policy input data:");
    // println!("   → Input JSON structure: {}", serde_json::to_string_pretty(&input).unwrap_or_else(|_| "Failed to serialize".to_string()));

    // Debug specific fields the policy is looking for
    if let Some(attestation) = input.get("attestation") {
        println!(
            "   → Attestation predicateType: {:?}",
            attestation.get("predicateType")
        );
        if let Some(predicate) = attestation.get("predicate") {
            if let Some(invocation) = predicate.get("invocation") {
                if let Some(config_source) = invocation.get("configSource") {
                    println!("   → configSource.uri: {:?}", config_source.get("uri"));
                } else {
                    println!("   → No configSource found in invocation");
                }
            } else {
                println!("   → No invocation found in predicate");
            }
        } else {
            println!("   → No predicate found in attestation");
        }
    }

    // Evaluate the main policy
    let results = engine
        .eval_query("data.verification.allow".to_string(), false)
        .map_err(|e| anyhow::anyhow!("Failed to evaluate policy: {}", e))?;

    // Check if the policy allows the attestation
    if let Some(result) = results.result.first() {
        if let Some(expressions) = result.expressions.first() {
            if let regorus::Value::Bool(allowed) = &expressions.value {
                if *allowed {
                    println!("✓ Policy verification passed");
                    return Ok(());
                } else {
                    println!("❌ Policy verification failed - main allow rule returned false");
                    anyhow::bail!("Policy verification failed: all conditions must be met for verification to pass");
                }
            }
        }
    }

    anyhow::bail!("Policy evaluation returned no result or invalid result type")
}

/// Creates minimal input data for the Rego policy - passes raw attestation data and config
fn create_policy_input(
    payload: &serde_json::Value,
    config: &VerificationConfig,
) -> anyhow::Result<serde_json::Value> {
    let mut input = serde_json::Map::new();

    // Convert VerificationConfig to JSON and add as config data
    let config_json = serde_json::to_value(config)?;
    input.insert("config".to_string(), config_json);

    // Pass the entire raw attestation payload - let the policy decide what to extract
    input.insert("attestation".to_string(), payload.clone());

    Ok(serde_json::Value::Object(input))
}

/// Converts a JSON value to a Rego value using the correct regorus API
fn json_to_rego_value(json: &serde_json::Value) -> anyhow::Result<regorus::Value> {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    match json {
        serde_json::Value::Null => Ok(regorus::Value::Null),
        serde_json::Value::Bool(b) => Ok(regorus::Value::Bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                // Create regorus Number from f64
                Ok(regorus::Value::Number(f.into()))
            } else {
                anyhow::bail!("Invalid number in JSON")
            }
        }
        serde_json::Value::String(s) => Ok(regorus::Value::String(Arc::from(s.as_str()))),
        serde_json::Value::Array(arr) => {
            let mut rego_arr = Vec::new();
            for item in arr {
                rego_arr.push(json_to_rego_value(item)?);
            }
            Ok(regorus::Value::Array(Arc::new(rego_arr)))
        }
        serde_json::Value::Object(obj) => {
            let mut rego_obj = BTreeMap::new();
            for (key, value) in obj {
                let rego_key = regorus::Value::String(Arc::from(key.as_str()));
                rego_obj.insert(rego_key, json_to_rego_value(value)?);
            }
            Ok(regorus::Value::Object(Arc::new(rego_obj)))
        }
    }
}

/// Verifies OCI artifact integrity by checking layer digests and manifest integrity
pub fn verify_oci_artifact_integrity(
    manifest_bytes: &[u8],
    layers: &[LayerDesc],
    subject_digest: &str,
) -> Result<()> {
    println!("🔍 Verifying OCI artifact integrity...");

    // 1. Verify manifest digest matches subject digest
    let computed_digest = sha2::Sha256::digest(manifest_bytes);
    let computed_hex = format!("sha256:{:x}", computed_digest);

    if subject_digest != computed_hex {
        anyhow::bail!(
            "Manifest digest mismatch: expected {}, computed {}",
            subject_digest,
            computed_hex
        );
    }
    println!("✓ Manifest digest verified");

    // 2. Verify each layer has valid digest format
    for (i, layer) in layers.iter().enumerate() {
        // Check digest format
        if !layer.digest.starts_with("sha256:") {
            anyhow::bail!("Layer {} has invalid digest format: {}", i, layer.digest);
        }

        let digest_hex = layer.digest.strip_prefix("sha256:").unwrap();
        if digest_hex.len() != 64 {
            anyhow::bail!(
                "Layer {} has invalid digest length: {}",
                i,
                digest_hex.len()
            );
        }

        // Verify hex encoding
        if !digest_hex.chars().all(|c| c.is_ascii_hexdigit()) {
            anyhow::bail!("Layer {} digest contains invalid hex characters", i);
        }

        println!("✓ Layer {} digest format valid: {}", i, layer.digest);
    }

    // 3. Check for suspicious layer patterns
    let mut layer_sizes: Vec<u64> = layers.iter().map(|l| l.size).collect();
    layer_sizes.sort_unstable();

    // Count duplicate layer sizes (could indicate layer reuse, but only concerning if excessive)
    let mut duplicate_count = 0;
    for window in layer_sizes.windows(2) {
        if window[0] == window[1] && window[0] > 1024 {
            // Only count non-tiny layers
            duplicate_count += 1;
        }
    }

    if duplicate_count > (layers.len() / 2) {
        println!("⚠️  Warning: High number of duplicate layer sizes ({}) - may indicate inefficient layering or potential security concern", duplicate_count);
    } else if duplicate_count > 0 {
        println!("ℹ️  Found {} layers with duplicate sizes - this is typically normal for similar layer operations", duplicate_count);
    }

    // 4. Check for empty layers (only warn if there are many, as some are legitimate)
    let empty_layers: Vec<usize> = layers
        .iter()
        .enumerate()
        .filter(|(_, layer)| layer.size == 0)
        .map(|(i, _)| i)
        .collect();

    if empty_layers.len() > 3 {
        println!("⚠️  Warning: Found {} empty layers - excessive empty layers may indicate build inefficiency or potential security issue", 
                empty_layers.len());
    } else if !empty_layers.is_empty() {
        println!("ℹ️  Found {} empty layers (indices: {:?}) - this is typically normal for container images", 
                empty_layers.len(), empty_layers);
    }

    println!("✓ OCI artifact integrity verification completed");
    Ok(())
}
struct OciArtifactData {
    blobs: std::collections::HashMap<String, Vec<u8>>,
    manifest: serde_json::Value,
}

fn parse_oci_artifact(oci_path: &str) -> Result<OciArtifactData> {
    let tar_file = File::open(oci_path).context("Failed to open OCI tar.gz file")?;
    let decoder = flate2::read::GzDecoder::new(tar_file);
    let mut archive = tar::Archive::new(decoder);

    let mut index_json: Option<serde_json::Value> = None;
    let mut blobs = std::collections::HashMap::new();

    for entry_result in archive.entries()? {
        let mut entry = entry_result?;
        let path = entry.path()?.to_path_buf();
        let path_str = path.to_string_lossy();

        if path_str == "index.json" {
            let mut contents = Vec::new();
            entry.read_to_end(&mut contents)?;
            index_json = Some(serde_json::from_slice(&contents)?);
        } else if path_str.starts_with("blobs/sha256/") && !path_str.ends_with('/') {
            let mut contents = Vec::new();
            entry.read_to_end(&mut contents)?;
            let filename = path
                .file_name()
                .context("Failed to get blob filename")?
                .to_string_lossy()
                .to_string();
            blobs.insert(filename, contents);
        }
    }

    let index = index_json.context("index.json not found in OCI artifact")?;
    let manifest_digest = index["manifests"][0]["digest"]
        .as_str()
        .context("No manifest digest found")?;
    let manifest_hex = manifest_digest
        .strip_prefix("sha256:")
        .context("Invalid manifest digest format")?;
    let manifest_content = blobs.get(manifest_hex).context("Manifest blob not found")?;
    let manifest: serde_json::Value = serde_json::from_slice(manifest_content)?;

    Ok(OciArtifactData { blobs, manifest })
}

pub fn get_module_zip_from_oci_targz(oci_path: &str) -> Result<Vec<u8>> {
    println!("🔍 Extracting ZIP bytes from OCI tar.gz: {}", oci_path);

    let artifact = parse_oci_artifact(oci_path)?;
    let layers = artifact.manifest["layers"]
        .as_array()
        .context("No layers found in manifest")?;
    if layers.is_empty() {
        anyhow::bail!("No layers found in manifest");
    }

    let layer = &layers[0];
    let layer_digest = layer["digest"].as_str().context("Layer digest not found")?;
    let layer_media_type = layer["mediaType"]
        .as_str()
        .context("Layer media type not found")?;
    let layer_hex = layer_digest
        .strip_prefix("sha256:")
        .context("Invalid layer digest format")?;
    let layer_content = artifact
        .blobs
        .get(layer_hex)
        .context("Layer blob not found")?;

    if layer_media_type.contains("infraweave.module") && layer_media_type.contains("zip") {
        println!(
            "✓ Found ZIP content directly ({} bytes)",
            layer_content.len()
        );
        Ok(layer_content.clone())
    } else if layer_media_type.contains("tar+gzip") || layer_media_type.contains("tar.gz") {
        println!(
            "🔄 Converting legacy tar.gz format to ZIP ({} bytes)",
            layer_content.len()
        );
        let zip_bytes = targz_to_zip_bytes(layer_content);
        println!("✓ Converted to ZIP ({} bytes)", zip_bytes.len());
        Ok(zip_bytes)
    } else {
        anyhow::bail!("Unsupported layer media type: {}", layer_media_type);
    }
}

pub fn get_module_manifest_from_oci_targz(oci_path: &str) -> Result<ModuleResp> {
    println!(
        "🔍 Extracting module manifest from OCI tar.gz: {}",
        oci_path
    );

    let artifact = parse_oci_artifact(oci_path)?;
    let config_digest = artifact.manifest["config"]["digest"]
        .as_str()
        .context("No config digest found")?;
    let config_hex = config_digest
        .strip_prefix("sha256:")
        .context("Invalid config digest format")?;
    let config_content = artifact
        .blobs
        .get(config_hex)
        .context("Config blob not found")?;
    let config: serde_json::Value = serde_json::from_slice(config_content)?;
    let module_value = config
        .get("module")
        .context("No 'module' field found in config blob")?;

    serde_json::from_value(module_value.clone()).context("Failed to deserialize module from config")
}
