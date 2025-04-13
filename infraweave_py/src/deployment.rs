use core::panic;
use std::{thread, time::Duration};

use crate::{module::Module, stack::Stack, utils::get_variable_mapping};
use env_common::{
    interface::GenericCloudHandler,
    logic::{destroy_infra, is_deployment_in_progress, run_claim},
};
use env_defs::{
    DeploymentManifest, DeploymentMetadata, DeploymentResp, DeploymentSpec, ExtraData, ModuleResp,
};
use log::info;
use pyo3::{create_exception, exceptions::PyException, prelude::*, types::PyDict};
use serde_json::Value;
use tokio::runtime::Runtime;

create_exception!(infraweave, DeploymentFailure, PyException);

#[pyclass]
pub struct Deployment {
    module: ModuleResp,
    variables: Value,
    is_stack: bool,
    name: String,
    environment: String,
    deployment_id: String,
    region: String,
    reference: String,
}

#[pymethods]
impl Deployment {
    #[new]
    fn new(
        name: String,
        environment: String,
        region: String,
        module: Option<&PyAny>,
        stack: Option<&PyAny>,
    ) -> PyResult<Self> {
        let reference = "python".to_string();

        match (module, stack) {
            (None, None) => Err(PyException::new_err(
                "Either module or stack must be provided",
            )),
            (Some(_), Some(_)) => Err(PyException::new_err(
                "Only one of module or stack must be provided",
            )),
            (Some(module), None) => {
                let module = extract_module(module)?;
                Ok(Deployment {
                    deployment_id: format!("{}/{}", module.module.module, name.clone()),
                    environment: get_environment(&environment),
                    region,
                    name: name.clone(),
                    variables: Value::Null,
                    module: module.module.clone(),
                    is_stack: false,
                    reference,
                })
            }
            (None, Some(stack)) => {
                let stack = extract_stack(stack)?;
                Ok(Deployment {
                    deployment_id: format!("{}/{}", stack.module.module, name.clone()),
                    environment: get_environment(&environment),
                    region,
                    name,
                    variables: Value::Null,
                    module: stack.module,
                    is_stack: true,
                    reference,
                })
            }
        }
    }

    #[args(kwargs = "**")]
    fn set_variables(&mut self, kwargs: Option<&PyDict>) -> PyResult<()> {
        if let Some(arguments) = kwargs {
            let py = arguments.py();
            let json_module = py.import("json")?;
            let json_str = json_module
                .call_method1("dumps", (arguments,))?
                .extract::<String>()?;

            let value: Value = serde_json::from_str(&json_str)
                .map_err(|e| PyException::new_err(format!("Failed to parse JSON: {}", e)))?;

            self.variables = value.clone();

            println!(
                "Setting variables for deployment {} in environment {} to:\n{}",
                self.name, self.environment, value
            );
        } else {
            return Err(PyException::new_err("No variables provided"));
        }
        Ok(())
    }

    fn apply(&self) -> PyResult<String> {
        println!(
            "Applying {} in environment {} ({})",
            self.name, self.environment, self.region
        );
        let rt = Runtime::new().unwrap();
        let (job_id, status, deployment) = match rt.block_on(run_job("apply", self)) {
            Ok((job_id, status, deployment)) => (job_id, status, deployment),
            Err(e) => {
                return Err(DeploymentFailure::new_err(format!(
                    "Failed to run apply for {}: {}",
                    self.deployment_id, e
                )));
            }
        };
        if status != "successful" {
            return Err(DeploymentFailure::new_err(format!(
                "Apply failed with status: {}, error: {}",
                status,
                deployment
                    .as_ref()
                    .map(|d| d.error_text.clone())
                    .unwrap_or_else(|| "No error message".to_string())
            )));
        }
        Ok((job_id).to_string())
    }

    fn plan(&self) -> PyResult<String> {
        println!(
            "Planning {} in environment {} ({})",
            self.name, self.environment, self.region
        );
        let rt = Runtime::new().unwrap();
        let (job_id, status, deployment) = match rt.block_on(run_job("plan", self)) {
            Ok((job_id, status, deployment)) => (job_id, status, deployment),
            Err(e) => {
                return Err(DeploymentFailure::new_err(format!(
                    "Failed to run plan for {}: {}",
                    self.deployment_id, e
                )));
            }
        };
        if status != "successful" {
            return Err(DeploymentFailure::new_err(format!(
                "Plan failed with status: {}, error: {}",
                status,
                deployment
                    .as_ref()
                    .map(|d| d.error_text.clone())
                    .unwrap_or_else(|| "No error message".to_string())
            )));
        }
        Ok((job_id).to_string())
    }

