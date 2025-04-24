use anyhow::Context;
use log::info;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::Cursor;
use std::io::Read;
use std::io::Write;
use std::io::{self};
use std::path::Path;
use std::path::PathBuf;
use walkdir::WalkDir;
use zip::write::FileOptions;
use zip::ZipArchive;
use zip::ZipWriter;

const ONE_MB: u64 = 1_048_576; // 1MB in bytes

pub async fn get_zip_file(directory: &Path, manifest_yaml_path: &PathBuf) -> io::Result<Vec<u8>> {
    if !manifest_yaml_path.exists() {
        println!("Manifest yaml file does not exist in the specified directory");
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Manifest yaml not found",
        ));
    }
    let mut buffer = Vec::new();
    let mut total_size: u64 = 0;

    let bypass_file_size_check =
        std::env::var("BYPASS_FILE_SIZE_CHECK").unwrap_or("false".to_string()) != "true";

    {
        let cursor = Cursor::new(&mut buffer);
        let mut zip = zip::ZipWriter::new(cursor);

        let options = FileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(0o755);

        let walker = WalkDir::new(directory)
            .into_iter()
            .filter_entry(|e| !is_terraform_dir(e));

        for entry in walker {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path != manifest_yaml_path {
                let name = path.strip_prefix(directory).unwrap().to_str().unwrap();
                zip.start_file(name, options)?;
                let mut f = File::open(path)?;
                let bytes_copied = io::copy(&mut f, &mut zip)?;

                total_size += bytes_copied;

                if bypass_file_size_check && total_size > ONE_MB {
                    println!("Module directory exceeds 1MB, aborting.\nThis typically is a sign of unwanted files in the module directory, text files should not be this large. Please remove files and retry.\n\nIf you have large files and need to publish in your module, you can by pass this check by setting the environment variable BYPASS_FILE_SIZE_CHECK to true");
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "ZIP file exceeds 1MB limit",
                    ));
                }
            }
        }
        zip.finish()?;
    }

    Ok(buffer)
}

fn is_terraform_dir(entry: &walkdir::DirEntry) -> bool {
    entry.file_type().is_dir() && entry.file_name() == ".terraform"
}

pub fn get_zip_file_from_str(file_content: &str, file_name: &str) -> io::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    {
        // Scope to close the zip writer when done
        let cursor = Cursor::new(&mut buffer);
        let mut zip = zip::ZipWriter::new(cursor);

        let options = FileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(0o755);

        zip.start_file(file_name, options)?;
        zip.write_all(file_content.as_bytes())?;
        zip.finish()?;
    }

    Ok(buffer)
}

pub enum ZipInput {
    WithFolders(HashMap<String, Vec<u8>>),
    WithoutFolders(Vec<Vec<u8>>),
}

pub fn merge_zips(input: ZipInput) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut buffer = Cursor::new(Vec::new());

    {
        // Scope to close the zip writer when done
        let mut zip_writer = ZipWriter::new(&mut buffer);

        let options = FileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(0o755);

        match input {
            ZipInput::WithFolders(zip_files) => {
                // Iterate over each folder and corresponding zip file
                for (folder, zip_file_data) in zip_files {
                    let mut zip_archive = ZipArchive::new(Cursor::new(zip_file_data))?;

                    // Iterate over the files inside each zip
                    for i in 0..zip_archive.len() {
                        let mut file = zip_archive.by_index(i)?;
                        let file_name = file.name().to_string();

                        // If the folder is "./" or empty string, don't prepend anything
                        let new_file_name = if folder == "./" || folder.is_empty() {
                            file_name
                        } else {
                            format!("{}/{}", folder, file_name)
                        };

                        // Read the file contents
                        let mut file_contents = Vec::new();
                        file.read_to_end(&mut file_contents)?;

                        // Add the file to the new zip
                        zip_writer.start_file(new_file_name, options)?;
                        zip_writer.write_all(&file_contents)?;
                    }
                }
            }

            ZipInput::WithoutFolders(zip_files) => {
                // Iterate over each zip file in the Vec
                for zip_file_data in zip_files {
                    let mut zip_archive = ZipArchive::new(Cursor::new(zip_file_data))?;

                    // Iterate over the files inside each zip
                    for i in 0..zip_archive.len() {
                        let mut file = zip_archive.by_index(i)?;
                        let file_name = file.name().to_string();

                        // Since it's a Vec input, put everything in the root
                        let new_file_name = file_name;

                        // Read the file contents
                        let mut file_contents = Vec::new();
                        file.read_to_end(&mut file_contents)?;

                        // Add the file to the new zip
                        zip_writer.start_file(new_file_name, options)?;
                        zip_writer.write_all(&file_contents)?;
                    }
                }
            }
        }

        zip_writer.finish()?;
    }

    Ok(buffer.into_inner())
}

