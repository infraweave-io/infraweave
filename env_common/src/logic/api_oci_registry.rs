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
    pub username: String,
    pub password: String,
}

impl OCIRegistryProvider {
    pub fn new(registry: String, username: String, password: String) -> Self {
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
        let client = Client::default();
        let auth = RegistryAuth::Basic(self.username.clone(), self.password.clone());
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
        let zip_bytes = base64::decode(zip_base64)?;
        let tar_bytes = env_utils::zip_bytes_to_targz(&zip_bytes);
        let diff_id = env_utils::get_diff_id(&tar_bytes)?;

        let tar_layer = ImageLayer::new(
            tar_bytes,
            "application/vnd.oci.image.layer.v1.tar+gzip".to_string(),
            None,
        );

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
            .push(&reference, &[tar_layer], config, &auth, None)
            .await?;

        let manifest_digest = client.fetch_manifest_digest(&reference, &auth).await?;
        println!("Pushed artifact digest: {}", manifest_digest);
        Ok(())
    }

    pub async fn get_oci(&self, oci_path: &str) -> anyhow::Result<ModuleResp, anyhow::Error> {
        let client = Client::default();
        let auth = RegistryAuth::Basic(self.username.clone(), self.password.clone());

        let oci_path = self.registry.clone() + "/" + oci_path;

        println!("Pulling from: {}", oci_path);
        let reference: Reference = oci_path.parse().unwrap();

        // after upload…
        let artifact = client
            .pull(
                &reference,
                &auth,
                vec![
                    "application/vnd.oci.image.config.v1+json",
                    "application/vnd.oci.image.layer.v1.tar+gzip",
                ],
            )
            .await?;

        let config_bytes = &artifact.config.data;
        let config = serde_json::from_slice::<serde_json::Value>(config_bytes)?;
        let module: ModuleResp = serde_json::from_value(config["module"].clone())?;

        println!("Extracted module: {:?}", module);

        let tar_bytes = &artifact.layers[0].data;
        let zip_bytes = env_utils::targz_to_zip_bytes(tar_bytes);
        let base64_zip = base64::encode(zip_bytes);
        println!("Base64 zip: {}", base64_zip);

        // let zip_data = env_utils::targz_to_zip_bytes(tar_bytes);
        // let file_name = format!("{}-{}.zip", module.module, module.version);
        // let output_path = std::path::Path::new("downloads").join(&file_name);
        // std::fs::create_dir_all("downloads")?;
        // std::fs::write(&output_path, &zip_data)?;
        // println!("Stored module archive at {:?}", output_path);

        Ok(module.clone())
    }
}
