use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct Blob {
    pub digest: String,
    pub content: Vec<u8>,
}

#[derive(Deserialize)]
pub struct OciManifest {
    #[serde(default, rename = "mediaType")]
    #[allow(dead_code)]
    pub media_type: Option<String>,
    #[allow(dead_code)]
    pub config: serde_json::Value,
    pub layers: Vec<LayerDesc>,
}

#[derive(Deserialize)]
pub struct LayerDesc {
    #[serde(rename = "mediaType")]
    #[allow(dead_code)]
    pub media_type: String,
    #[allow(dead_code)]
    pub size: u64,
    pub digest: String,
}
#[derive(Serialize, Deserialize)]
pub struct IndexEntry {
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub digest: String,
    pub size: u64,
}
#[derive(Serialize)]
pub struct IndexJson {
    #[serde(rename = "schemaVersion")]
    pub schema_version: i32,
    pub manifests: Vec<IndexEntry>,
}
#[derive(Serialize)]
pub struct LayoutFile {
    #[serde(rename = "imageLayoutVersion")]
    pub image_layout_version: &'static str,
}

/// Structure to hold separate OCI artifacts for offline verification
#[derive(Debug, Serialize, Deserialize)]
pub struct OciArtifactSet {
    pub artifact_path: String,
    pub attestation_path: Option<String>,
    pub signature_path: Option<String>,
    pub digest: String,
}
