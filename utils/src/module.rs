use env_defs::TfOutput;
use env_defs::TfVariable;
use hcl::de;
use std::collections::HashMap;
use std::io::{self, ErrorKind};

pub fn validate_tf_backend_not_set(contents: &String) -> Result<(), String> {
    let parsed_hcl: HashMap<String, serde_json::Value> =
        de::from_str(&contents).map_err(|err| format!("Failed to parse HCL: {}", err))?;

        if let Some(terraform_blocks) = parsed_hcl.get("terraform") {
        if terraform_blocks.is_object() {
            ensure_no_backend_block(terraform_blocks).unwrap();
        } else if terraform_blocks.is_array() {
            for terraform_block in terraform_blocks.as_array().unwrap() {
                ensure_no_backend_block(terraform_block).unwrap();
            }
        } else {
            return Ok(());
        }
    }

    Ok(())
}

fn ensure_no_backend_block(terraform_block: &serde_json::Value) -> Result<(), String> {
    // Check if the backend block is present in the terraform configuration
    if let Some(backend_blocks) = terraform_block.get("backend") {
        panic!(
            "Backend block was found in the terraform backend configuration\n{}", get_block_help(terraform_block)
        );
    }
    Ok(())
}

pub fn get_block_help(block: &serde_json::Value) -> String {
    let help = format!(r#"
Please make sure you do not set any backend block in your terraform code, this is handled by the platform.

Remove this block from your terraform configuration to proceed:

{}
    "#, hcl::to_string(block).unwrap());
    help.to_string()
}

pub fn get_variables_from_tf_files(contents: &String) -> Result<Vec<TfVariable>, String> {
    let parsed_hcl: HashMap<String, serde_json::Value> =
        de::from_str(&contents).map_err(|err| format!("Failed to parse HCL: {}", err))?;

    let mut variables = Vec::new();

    // Iterate through the HCL blocks (assuming `parsed_hcl` is correctly structured)
    if let Some(var_blocks) = parsed_hcl.get("variable") {
        if let Some(var_map) = var_blocks.as_object() {
            for (var_name, var_attrs) in var_map {
                // Extract the attributes for the variable (type, default, description, etc.)
                let variable_type = var_attrs
                    .get("type")
                    .cloned()
                    .unwrap_or(serde_json::Value::String("string".to_string()));
                // Handle type values that might be wrapped in ${}
                let variable_type = match variable_type {
                    serde_json::Value::String(s) => {
                        // Strip ${} if present
                        if s.starts_with("${") && s.ends_with("}") {
                            serde_json::Value::String(
                                s.trim_start_matches("${").trim_end_matches("}").to_string(),
                            )
                        } else {
                            serde_json::Value::String(s)
                        }
                    }
                    _ => variable_type, // Keep as is for complex types like maps
                };
                let default_value = var_attrs
                    .get("default")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let description = var_attrs
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let nullable = var_attrs
                    .get("nullable")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let sensitive = var_attrs
                    .get("sensitive")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let variable = TfVariable {
                    name: var_name.clone(),
                    _type: variable_type,
                    default: Some(default_value),
                    description: Some(description),
                    nullable: Some(nullable),
                    sensitive: Some(sensitive),
                };

                variables.push(variable);
            }
        }
    }

    Ok(variables)
}

pub fn get_outputs_from_tf_files(contents: &String) -> Result<Vec<env_defs::TfOutput>, String> {
    let hcl_body = hcl::parse(&contents)
        .map_err(|_| io::Error::new(ErrorKind::InvalidData, "Failed to parse HCL content"))
        .unwrap();

    let mut outputs = Vec::new();

    for block in hcl_body.blocks() {
        if block.identifier() == "output" {
            // Exclude outputs that are not meant to be exported, such as "value"
            let attrs = get_attributes(&block, vec!["value".to_string()]);

            if block.labels().len() != 1 {
                panic!(
                    "Expected exactly one label for output block, found: {:?}",
                    block.labels()
                );
            }
            let output_name = block.labels().get(0).unwrap().as_str().to_string();

            let output = TfOutput {
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

fn get_attributes(block: &hcl::Block, excluded_attrs: Vec<String>) -> HashMap<&str, String> {
    let mut attrs = HashMap::new();

    for attribute in block.body().attributes() {
        if excluded_attrs.contains(&attribute.key().to_string()) {
            continue;
        }
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
            hcl::Expression::Null => {
                attrs.insert(attribute.key(), "null".to_string());
            }
            hcl::Expression::Number(n) => {
                attrs.insert(attribute.key(), n.to_string());
            }
            hcl::Expression::TemplateExpr(te) => {
                attrs.insert(attribute.key(), te.to_string());
            }
            hcl::Expression::Object(o) => {
                let object_elements = o.iter().map(|(key, value)| {
                    match value {
                        hcl::Expression::String(s) => serde_json::to_string(&s).unwrap(),
                        hcl::Expression::Variable(v) => serde_json::to_string(&v.to_string()).unwrap(),
                        hcl::Expression::Bool(b) => serde_json::to_string(&b).unwrap(),
                        hcl::Expression::Number(n) => serde_json::to_string(&n).unwrap(),
                        hcl::Expression::Null => "null".to_string(),
                        // Add other necessary cases here
                        unimplemented_expression => {
                            panic!(
                                "Error while parsing HCL inside an object, type not yet supported: {:?} for identifier {:?}, attribute: {:?}",
                                unimplemented_expression, attribute.key(), attribute.expr()
                            )
                        }
                    }
                }).collect::<Vec<String>>().join(", ");
                attrs.insert(attribute.key(), format!("{{{}}}", object_elements));
            }
            hcl::Expression::Array(a) => {
                let array_elements = a.iter().map(|item| {
                    match item {
                        hcl::Expression::String(s) => serde_json::to_string(&s).unwrap(),
                        hcl::Expression::Variable(v) => serde_json::to_string(&v.to_string()).unwrap(),
                        hcl::Expression::Bool(b) => serde_json::to_string(&b).unwrap(),
                        hcl::Expression::Number(n) => serde_json::to_string(&n).unwrap(),
                        hcl::Expression::Null => "null".to_string(),
                        // Add other necessary cases here
                        unimplemented_expression => {
                            panic!(
                                "Error while parsing HCL inside an array, type not yet supported: {:?} for identifier {:?}, attribute: {:?}",
                                unimplemented_expression, attribute.key(), attribute.expr()
                            )
                        }
                    }
                }).collect::<Vec<String>>().join(", ");
                attrs.insert(attribute.key(), format!("[{}]", array_elements));
            }
            // TODO: Add support for other types to support validation parameter
            unimplemented_expression => {
                // If validation block (type is hcl::FuncCall), pass
                if let hcl::Expression::FuncCall(_) = unimplemented_expression {
                    continue;
                }
                panic!(
                    "Error while parsing HCL, type not yet supported: {:?} for identifier {:?}, attribute: {:?}",
                    unimplemented_expression, attribute.key(), attribute.expr()
                )
            }
        }
    }

    attrs
}