    fn destroy(&self) -> PyResult<String> {
        println!(
            "Destroying {} in environment {} ({})",
            self.name, self.environment, self.region
        );
        let rt = Runtime::new().unwrap();
        let (job_id, status, deployment) = match rt.block_on(run_job("destroy", self)) {
            Ok((job_id, status, deployment)) => (job_id, status, deployment),
            Err(e) => {
                return Err(DeploymentFailure::new_err(format!(
                    "Failed to run destroy for {}: {}",
                    self.deployment_id, e
                )));
            }
        };
        if status != "successful" {
            return Err(DeploymentFailure::new_err(format!(
                "Destroy failed with status: {}, error: {}",
                status,
                deployment
                    .as_ref()
                    .map(|d| d.error_text.clone())
                    .unwrap_or_else(|| "No error message".to_string())
            )));
        }
        Ok((job_id).to_string())
    }
}

pub fn get_environment(environment_arg: &str) -> String {
    if !environment_arg.contains('/') {
        format!("python/{}", environment_arg)
    } else {
        environment_arg.to_string()
    }
}

async fn run_job(
    command: &str,
    deployment: &Deployment,
) -> Result<(String, String, Option<DeploymentResp>), anyhow::Error> {
    let handler = &GenericCloudHandler::region(&deployment.region).await;
    let result = match command {
        "destroy" => {
            destroy_infra(
                &handler,
                &deployment.deployment_id,
                &deployment.environment,
                ExtraData::None,
            )
            .await
        }
        "apply" => plan_or_apply_deployment(command, deployment).await,
        "plan" => plan_or_apply_deployment(command, deployment).await,
        _ => panic!("Invalid command"),
    };
    let job_id = match result {
        Ok(job_id) => job_id,
        Err(e) => {
            return Err(anyhow::anyhow!("{}", e));
        }
    };

    let final_status: String;
    let deployment_result: Option<DeploymentResp>;

    loop {
        let (in_progress, _, _status, deployment_job_result) =
            is_deployment_in_progress(&handler, &deployment.deployment_id, &deployment.environment)
                .await;
        if !in_progress {
            let status = if command == "destroy" {
                "successful" // Since deployment not found is considered successful
            } else {
                &match &deployment_job_result {
                    Some(deployment_job_result) => deployment_job_result.status.clone(),
                    None => "unknown".to_string(),
                }
            };
            println!(
                "Finished {} with status {}! (job_id: {})\n{}",
                command,
                status,
                job_id,
                deployment_job_result
                    .as_ref()
                    .map(|d| d.error_text.clone())
                    .unwrap_or_else(|| "No error_text".to_string())
            );
            final_status = status.to_string();
            deployment_result = deployment_job_result;
            break;
        }
        thread::sleep(Duration::from_secs(10));
    }

    Ok((job_id, final_status, deployment_result))
}

async fn plan_or_apply_deployment(
    command: &str,
    deployment: &Deployment,
) -> Result<String, anyhow::Error> {
    let variable_mapping = get_variable_mapping(deployment.is_stack, &deployment.variables);
    let variables_yaml_mapping = match serde_yaml::to_value(&variable_mapping).unwrap() {
        serde_yaml::Value::Mapping(map) => map,
        _ => panic!("Expected a mapping"),
    };

    let deployment_spec = DeploymentSpec {
        module_version: if deployment.is_stack {
            None
        } else {
            Some(deployment.module.version.clone())
        },
        stack_version: if deployment.is_stack {
            Some(deployment.module.version.clone())
        } else {
            None
        },
        region: deployment.region.clone(),
        reference: Some(deployment.reference.clone()),
        variables: variables_yaml_mapping,
        dependencies: None,
        drift_detection: None,
    };

    let deployment_manifest = DeploymentManifest {
        api_version: "infraweave.io/v1".to_string(),
        metadata: DeploymentMetadata {
            name: deployment.name.clone(),
            namespace: Some(deployment.environment.clone()),
            labels: None,
            annotations: None,
        },
        kind: deployment.module.module_name.clone(),
        spec: deployment_spec,
    };

    let deployment_yaml = serde_yaml::to_value(&deployment_manifest).unwrap();
    info!(
        "Running equivalent {} of deployment YAML: {}",
        command,
        serde_yaml::to_string(&deployment_yaml).unwrap()
    );

    let (job_id, _deployment_id) = match run_claim(
        &GenericCloudHandler::region(&deployment.region).await,
        &deployment_yaml,
        &deployment.environment,
        command,
        vec![],
        ExtraData::None,
        &deployment.reference,
    )
    .await
    {
        Ok((job_id, deployment_id)) => (job_id, deployment_id),
        Err(e) => {
            return Err(anyhow::anyhow!(e));
        }
    };
    info!(
        "Deployment id: {}, environment: {}, job id: {}",
        deployment.deployment_id, deployment.environment, job_id
    );

    Ok(job_id)
}

fn extract_module(obj: &PyAny) -> PyResult<Module> {
    if let Ok(module_attr) = obj.getattr("module") {
        module_attr.extract()
    } else {
        obj.extract()
    }
}

fn extract_stack(obj: &PyAny) -> PyResult<Stack> {
    if let Ok(module_attr) = obj.getattr("module") {
        module_attr.extract()
    } else {
        obj.extract()
    }
}