pub async fn download_zip(url: &str, path: &Path) -> Result<(), anyhow::Error> {
    info!("Downloading ZIP file from {url} to {}", path.display());
    let resp = reqwest::get(url)
        .await
        .with_context(|| format!("request to {url} failed"))?;

    if let Err(err) = resp.error_for_status_ref() {
        if err.status() == Some(reqwest::StatusCode::NOT_FOUND) {
            return Err(anyhow::anyhow!("remote object does not exist (404)"));
        }
        return Err(err).context("server returned an error status");
    }

    let bytes = resp.bytes().await.context("failed reading body")?;

    let mut file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("failed writing to {}", path.display()))?;

    Ok(())
}

pub async fn download_zip_to_vec(url: &str) -> Result<Vec<u8>, anyhow::Error> {
    info!("Downloading zip file from {} to vec", url);
    let response = reqwest::get(url).await?.bytes().await?;
    Ok(response.to_vec())
}

pub fn unzip_file(zip_path: &Path, extract_path: &Path) -> Result<(), anyhow::Error> {
    let zip_file = File::open(zip_path)?;
    let mut zip = ZipArchive::new(zip_file)?;

    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let outpath = extract_path.join(file.mangled_name());

        if (file.name()).ends_with('/') {
            std::fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    std::fs::create_dir_all(p)?;
                }
            }
            let mut outfile = File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }
    Ok(())
}

/// Reads all .tf files in a given directory and concatenates their contents.
pub fn read_tf_directory(directory: &Path) -> io::Result<String> {
    let mut combined_contents = String::new();

    for entry in WalkDir::new(directory)
        .max_depth(1)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_type().is_file() && e.path().extension().map_or(false, |ext| ext == "tf")
        })
    {
        let content = fs::read_to_string(entry.path())?;
        combined_contents.push_str(&content);
        combined_contents.push('\n');
    }

    Ok(combined_contents)
}

/// Reads all .tf files in a in-memory zip-file and concatenates their contents.
pub fn read_tf_from_zip(zip_data: &[u8]) -> io::Result<String> {
    let cursor = Cursor::new(zip_data);
    let mut zip = ZipArchive::new(cursor).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Failed to read ZIP archive: {}", e),
        )
    })?;

    let mut combined_contents = String::new();

    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to access file in ZIP: {}", e),
            )
        })?;

        // Skip directories
        if file.is_dir() {
            continue;
        }

        // Check for ".tf" extension
        let path = Path::new(file.name());
        if path.extension().and_then(|s| s.to_str()) == Some("tf") {
            let mut content = String::new();
            file.read_to_string(&mut content).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Failed to read file {}: {}", file.name(), e),
                )
            })?;
            combined_contents.push_str(&content);
            combined_contents.push('\n');
        }
    }

    Ok(combined_contents)
}

pub fn contains_terraform_lockfile(zip_data: &[u8]) -> Result<String, anyhow::Error> {
    let cursor = Cursor::new(zip_data);
    let mut zip = ZipArchive::new(cursor)?;

    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let file_path = Path::new(file.name());

        // Check if the file name matches `.terraform.lock.hcl`
        if file_path
            .file_name()
            .map(|name| name == ".terraform.lock.hcl")
            .unwrap_or(false)
        {
            let mut lockfile_content = String::new();
            file.read_to_string(&mut lockfile_content)?;
            return Ok(lockfile_content);
        }
    }
    Err(anyhow::anyhow!("No .terraform.lock.hcl file found"))
}
