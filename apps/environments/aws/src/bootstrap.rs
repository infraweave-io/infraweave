use anyhow::{anyhow, Result};
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::types::{
    BucketLocationConstraint, BucketVersioningStatus, CreateBucketConfiguration,
    VersioningConfiguration,
};
use aws_sdk_ssm::Client;
use log::{debug, error, info};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;
use zip::ZipArchive;

use rand::{distributions::Alphanumeric, Rng};

pub async fn bootstrap_environment(region: &String, local: bool) -> Result<(), anyhow::Error> {
    if local {
        info!("Bootstrapping local, region: {}", region);

        if !check_if_bucket_exists(region).await.unwrap() {
            create_bootstrap_bucket(region).await.unwrap();
        }

        run_terraform_locally("apply", "global", region).await?;
    } else {
        info!("Bootstrapping remote environment");
    }

    Ok(())
}

pub async fn bootstrap_teardown_environment(
    region: &String,
    local: bool,
) -> Result<(), anyhow::Error> {
    if local {
        info!("Boostrap teardown local, region: {}", region);
        run_terraform_locally("destroy", "global", region).await?;

        info!("Remove bootstrap bucket");
        delete_bootstrap_bucket(region).await.unwrap();
    } else {
        info!("Boostrapping remote, region: {}", region);
    }
    Ok(())
}

async fn run_terraform_locally(
    command: &str,
    version: &str,
    region: &str,
) -> Result<(), anyhow::Error> {
    let url = format!(
        "https://temporary-tf-release-bucket-jui5345.s3.eu-central-1.amazonaws.com/public/{}.zip",
        version,
    );
    let temp_dir = tempdir()?;
    let zip_path = temp_dir.path().join("file.zip");

    download_zip(&url, &zip_path).await?;

    info!("Downloaded zip file to {:?}", zip_path);

    unzip_file(&zip_path, temp_dir.path())?;

    let key = get_bootstrap_bucket_key(region);
    let bootstrap_bucket_name = read_parameter(&key).await?;

    info!(
        "Bootstrapping Terraform with state-bucket: {}",
        bootstrap_bucket_name
    );

    let init_output = Command::new("terraform")
        .arg("init")
        .arg("-no-color")
        .arg("-input=false")
        .arg(format!("-backend-config=bucket={}", bootstrap_bucket_name))
        .arg("-backend-config=key=terraform.tfstate")
        .arg(format!("-backend-config=region={}", region))
        .current_dir(temp_dir.path())
        .output()?;

    let init_output_str = String::from_utf8(init_output.stdout)?;

    print!("Terraform init output: {}", init_output_str);

    if !std::process::ExitStatus::success(&init_output.status) {
        let init_error_str = String::from_utf8(init_output.stderr)?;
        info!("Terraform init failed: {}", init_error_str);
        return Err(anyhow!("Terraform init failed"));
    }

    // let init_output = Command::new("terraform")
    //     .arg("apply")
    //     .arg("-no-color")
    //     .arg("-input=false")
    //     .arg("-auto-approve")
    //     // .arg(format!("-var=region={}", region))
    //     .current_dir(temp_dir.path())
    //     .output()?;

    // let init_output_str = String::from_utf8(init_output.stdout)?;

    // println!("Terraform apply output: {}", init_output_str);

    // if !std::process::ExitStatus::success(&init_output.status) {
    //     let init_error_str = String::from_utf8(init_output.stderr)?;
    //     println!("Terraform apply failed: {}", init_error_str);
    //     return Err(anyhow!("Terraform apply failed"));
    // }

    let mut child = Command::new("terraform")
        .arg(command)
        .arg("-no-color")
        .arg("-input=false")
        .arg("-auto-approve")
        // .arg(format!("-var=region={}", region))
        .current_dir(temp_dir.path())
        // Additional arguments as needed
        .stdout(std::process::Stdio::piped()) // Capture stdout
        .spawn()?; // Start the command without waiting for it to finish

    // Check if `stdout` was successfully captured
    if let Some(stdout) = child.stdout.take() {
        let reader = std::io::BufReader::new(stdout);

        // Stream each line of output as it's produced
        for line in std::io::BufRead::lines(reader) {
            match line {
                Ok(line) => println!("{}", line), // Print each line to stdout
                Err(e) => error!("Error reading line: {}", e),
            }
        }
    }

    // Wait for the command to finish
    let _ = child.wait()?;

    store_parameter(
        &get_bootstrap_version_key(region),
        version,
        &format!(
            "Version of the bootstrap state bucket for region {}",
            region
        ),
    )
    .await?;

    Ok(())
}

fn get_bootstrap_bucket_key(region: &str) -> String {
    format!("bootstrap_bucket_name-{}", region).to_string()
}

fn get_bootstrap_version_key(region: &str) -> String {
    format!("bootstrap_version-{}", region).to_string()
}

async fn download_zip(url: &str, path: &Path) -> Result<(), anyhow::Error> {
    let response = reqwest::get(url).await?.bytes().await?;
    let mut file = File::create(path)?;
    file.write_all(&response)?;
    Ok(())
}

fn unzip_file(zip_path: &Path, extract_path: &Path) -> Result<(), anyhow::Error> {
    let zip_file = File::open(zip_path)?;
    let mut zip = ZipArchive::new(zip_file)?;

    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let outpath = extract_path.join(file.sanitized_name());

        if (&*file.name()).ends_with('/') {
            std::fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    std::fs::create_dir_all(&p)?;
                }
            }
            let mut outfile = File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }
    Ok(())
}

