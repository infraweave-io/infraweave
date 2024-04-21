use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::Cursor;
use std::io::{self, ErrorKind};
use std::path::Path;
use walkdir::WalkDir;
use zip::write::FileOptions;

pub async fn get_module_zip_file(directory: &Path) -> io::Result<Vec<u8>> {
    let module_yaml_path = directory.join("module.yaml");
    if !module_yaml_path.exists() {
        println!("module.yaml does not exist in the specified directory");
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "module.yaml not found",
        ));
    }

    let mut buffer = Vec::new();

    {
        let cursor = Cursor::new(&mut buffer);
        let mut zip = zip::ZipWriter::new(cursor);

        let options = FileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(0o755);

        for entry in WalkDir::new(directory) {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path != module_yaml_path {
                let name = path.strip_prefix(directory).unwrap().to_str().unwrap();
                zip.start_file(name, options)?;
                let mut f = File::open(path)?;
                io::copy(&mut f, &mut zip)?;
            }
        }
        zip.finish()?;
    }

    Ok(buffer)
}

/// Reads all .tf files in a given directory and concatenates their contents.
fn read_tf_directory(directory: &Path) -> io::Result<String> {
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

pub fn get_variables_from_tf_files(
    directory_path: &Path,
) -> Result<Vec<env_defs::Variable>, String> {
    let contents = read_tf_directory(directory_path).unwrap();

    let hcl_body = hcl::parse(&contents)
        .map_err(|_| io::Error::new(ErrorKind::InvalidData, "Failed to parse HCL content"))
        .unwrap();

    let mut variables = Vec::new();

    for block in hcl_body.blocks() {
        if block.identifier() == "variable" {
            let mut attrs = HashMap::new();

            for attribute in block.body().attributes() {
                match attribute.expr.clone().into() {
                    hcl::Expression::String(s) => {
                        attrs.insert(attribute.key(), s);
                    }
                    hcl::Expression::Variable(v) => {
                        attrs.insert(attribute.key(), v.to_string());
                    }
                    hcl::Expression::Bool(b) => {
                        attrs.insert(attribute.key(), b.to_string());
                    }
                    // TODO: Add support for other types to support validation parameter
                    unimplemented_expression => {
                        return Err(format!(
                            "Error while parsing HCL, type not yet supported: {:?} for identifier {:?}, attribute: {:?}",
                            unimplemented_expression, attribute.key(), attribute.expr()
                        ))
                    }
                }
            }
            if block.labels().len() != 1 {
                panic!(
                    "Expected exactly one label for variable block, found: {:?}",
                    block.labels()
                );
            }
            let variable_name = block.labels().get(0).unwrap().as_str().to_string();

            let variable = env_defs::Variable {
                name: variable_name,
                _type: attrs.get("type").unwrap_or(&"".to_string()).to_string(),
                default: attrs.get("default").unwrap_or(&"".to_string()).to_string(),
                description: attrs
                    .get("description")
                    .unwrap_or(&"".to_string())
                    .to_string(),
                nullable: attrs.get("nullable").unwrap_or(&"true".to_string()) == "true",
                sensitive: attrs.get("sensitive").unwrap_or(&"false".to_string()) == "true",
                // validation: block
                //     .nested_block("validation")
                //     .map(|vb| env_defs::Validation {
                //         expression: vb
                //             .attributes()
                //             .get("expression")
                //             .and_then(|v| v.as_str())
                //             .unwrap_or_default()
                //             .to_string(),
                //         message: vb
                //             .attributes()
                //             .get("message")
                //             .and_then(|v| v.as_str())
                //             .unwrap_or_default()
                //             .to_string(),
                //     })
                //     .unwrap_or(env_defs::Validation {
                //         expression: String::new(),
                //         message: String::new(),
                //     }),
            };

            variables.push(variable);
        }
    }
    // log::info!("variables: {:?}", serde_json::to_string(&variables));
    Ok(variables)
}

pub fn get_outputs_from_tf_files(directory_path: &Path) -> Result<Vec<env_defs::Output>, String> {
    let contents = read_tf_directory(directory_path).unwrap();

    let hcl_body = hcl::parse(&contents)
        .map_err(|_| io::Error::new(ErrorKind::InvalidData, "Failed to parse HCL content"))
        .unwrap();

    let mut outputs = Vec::new();

    for block in hcl_body.blocks() {
        if block.identifier() == "output" {
            let mut attrs = HashMap::new();

            for attribute in block.body().attributes() {
                match attribute.expr.clone().into() {
                    hcl::Expression::String(s) => {
                        attrs.insert(attribute.key(), s);
                    }
                    hcl::Expression::Variable(v) => {
                        attrs.insert(attribute.key(), v.to_string());
                    }
                    hcl::Expression::Bool(b) => {
                        attrs.insert(attribute.key(), b.to_string());
                    }
                    // TODO: Add support for other types to support validation parameter
                    // https://docs.rs/hcl-rs/latest/hcl/enum.Expression.html
                    unimplemented_expression => {
                        return Err(format!(
                            "Error while parsing HCL, type not yet supported: {:?} for identifier {:?}, attribute: {:?}",
                            unimplemented_expression, attribute.key(), attribute.expr()
                        ))
                    }
                }
            }
            if block.labels().len() != 1 {
                panic!(
                    "Expected exactly one label for output block, found: {:?}",
                    block.labels()
                );
            }
            let output_name = block.labels().get(0).unwrap().as_str().to_string();

            let output = env_defs::Output {
                name: output_name,
                description: attrs
                    .get("description")
                    .unwrap_or(&"".to_string())
                    .to_string(),
                value: attrs.get("value").unwrap_or(&"".to_string()).to_string(),
            };

            outputs.push(output);
        }
    }
    // log::info!("variables: {:?}", serde_json::to_string(&variables));
    Ok(outputs)
}
