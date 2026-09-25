use std::fs::remove_file;

use env_defs::{CloudProvider, ExtraData};
use env_utils::{
    create_temp_dir, download_zip, get_extra_environment_variables_all, store_backend_file,
    store_tf_vars_json, unzip_file,
};

use crate::current_region_handler;

pub async fn handle_setup_workspace(deployment_id: &str, environment_id: &str) {
    let (deployment, _) = current_region_handler()
        .await
        .get_deployment_and_dependents(deployment_id, environment_id, false)
        .await
        .unwrap();

    if deployment.is_none() {
        println!("Deployment not found");
        std::process::exit(1);
    }
    let deployment = deployment.unwrap();

    let is_stack = deployment.module_type == "stack";

    println!(
        "Setting up manual workspace for {} deployment {} in environment {}",
        if is_stack { "stack" } else { "module" },
        deployment_id,
        environment_id
    );

    let module = if is_stack {
        current_region_handler()
            .await
            .get_stack_version(
                &deployment.module,
                &deployment.module_track,
                &deployment.module_version,
            )
            .await
            .unwrap()
    } else {
        current_region_handler()
            .await
            .get_module_version(
                &deployment.module,
                &deployment.module_track,
                &deployment.module_version,
            )
            .await
            .unwrap()
    };

    if module.is_none() {
        eprintln!(
            "{} version not found",
            if is_stack { "Stack" } else { "Module" }
        );
        std::process::exit(1);
    }
    let module = module.unwrap();

    let handler = current_region_handler().await;
    let url = match env_common::get_modules_download_url(&handler, &module.s3_key).await {
        Ok(url) => url,
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    };

    let new_dir = create_temp_dir().unwrap();
    let zip_path = new_dir.join("temp-module.zip");
    download_zip(&url, &zip_path).await.unwrap();
    unzip_file(&zip_path, &new_dir).unwrap();
    remove_file(&zip_path).unwrap();

    let extra_vars_values = get_extra_environment_variables_all(
        &deployment.deployment_id,
        environment_id,
        &deployment.reference,
        &deployment.module_version,
        &deployment.module_type,
        &deployment.module_track,
        &deployment.drift_detection,
        &ExtraData::None,
    );

    // Filter out git-related variables
    let extra_vars: std::collections::HashMap<String, String> = extra_vars_values
        .into_iter()
        .filter(|(k, _)| !k.to_lowercase().contains("git"))
        .collect();

    let mut all_variables = deployment.variables.clone();
    if let serde_json::Value::Object(ref mut map) = all_variables {
        if let serde_json::Value::Object(extra_map) = serde_json::json!(extra_vars) {
            map.extend(extra_map);
        }
    }

    store_tf_vars_json(&all_variables, new_dir.to_str().unwrap());
    store_backend_file(
        handler.get_backend_provider(),
        new_dir.to_str().unwrap(),
        &handler
            .get_backend_provider_arguments(environment_id, deployment_id)
            .await,
    )
    .await;

    println!(
        "Manual workspace setup complete. Downloaded and unzipped module to {}",
        new_dir.display()
    );

    println!("\nStart by running \n\npushd {}\n", new_dir.display());
    println!("Then run initialize the workspace with \n\ntofu init\n");
    println!("Finally, to apply the changes run \n\ntofu apply\n");
    println!("\nHint: Remember to `popd` when you're done to exit the workspace.");
}

pub async fn handle_get_state(deployment_id: &str, environment_id: &str, output: Option<&str>) {
    let handler = current_region_handler().await;

    let (deployment, _) = handler
        .get_deployment_and_dependents(deployment_id, environment_id, false)
        .await
        .unwrap();

    if deployment.is_none() {
        eprintln!("Deployment not found: {}", deployment_id);
        std::process::exit(1);
    }

    println!(
        "Downloading state file for deployment {} in environment {}",
        deployment_id, environment_id
    );

    if let Err(e) = handler
        .download_state_file(environment_id, deployment_id, output.map(|s| s.to_string()))
        .await
    {
        eprintln!("Failed to download state file: {:?}", e);
        std::process::exit(1);
    }
}
