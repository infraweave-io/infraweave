use zip::ZipArchive;
use std::fs::File;
use std::io::Write;
use std::io::Cursor;
use std::io::{self};
use std::path::Path;
use std::path::PathBuf;
use walkdir::WalkDir;
use zip::write::FileOptions;

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

        for entry in WalkDir::new(directory) {
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


pub async fn download_zip(url: &str, path: &Path) -> Result<(), anyhow::Error> {
    print!("Downloading zip file from {} to {}", url, path.display());
    let response = reqwest::get(url).await?.bytes().await?;
    let mut file = File::create(path)?;
    file.write_all(&response)?;
    Ok(())
}

pub fn unzip_file(zip_path: &Path, extract_path: &Path) -> Result<(), anyhow::Error> {
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
