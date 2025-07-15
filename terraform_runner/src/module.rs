use env_common::DeploymentStatusHandler;
use env_defs::{ApiInfraPayload, CloudProvider};
use log::{error, info};
use std::path::Path;

use env_common::{get_module_download_url, interface::GenericCloudHandler};

pub async fn download_module(
    handler: &GenericCloudHandler,
    s3_key: &String,
    destination: &str,
) -> Result<(), anyhow::Error> {
    println!("Downloading module from {}...", s3_key);

    let url = match get_module_download_url(&handler, s3_key).await {
        Ok(url) => url,
        Err(e) => {
            return Err(anyhow::anyhow!("Error: {:?}", e));
        }
    };

    match env_utils::download_zip(&url, Path::new("module.zip")).await {
        Ok(_) => {
            println!("Downloaded module");
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Error: {:?}", e));
        }
    }

    match env_utils::unzip_file(Path::new("module.zip"), Path::new(destination)) {
        Ok(_) => {
            println!("Unzipped module");
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Error: {:?}", e));
        }
    }
    Ok(())
}

pub async fn get_module(
    handler: &GenericCloudHandler,
    payload: &ApiInfraPayload,
    status_handler: &mut DeploymentStatusHandler<'_>,
) -> Result<env_defs::ModuleResp, anyhow::Error> {
    let track = payload.module_track.clone();
    match handler
        .get_module_version(&payload.module, &track, &payload.module_version)
        .await
    {
        Ok(module) => {
            info!("Successfully fetched module: {:?}", module);
            if module.is_none() {
                let error_text = "Module does not exist";
                println!("{}", error_text);
                let status = "failed_init".to_string();
                status_handler.set_status(status);
                status_handler.set_event_duration();
                status_handler.set_error_text(error_text.to_string());
                status_handler.send_event(&handler).await;
                status_handler.send_deployment(&handler).await;
                Err(anyhow::anyhow!("Module does not exist"))
            } else {
                let module = module.unwrap(); // Improve this
                Ok(module)
            }
        }
        Err(e) => {
            error!("Failed to get module: {:?}", e);
            let status = "failed_init".to_string();
            let error_text: String = e.to_string();
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.set_error_text(error_text);
            status_handler.send_event(&handler).await;
            status_handler.send_deployment(&handler).await;
            Err(anyhow::anyhow!("Failed to get module"))
        }
    }
}
