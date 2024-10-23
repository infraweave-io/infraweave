use crd_templator::generate_crd_from_module;
use env_defs::ModuleManifest;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::api::PatchParams;
use kube::{
    api::{Api, ApiResource, DynamicObject, GroupVersionKind, Patch},
    Client,
};
use log::{error, warn};


pub async fn apply_module_crd(
    client: Client,
    manifest: &ModuleManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    let kind = manifest.spec.module_name.clone();
    let manifest_yaml = serde_yaml::to_string(&manifest).expect("Failed to serialize to YAML");
    warn!("Module {} has yaml manifest:\n{}", kind, manifest_yaml);
    let crd_manifest = match generate_crd_from_module(&manifest) {
        Ok(crd) => crd,
        Err(e) => {
            error!("Failed to generate CRD: {}", e);
            return Err(e.into());
        }
    };
    warn!("Generated CRD Manifest for {}: {}", kind, crd_manifest);

    let api: Api<CustomResourceDefinition> = Api::all(client.clone());

    let crd_json = serde_yaml::from_str::<serde_json::Value>(&crd_manifest)?;

    println!("CRD YAML to be applied for module {}: \n{:#?}", kind, crd_json);

    // Use the name from the CRD object
    let name = crd_json["metadata"]["name"]
        .as_str()
        .ok_or("CRD missing metadata.name")?;

    // Use the JSON string with Patch::Apply
    let patch = Patch::Apply(crd_json.clone());
    let pp = PatchParams::apply("infraweave-operator").force();

    // Execute the patch (apply) operation
    match api.patch(name, &pp, &patch).await {
        Ok(_) => warn!("Successfully applied CRD for: {}", name),
        Err(e) => {
            error!("Failed to apply CRD: {}", e);
            return Err(e.into());
        }
    }

    Ok(())
}
