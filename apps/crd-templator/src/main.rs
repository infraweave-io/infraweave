mod def;
mod generate;
mod read;

use read::read_module_from_file;
use generate::generate_crd_from_module;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let module = read_module_from_file("/tmp/s3.yaml").await?;
    let crd_manifest = generate_crd_from_module(&module)?;
    println!("Generated CRD Manifest:\n{}", crd_manifest);

    Ok(())
}
