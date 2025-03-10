use std::{thread, time::Duration};

use crate::{module::Module, stack::Stack};
use env_common::{
    interface::{initialize_project_id_and_region, GenericCloudHandler},
    logic::{destroy_infra, is_deployment_in_progress},
    submit_claim_job,
};
use env_defs::{ApiInfraPayload, CloudProvider, DriftDetection, ExtraData, ModuleResp};
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
    reference: String,
}

#[pymethods]
impl Deployment {
    #[new]
    fn new(
        name: String,
        environment: String,
        module: Option<&PyAny>,
        stack: Option<&PyAny>,
    ) -> PyResult<Self> {
        let deployment_id = name.clone();
        let reference = "python-script".to_string();

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
                    variables: Value::Null,
                    module: module.module,
                    is_stack: false,
                    name,
                    environment,
                    deployment_id,
                    reference,
                })
            }
            (None, Some(stack)) => {
                let stack = extract_stack(stack)?;
                Ok(Deployment {
                    variables: Value::Null,
                    module: stack.module,
                    is_stack: true,
                    name,
                    environment,
                    deployment_id,
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
        println!("Applying {} in environment {}", self.name, self.environment);
        let rt = Runtime::new().unwrap();
        let (job_id, status) = rt.block_on(run_job("apply", self));
        if status != "successful" {
            return Err(DeploymentFailure::new_err(format!(
                "Apply failed with status: {:?}",
                status
            )));
        }
        Ok((job_id).to_string())
    }

    fn plan(&self) -> PyResult<String> {
        println!("Planning {} in environment {}", self.name, self.environment);
        let rt = Runtime::new().unwrap();
        let (job_id, status) = rt.block_on(run_job("plan", self));
        if status != "successful" {
            return Err(DeploymentFailure::new_err(format!(
                "Plan failed with status: {:?}",
                status
            )));
        }
        Ok((job_id).to_string())
    }

    fn destroy(&self) -> PyResult<String> {
        println!(
            "Destroying {} in environment {}",
            self.name, self.environment
        );
        let rt = Runtime::new().unwrap();
        let (job_id, status) = rt.block_on(run_job("destroy", self));
        if status != "successful" {
            return Err(DeploymentFailure::new_err(format!(
                "Destroy failed with status: {:?}",
                status
            )));
        }
        Ok((job_id).to_string())
    }
}

async fn run_job(command: &str, deployment: &Deployment) -> (String, String) {
    let handler = GenericCloudHandler::default().await;
    let job_id = match command {
        "destroy" => destroy_infra(
            &handler,
            &deployment.deployment_id,
            &deployment.environment,
            ExtraData::None,
        )
        .await
        .unwrap(),
        "apply" => plan_or_apply_deployment(command, deployment).await,
        "plan" => plan_or_apply_deployment(command, deployment).await,
        _ => panic!("Invalid command"),
    };

    let final_status: String;

    loop {
        let (in_progress, _, status, _) =
            is_deployment_in_progress(&handler, &deployment.deployment_id, &deployment.environment)
                .await;
        if !in_progress {
            let status = if command == "destroy" {
                "successful"
            } else {
                &status
            };
            println!(
                "Finished {} with status {}! (job_id: {})",
                command, status, job_id
            );
            final_status = status.to_string();
            break;
        }
        thread::sleep(Duration::from_secs(10));
    }

    (job_id, final_status)
}

async fn plan_or_apply_deployment(command: &str, deployment: &Deployment) -> String {
    let project_id = initialize_project_id_and_region().await;
    let handler = GenericCloudHandler::default().await;

    let payload = ApiInfraPayload {
        command: command.to_string(),
        flags: vec![],
        module: deployment.module.module.clone().to_lowercase(), // TODO: Only have access to kind, not the module name (which is assumed to be lowercase of module_name)
        module_type: if deployment.is_stack {
            "stack"
        } else {
            "module"
        }
        .to_string(),
        module_version: deployment.module.version.clone(),
        module_track: deployment.module.track.clone(),
        name: deployment.name.clone(),
        environment: deployment.environment.clone(),
        deployment_id: deployment.deployment_id.clone(),
        project_id: project_id.to_string(),
        region: handler.get_region().to_string(),
        drift_detection: DriftDetection {
            enabled: false,
            interval: "1h".to_string(),
            webhooks: vec![],
            auto_remediate: false,
        },
        next_drift_check_epoch: -1, // Prevent reconciler from finding this deployment since it is in progress
        variables: deployment.variables.clone(),
        annotations: serde_json::from_str("{}").unwrap(),
        dependencies: vec![],
        initiated_by: handler.get_user_id().await.unwrap(),
        cpu: deployment.module.cpu.clone(),
        memory: deployment.module.memory.clone(),
        reference: deployment.reference.to_string(),
        extra_data: ExtraData::None,
    };

    let job_id = submit_claim_job(&handler, &payload).await.unwrap(); // TODO: Handle with python error

    info!("Job ID: {}", job_id);

    job_id
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
