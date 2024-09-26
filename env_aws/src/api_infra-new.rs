// use anyhow::Result;
// use aws_config::meta::region::RegionProviderChain;
// use aws_sdk_dynamodb::types::AttributeValue;
// use aws_sdk_dynamodb::Client as DynamoDbClient;
// use aws_sdk_ecs::types::{
//     AssignPublicIp, AwsVpcConfiguration, ContainerOverride, KeyValuePair, LaunchType,
//     NetworkConfiguration, TaskOverride,
// };
// use aws_sdk_ecs::Client as EcsClient;
// use chrono::Utc;
// use rand::{distributions::Alphanumeric, thread_rng, Rng};
// use serde_json::Value;
// use std::collections::HashMap;
// use std::time::{SystemTime, UNIX_EPOCH};

// use crate::utils::get_ssm_parameter;

// pub async fn mutate_infra(
//     event: String,
//     module: String,
//     name: String,
//     environment: String,
//     mut deployment_id: String,
//     spec: Value,
//     annotations: Value,
// ) -> Result<String> {
//     // Load environment variables
//     let region_provider = RegionProviderChain::default_provider().or_else("us-east-1");
//     let shared_config = aws_config::from_env().region(region_provider).load().await;
//     let region = shared_config.region().map(|r| r.as_ref()).unwrap();
//     let ssm_client = aws_sdk_ssm::Client::new(&shared_config);
//     let dynamodb_client = DynamoDbClient::new(&shared_config);
//     let ecs_client = EcsClient::new(&shared_config);

//     let event_table_name = get_ssm_parameter(
//         &ssm_client,
//         &format!(
//             "/infrabridge/{}/{}/dynamodb_events_table_name",
//             region, environment
//         ),
//         false,
//     )
//     .await
//     .map_err(|e| anyhow::Error::msg(e.to_string()))?;

//     let modules_bucket = get_ssm_parameter(
//         &ssm_client,
//         &format!("/infrabridge/{}/{}/modules_bucket", region, environment),
//         false,
//     )
//     .await
//     .map_err(|e| anyhow::Error::msg(e.to_string()))?;

//     let ecs_cluster_name = get_ssm_parameter(
//         &ssm_client,
//         &format!("/infrabridge/{}/{}/ecs_cluster_name", region, environment),
//         false,
//     )
//     .await
//     .map_err(|e| anyhow::Error::msg(e.to_string()))?;

//     let ecs_task_definition = get_ssm_parameter(
//         &ssm_client,
//         &format!(
//             "/infrabridge/{}/{}/ecs_task_definition",
//             region, environment
//         ),
//         false,
//     )
//     .await
//     .map_err(|e| anyhow::Error::msg(e.to_string()))?;

//     let ecs_subnet_id = get_ssm_parameter(
//         &ssm_client,
//         &format!("/infrabridge/{}/{}/ecs_subnet_id", region, environment),
//         false,
//     )
//     .await
//     .map_err(|e| anyhow::Error::msg(e.to_string()))?;

//     let ecs_security_group = get_ssm_parameter(
//         &ssm_client,
//         &format!("/infrabridge/{}/{}/ecs_security_group", region, environment),
//         false,
//     )
//     .await
//     .map_err(|e| anyhow::Error::msg(e.to_string()))?;

//     let modules_table_name = get_ssm_parameter(
//         &ssm_client,
//         &format!("/infrabridge/{}/{}/modules_table_name", region, environment),
//         false,
//     )
//     .await
//     .map_err(|e| anyhow::Error::msg(e.to_string()))?;

//     let response = dynamodb_client
//         .query()
//         .table_name(modules_table_name)
//         .key_condition_expression("#mod = :module_val")
//         .filter_expression("#env = :env_val")
//         .expression_attribute_names("#mod", "module")
//         .expression_attribute_names("#env", "environment")
//         .expression_attribute_values(":module_val", AttributeValue::S(module.to_string()))
//         .expression_attribute_values(":env_val", AttributeValue::S(environment.to_string()))
//         .limit(1)
//         .send()
//         .await?;

