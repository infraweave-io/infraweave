use base64::engine::general_purpose::STANDARD as base64;
use base64::Engine;
use env_defs::ModuleResp;
use std::collections::BTreeMap;

use oci_client::{
    client::{Client, Config, ImageLayer},
    secrets::RegistryAuth,
    Reference,
};
use serde_json;

#[derive(Clone)]
pub struct OCIRegistryProvider {
    pub registry: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl OCIRegistryProvider {
    pub fn new(registry: String, username: Option<String>, password: Option<String>) -> Self {
        OCIRegistryProvider {
            registry,
            username,
            password,
        }
    }

    pub async fn upload_module(
        &self,
        module: &ModuleResp,
        zip_base64: &String,
    ) -> anyhow::Result<(), anyhow::Error> {
        let (client, auth) = self.get_client_auth();
        let full_path = format!(
            "{}:{}",
            self.registry,
            format!("{}-{}", module.module, module.version.replace("+", "-"))
        );
        println!("Pushing to: {}", full_path);
        let reference: Reference = full_path.parse().unwrap();

        let mut ann = BTreeMap::new();
        ann.insert(
            "io.infraweave.module.name".to_string(),
            serde_json::to_string(&module.module)?,
        );
        ann.insert(
            "io.infraweave.module.version".to_string(),
            serde_json::to_string(&module.version)?,
        );
        ann.insert(
            "io.infraweave.module.manifest".to_string(),
            serde_json::to_string(&module)?,
        );
        let zip_bytes = base64.decode(zip_base64)?;

        let zip_layer = ImageLayer::new(
            zip_bytes.clone(),
            "application/vnd.infraweave.module.v1.zip".to_string(),
            None,
        );

        let diff_id = env_utils::get_diff_id_from_zip(&zip_bytes)?;

        let module_json = serde_json::to_value(&module)?;
        let mut cfg_map = serde_json::Map::new();
        cfg_map.insert("module".to_string(), module_json);
        cfg_map.insert(
            "rootfs".to_string(),
            serde_json::json!({ "type": "layers", "diff_ids": [diff_id] }),
        );
        cfg_map.insert("history".to_string(), serde_json::json!([]));
        let cfg_val = serde_json::Value::Object(cfg_map);
        let cfg_data = serde_json::to_vec(&cfg_val)?;
        let config = Config::oci_v1(cfg_data, Some(ann));
        client
            .push(&reference, &[zip_layer], config, &auth, None)
            .await?;

        let manifest_digest = client.fetch_manifest_digest(&reference, &auth).await?;
        println!("Pushed artifact digest: {}", manifest_digest);

        // Store information that can easily be retrieved later in a CI/CD pipeline
        let path_file_digest = format!("/tmp/infraweave_oci_digest");
        std::fs::write(&path_file_digest, &manifest_digest).map_err(|e| {
            anyhow::anyhow!("Failed to write digest to file {}: {}", manifest_digest, e)
        })?;
        println!("✓ Stored oci artifact digest in: {}", manifest_digest);

        let path_file = format!("/tmp/infraweave_oci_url");
        std::fs::write(&path_file, &full_path)
            .map_err(|e| anyhow::anyhow!("Failed to write url to file {}: {}", path_file, e))?;
        println!("✓ Stored oci artifact url in: {}", path_file);

        Ok(())
    }

    pub async fn get_oci(&self, oci_path: &str) -> anyhow::Result<ModuleResp, anyhow::Error> {
        let (client, auth) = self.get_client_auth();

        let oci_path = self.registry.clone() + "/" + oci_path;

        println!("Pulling from: {}", oci_path);
        let reference: Reference = oci_path.parse().unwrap();

        let artifact = client.pull(&reference, &auth, vec![]).await?;

        let config_bytes = &artifact.config.data;
        let config = serde_json::from_slice::<serde_json::Value>(config_bytes)?;
        let module: ModuleResp = serde_json::from_value(config["module"].clone())?;

        println!("Extracted module: {:?}", module);

        let zip_bytes = &artifact.layers[0].data;
        let base64_zip = base64.encode(zip_bytes);
        println!("Base64 zip: {}", base64_zip);

        Ok(module.clone())
    }

    fn get_client_auth(&self) -> (Client, RegistryAuth) {
        let protocol = if std::env::var("OCI_REGISTRY_ALLOW_HTTP").is_ok() {
            oci_client::client::ClientProtocol::Http
        } else {
            oci_client::client::ClientProtocol::Https
        };

        let config = oci_client::client::ClientConfig {
            protocol,
            ..Default::default()
        };
        let client = Client::new(config);
        let auth = match &self.username {
            None => RegistryAuth::Anonymous,
            Some(username) => {
                if self.password.is_none() || self.password.as_ref().unwrap().is_empty() {
                    panic!("Password is required for authenticated push");
                }
                RegistryAuth::Basic(username.clone(), self.password.clone().unwrap())
            }
        };
        (client, auth)
    }
}