async fn delete_bootstrap_bucket(region: &str) -> Result<(), anyhow::Error> {
    let key = get_bootstrap_bucket_key(region);
    let bootstrap_bucket_name = read_parameter(&key).await?;
    // Load AWS configuration and create an S3 client
    let region_provider = RegionProviderChain::default_provider().or_else("eu-central-1");
    let config = aws_config::from_env().region(region_provider).load().await;
    let s3_client = aws_sdk_s3::Client::new(&config);

    s3_client
        .delete_object()
        .bucket(&bootstrap_bucket_name)
        .key("terraform.tfstate")
        .send()
        .await?;

    // TODO: Delete all objects in the bucket before trying to delete it
    // Since the bucket is versioned, we need to delete all versions of the object first
    // Currently below will fail if the bucket is not empty
    if let Err(_) = s3_client
        .delete_bucket()
        .bucket(bootstrap_bucket_name)
        .send()
        .await
    {
        error!("Failed to delete the bucket");
        error!("This not yet implemented. Please delete the bucket manually")
    }

    remove_parameter(&key).await?;
    remove_parameter(&get_bootstrap_version_key(region)).await?;
    Ok(())
}

async fn check_if_bucket_exists(region: &str) -> Result<bool, anyhow::Error> {
    let key = get_bootstrap_bucket_key(region);
    let bootstrap_bucket_name = match read_parameter(&key).await {
        Ok(bucket_name) => bucket_name,
        Err(_) => return Ok(false),
    };
    // Load AWS configuration and create an S3 client
    let region_provider = RegionProviderChain::default_provider().or_else("eu-central-1");
    let config = aws_config::from_env().region(region_provider).load().await;
    let s3_client = aws_sdk_s3::Client::new(&config);

    match s3_client
        .head_bucket()
        .bucket(&bootstrap_bucket_name)
        .send()
        .await
    {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

async fn create_bootstrap_bucket(region: &str) -> Result<(), anyhow::Error> {
    let key = get_bootstrap_bucket_key(region);

    let bootstrap_bucket_name = match read_parameter(&key).await {
        Ok(bucket_name) => bucket_name,
        Err(e) => {
            debug!("Did not find parameter {}: {}", &key, e);
            info!("Creating parameter: {}", key);
            let random_suffix = generate_random_alphanumeric(8);
            let bootstrap_bucket_name =
                format!("bootstrap-terraform-state-{}-{}", region, random_suffix);
            let description = format!(
                "Bucket to store Terraform state for bootstrapping environment for region {}",
                region
            );
            store_parameter(&key, &bootstrap_bucket_name, &description).await?;
            bootstrap_bucket_name
        }
    };

    // Load AWS configuration and create an S3 client
    let region_provider = RegionProviderChain::default_provider().or_else("eu-central-1");
    let config = aws_config::from_env().region(region_provider).load().await;
    let s3_client = aws_sdk_s3::Client::new(&config);

    let constraint = BucketLocationConstraint::from(region);
    let cfg = CreateBucketConfiguration::builder()
        .location_constraint(constraint)
        .build();

    match s3_client
        .create_bucket()
        .create_bucket_configuration(cfg)
        .bucket(&bootstrap_bucket_name)
        .send()
        .await
    {
        Ok(_) => info!("Bucket created: {}", bootstrap_bucket_name),
        Err(e) => {
            remove_parameter(&key).await?;
            return Err(anyhow!("Error creating bucket: {}", e));
        }
    }

    let versioning_configuration = VersioningConfiguration::builder()
        .status(BucketVersioningStatus::Enabled)
        .build();

    s3_client
        .put_bucket_versioning()
        .bucket(&bootstrap_bucket_name)
        .versioning_configuration(versioning_configuration)
        .send()
        .await?;
    Ok(())
}

async fn read_parameter(key: &str) -> Result<String, anyhow::Error> {
    // Load the AWS configuration and create an SSM client
    let region_provider = RegionProviderChain::default_provider().or_else("eu-central-1");
    let config = aws_config::from_env().region(region_provider).load().await;
    let client = Client::new(&config);

    match client
        .get_parameter()
        .name(key)
        .with_decryption(false)
        .send()
        .await
    {
        Ok(resp) => {
            if let Some(param) = resp.parameter() {
                info!(
                    "Parameter found: {}, value={}",
                    key,
                    param.value().unwrap_or_default()
                );
                Ok(param.value().unwrap_or_default().to_string())
            } else {
                Err(anyhow!("Parameter found but value missing"))
            }
        }
        Err(e) => Err(anyhow!("Parameter {} not found: {}", key, e)),
    }
}

async fn store_parameter(key: &str, value: &str, description: &str) -> Result<bool, anyhow::Error> {
    // Load the AWS configuration and create an SSM client
    let region_provider = RegionProviderChain::default_provider().or_else("eu-central-1");
    let config = aws_config::from_env().region(region_provider).load().await;
    let client = Client::new(&config);

    info!("Parameter {key} does not exist");
    info!("Creating parameter={} with value={}", key, value);
    let resp = client
        .put_parameter()
        .overwrite(true)
        .r#type(aws_sdk_ssm::types::ParameterType::String)
        .name(key)
        .value(value)
        .description(description)
        .send()
        .await?;

    info!("Success! Parameter now has version: {}", resp.version());
    Ok(true)
}

async fn remove_parameter(key: &str) -> Result<bool, anyhow::Error> {
    // Load the AWS configuration and create an SSM client
    let region_provider = RegionProviderChain::default_provider().or_else("eu-central-1");
    let config = aws_config::from_env().region(region_provider).load().await;
    let client = Client::new(&config);

    client.delete_parameter().name(key).send().await?;

    info!("Success! Parameter {} was deleted", key);
    Ok(true)
}

// TODO: move to common utils
fn generate_random_alphanumeric(len: usize) -> String {
    let rng = rand::thread_rng();
    let alpha: String = rng
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect();
    alpha.to_lowercase()
}