//     // Handling the response and fetching the latest module entry
//     let latest_module = match response.items {
//         Some(items) if !items.is_empty() => (&items[0]).clone(),
//         _ => return Err(anyhow::Error::msg("No module found")),
//     };

//     let s3_key = match latest_module.get("s3_key") {
//         Some(AttributeValue::S(key)) => key, // If it's a string, extract it
//         _ => return Err(anyhow::Error::msg("No 's3_key' found in the latest module")),
//     };

//     let source_location = format!("{}/{}", modules_bucket, s3_key);

//     // Generate or verify the deployment_id
//     if deployment_id.is_empty() {
//         deployment_id = generate_unique_deployment_id(&module, &name).await?;
//     }

//     // Build signal with status received
//     let signal = get_signal(
//         &deployment_id,
//         &event,
//         &module,
//         &name,
//         &spec,
//         "received".to_string(),
//     );

//     println!("Signal: {:?}", signal);

//     // Convert the signal JSON to a HashMap<String, AttributeValue>
//     let signal_map: HashMap<String, AttributeValue> = match &signal {
//         Value::Object(map) => map
//             .iter()
//             .map(|(k, v)| (k.clone(), json_to_dynamodb_attr(v)))
//             .collect(),
//         _ => return Err(anyhow::Error::msg("It is not a JSON object")),
//     };

//     // Write signal to DynamoDB
//     dynamodb_client
//         .put_item()
//         .table_name(&event_table_name)
//         .set_item(Some(signal_map))
//         .send()
//         .await?;

//     // Validate the event type
//     if !["apply", "destroy"].contains(&event.as_str()) {
//         return Err(anyhow::anyhow!("Invalid event type ({})", event));
//     }

//     // Build environment variables for the ECS task
//     let mut module_envs = vec![];
//     if let Value::Object(spec_map) = &spec {
//         for (key, value) in spec_map.iter() {
//             let env_var_name = format!("TF_VAR_{}", camel_to_snake(key));
//             let env_var_value = value.as_str().unwrap_or("").to_string();
//             module_envs.push(
//                 KeyValuePair::builder()
//                     .name(env_var_name)
//                     .value(env_var_value)
//                     .build(),
//             );
//         }
//     }

//     // Add other necessary environment variables
//     module_envs.push(
//         KeyValuePair::builder()
//             .name("DEPLOYMENT_ID")
//             .value(&deployment_id)
//             .build(),
//     );
//     module_envs.push(KeyValuePair::builder().name("EVENT").value(&event).build());
//     module_envs.push(
//         KeyValuePair::builder()
//             .name("MODULE_NAME")
//             .value(&module)
//             .build(),
//     );
//     module_envs.push(
//         KeyValuePair::builder()
//             .name("SIGNAL")
//             .value(signal.to_string())
//             .build(),
//     );
//     module_envs.push(
//         KeyValuePair::builder()
//             .name("SOURCE_LOCATION")
//             .value(source_location)
//             .build(),
//     );

//     // Run ECS task
//     let ecs_response = ecs_client
//         .run_task()
//         .cluster(ecs_cluster_name)
//         .task_definition(ecs_task_definition)
//         .launch_type(LaunchType::Fargate) // Direct reference to LaunchType
//         .overrides(
//             TaskOverride::builder() // Direct reference to TaskOverride
//                 .container_overrides(
//                     ContainerOverride::builder() // Direct reference to ContainerOverride
//                         .name("terraform-docker")
//                         .set_environment(
//                             Some(module_envs), // Include the environment variables
//                         )
//                         .build(),
//                 )
//                 .build(),
//         )
//         .network_configuration(
//             NetworkConfiguration::builder()
//                 .awsvpc_configuration(
//                     AwsVpcConfiguration::builder() // Direct reference to AwsVpcConfiguration
//                         .subnets(ecs_subnet_id)
//                         .security_groups(ecs_security_group)
//                         .assign_public_ip(AssignPublicIp::Enabled) // Direct reference to AssignPublicIp
//                         .build()?,
//                 )
//                 .build(),
//         )
//         .count(1)
//         .send()
//         .await?;

