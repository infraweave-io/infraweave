use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct Blob {
    pub digest: String,
    pub content: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ArtifactType {
    MainPackage,
    Attestation,
    Signature,
    Unknown,
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
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OciArtifactSet {
    pub oci_artifact_path: String,
    pub digest: String,
    #[serde(default)]
    pub tag_main: String,
    #[serde(default)]
    pub tag_signature: Option<String>,
    #[serde(default)]
    pub tag_attestation: Option<String>,
}
