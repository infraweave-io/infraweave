use std::{thread, time::Duration};

use env_common::{interface::{initialize_project_id, CloudHandler}, logic::{destroy_infra, handler, is_deployment_in_progress}, submit_claim_job};
use env_defs::{ApiInfraPayload, DriftDetection, ModuleResp};
use log::info;
use pyo3::{exceptions::PyException, prelude::*};
use tokio::runtime::Runtime;
use serde_json::Value;
use crate::{module::Module, stack::Stack};

#[pyclass]
pub struct Deployment {
    module: ModuleResp,
    variables: Value,
    is_stack: bool,
    name: String,
    environment: String,
    deployment_id: String,
}

#[pymethods]
impl Deployment {
    #[new]
    fn new(name: String, environment: String, module: Option<Module>, stack: Option<Stack>) -> PyResult<Self> {

        let deployment_id = name.clone();

        match (module, stack) {
            (None, None) => Err(PyException::new_err("Either module or stack must be provided")),
            (Some(_), Some(_)) => Err(PyException::new_err("Only one of module or stack must be provided")),
            (Some(module), None) => {
                // Use the module provided
                Ok(Deployment {
                    variables: Value::Null,
                    module: module.module,
                    is_stack: false,
                    name,
                    environment,
                    deployment_id,
                })
            }
            (None, Some(stack)) => {
                // Use the module from the stack provided
                Ok(Deployment {
                    variables: Value::Null,
                    module: stack.module,
                    is_stack: true,
                    name,
                    environment,
                    deployment_id,
                })
            }
        }
    }
    
    fn set_variables(&mut self, variables: &PyAny) -> PyResult<()> {
        let py = variables.py();
        let json_module = py.import("json")?;
        let json_str = json_module
            .call_method1("dumps", (variables,))?
            .extract::<String>()?;
        let value: Value = serde_json::from_str(&json_str)
            .map_err(|e| PyException::new_err(format!("Failed to parse JSON: {}", e)))?;
        self.variables = value.clone();
        Ok(())
    }

    fn apply(&self) -> PyResult<String> {
        println!("Applying {} in environment {}", self.name, self.environment);
        let rt = Runtime::new().unwrap();
        let job_id = rt.block_on(run_job("apply", &self));
        Ok((job_id).to_string())
    }

    fn plan(&self) -> PyResult<String> {
        println!("Planning {} in environment {}", self.name, self.environment);
        let rt = Runtime::new().unwrap();
        let job_id = rt.block_on(run_job("plan", &self));
        Ok((job_id).to_string())
    }

    fn destroy(&self) -> PyResult<String> {
        println!("Destroying {} in environment {}", self.name, self.environment);
        let rt = Runtime::new().unwrap();
        let job_id = rt.block_on(run_job("destroy", &self));
        Ok((job_id).to_string())
    }
}

async fn run_job(command: &str, deployment: &Deployment) -> String {
    let job_id = match command {
        "destroy" => destroy_infra(&deployment.deployment_id, &deployment.environment).await.unwrap(),
        "apply" => plan_or_apply_deployment(command, deployment).await,
        "plan" => plan_or_apply_deployment(command, deployment).await,
        _ => panic!("Invalid command"),
    };

    loop {
        let (in_progress, _, _, _) = is_deployment_in_progress(&deployment.deployment_id, &deployment.environment).await;
        if !in_progress {
            println!("Finished {} successfully! (job_id: {})", command, job_id);
            break;
        }
        thread::sleep(Duration::from_secs(10));
    }
    
    job_id
}

async fn plan_or_apply_deployment(command: &str, deployment: &Deployment) -> String {

    let project_id = initialize_project_id().await;
    let region = "eu-central-1";
    
    let payload = ApiInfraPayload {
        command: command.to_string(),
        args: vec![],
        module: deployment.module.module.clone().to_lowercase(), // TODO: Only have access to kind, not the module name (which is assumed to be lowercase of module_name)
        module_type: if deployment.is_stack {"stack"} else {"module"}.to_string(),
        module_version: deployment.module.version.clone(),
        name: deployment.name.clone(),
        environment: deployment.environment.clone(),
        deployment_id: deployment.deployment_id.clone(),
        project_id: project_id.to_string(),
        region: region.to_string(),
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
        initiated_by: handler().get_user_id().await.unwrap(),
    };

    let job_id = submit_claim_job(&payload).await;

    info!("Job ID: {}", job_id);

    job_id
}