//     // Log ECS Task response
//     println!("ECS Task Response: {:?}", ecs_response);

//     // Build signal for initiated or failed
//     let ecs_task_arn = ecs_response
//         .tasks
//         .unwrap_or_default() // Unwraps the Option or returns an empty Vec
//         .get(0) // Now works on the Vec
//         .and_then(|task| task.task_arn.clone())
//         .unwrap_or("NO_TASK_ARN".to_string());

//     let final_signal = get_signal(
//         &deployment_id,
//         &event,
//         &module,
//         &name,
//         &spec,
//         "initiated".to_string(),
//     );

//     // Convert the signal JSON to a HashMap<String, AttributeValue>
//     let signal_map: HashMap<String, AttributeValue> = match &final_signal {
//         Value::Object(map) => map
//             .iter()
//             .map(|(k, v)| (k.clone(), json_to_dynamodb_attr(v)))
//             .collect(),
//         _ => return Err(anyhow::Error::msg("It is not a JSON object")),
//     };

//     // Write signal to DynamoDB
//     dynamodb_client
//         .put_item()
//         .table_name(&event_table_name)
//         .set_item(Some(signal_map))
//         .send()
//         .await?;

//     Ok(deployment_id)
// }

// fn json_to_dynamodb_attr(value: &Value) -> AttributeValue {
//     match value {
//         Value::Null => AttributeValue::Null(true),
//         Value::Bool(b) => AttributeValue::Bool(*b),
//         Value::Number(n) => {
//             if let Some(i) = n.as_i64() {
//                 AttributeValue::N(i.to_string())
//             } else if let Some(f) = n.as_f64() {
//                 AttributeValue::N(f.to_string())
//             } else {
//                 AttributeValue::N(n.to_string())
//             }
//         }
//         Value::String(s) => AttributeValue::S(s.clone()),
//         Value::Array(arr) => {
//             AttributeValue::L(arr.iter().map(|v| json_to_dynamodb_attr(v)).collect())
//         }
//         Value::Object(map) => AttributeValue::M(
//             map.iter()
//                 .map(|(k, v)| (k.clone(), json_to_dynamodb_attr(v)))
//                 .collect(),
//         ),
//     }
// }

// async fn generate_unique_deployment_id(module: &str, name: &str) -> Result<String> {
//     let deployment_id = format!("{}-{}-{}", module, name, generate_random_id(3));
//     Ok(deployment_id)
// }

// fn generate_random_id(length: usize) -> String {
//     thread_rng()
//         .sample_iter(&Alphanumeric)
//         .take(length)
//         .map(char::from)
//         .collect()
// }

// fn get_signal(
//     deployment_id: &str,
//     event: &str,
//     module: &str,
//     name: &str,
//     spec: &Value,
//     status: String,
// ) -> Value {
//     let epoch_milliseconds = SystemTime::now()
//         .duration_since(UNIX_EPOCH)
//         .expect("Time went backwards")
//         .as_millis();

//     serde_json::json!({
//         "deployment_id": deployment_id,
//         "event": event,
//         "module": module,
//         "name": name,
//         "spec": spec,
//         "id": format!("{}-{}-{}-{}", deployment_id, event, epoch_milliseconds, status),
//         "status": status,
//         "epoch": epoch_milliseconds,
//         "timestamp": Utc::now().to_rfc3339(),
//         "metadata": "",
//     })
// }

// fn camel_to_snake(name: &str) -> String {
//     let mut snake = String::new();
//     for (i, c) in name.char_indices() {
//         if c.is_uppercase() && i != 0 {
//             snake.push('_');
//         }
//         snake.push(c.to_ascii_lowercase());
//     }
//     snake
// }
