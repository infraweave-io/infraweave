use crate::{module::Module, stack::Stack, utils::get_variable_mapping};
use core::panic;
use env_common::{
    interface::GenericCloudHandler,
    logic::{destroy_infra, is_deployment_in_progress, is_deployment_plan_in_progress, run_claim},
};
use env_defs::CloudProvider;
use env_defs::{
    DeploymentManifest, DeploymentMetadata, DeploymentResp, DeploymentSpec, ExtraData, ModuleResp,
};
use log::info;
use pyo3::Bound;
use pyo3::{create_exception, exceptions::PyException, prelude::*, types::PyDict};
use serde_json::Value;
use std::{thread, time::Duration};
use tokio::runtime::Runtime;

// Define a Python exception for deployment failures
create_exception!(infraweave, DeploymentFailure, PyException);

#[derive(Clone)]
struct ResultBase {
    job_id: String,
    environment_id: String,
    deployment_id: String,
    region: String,
}

impl ResultBase {
    fn fetch_output(&self, command: &str) -> PyResult<String> {
        let rt = Runtime::new().unwrap();
        let handler = rt.block_on(GenericCloudHandler::region(&self.region));

        let change_record = rt.block_on(handler.get_change_record(
            &self.environment_id,
            &self.deployment_id,
            &self.job_id,
            command,
        ));

        match change_record {
            Ok(record) => Ok(record.plan_std_output),
            Err(e) => Err(PyException::new_err(format!(
                "Failed to fetch output: {}",
                e
            ))),
        }
    }

    fn get_environment_id(&self) -> &str {
        &self.environment_id
    }

    fn get_deployment_id(&self) -> &str {
        &self.deployment_id
    }

    fn get_region(&self) -> &str {
        &self.region
    }
}

/// Represents the result of a plan operation.
///
/// PlanResult provides methods to analyze the planned infrastructure changes
/// before actually applying them.
///
/// # Example
///
/// ```python
/// from infraweave import Deployment, S3Bucket
///
/// bucket_module = S3Bucket(version='0.0.11-dev', track="dev")
/// bucket1 = Deployment(name="bucket1", namespace="playground", module=bucket_module, region="us-west-2")
///
/// bucket1.set_variables(bucket_name="my-bucket12347ydfs3", enable_acl=False)
/// plan_result = bucket1.plan()
/// print(f"Job ID: {plan_result.job_id}")
/// print(f"Plan output: {plan_result.get_output()}")
///
/// if plan_result.has_destructive_changes():
///     print("Warning: This plan will destroy or replace resources!")
///     destructive = plan_result.get_destructive_changes()
///     for address, action in destructive:
///         print(f"  - {action}: {address}")
///     # Decide whether to proceed with apply
/// ```
#[pyclass(module = "infraweave")]
#[derive(Clone)]
pub struct PlanResult {
    #[pyo3(get)]
    pub job_id: String,
    base: ResultBase,
}

#[pymethods]
impl PlanResult {
    fn __repr__(&self) -> String {
        format!("PlanResult(job_id='{}')", self.job_id)
    }

    /// Gets the plan output from Terraform.
    ///
    /// Returns the full text output from the `terraform plan` command,
    /// which includes details about what changes will be made.
    ///
    /// # Returns
    ///
    /// str: The plan output text.
    ///
    /// # Example
    ///
    /// ```python
    /// plan_result = deployment.plan()
    /// output = plan_result.get_output()
    /// print(output)
    /// ```
    fn get_output(&self) -> PyResult<String> {
        self.base.fetch_output("PLAN")
    }

    /// Checks if the plan contains any destructive changes.
    ///
    /// Returns True if the plan will delete or replace (recreate) any resources,
    /// False otherwise.
    ///
    /// # Returns
    ///
    /// bool: True if destructive changes are present, False otherwise.
    ///
    /// # Example
    ///
    /// ```python
    /// plan_result = deployment.plan()
    /// if plan_result.has_destructive_changes():
    ///     print("Warning: This plan contains destructive changes!")
    /// ```
    fn has_destructive_changes(&self) -> PyResult<bool> {
        Ok(!self.get_destructive_changes()?.is_empty())
    }

    /// Gets detailed information about resources that will be destroyed or replaced.
    ///
    /// Returns a list of tuples containing (resource_address, action) for each
    /// resource that will be deleted or replaced in the plan.
    ///
    /// # Returns
    ///
    /// list[tuple[str, str]]: A list of tuples where each tuple contains:
    ///     - resource_address (str): The Terraform resource address (e.g., "aws_s3_bucket.example")
    ///     - action (str): Either "delete" or "replace"
    ///
    /// # Example
    ///
    /// ```python
    /// plan_result = deployment.plan()
    /// destructive = plan_result.get_destructive_changes()
    /// for address, action in destructive:
    ///     print(f"{action}: {address}")
    /// # Output might be:
    /// # delete: aws_s3_bucket.old
    /// # replace: aws_instance.web
    /// ```
    fn get_destructive_changes(&self) -> PyResult<Vec<(String, String)>> {
        let rt = Runtime::new().unwrap();
        let handler = rt.block_on(GenericCloudHandler::region(self.base.get_region()));

        let terraform_json = match rt.block_on(handler.get_change_record_json(
            self.base.get_environment_id(),
            self.base.get_deployment_id(),
            &self.job_id,
            "PLAN",
        )) {
            Ok(json) => json,
            Err(e) => {
                eprintln!("Warning: Could not fetch change record JSON: {}", e);
                return Ok(Vec::new());
            }
        };

        let changes = env_utils::plan_get_destructive_changes(&terraform_json);
        Ok(changes.into_iter().map(|c| (c.address, c.action)).collect())
    }

    #[getter]
    fn environment_id(&self) -> &str {
        self.base.get_environment_id()
    }

    #[getter]
    fn deployment_id(&self) -> &str {
        self.base.get_deployment_id()
    }

    #[getter]
    fn region(&self) -> &str {
        self.base.get_region()
    }
}

/// Represents the result of an apply or destroy operation.
///
/// DeploymentResult provides methods to fetch the output of the deployment operation.
///
/// # Example
///
/// ```python
/// from infraweave import Deployment, S3Bucket
///
/// bucket_module = S3Bucket(version='0.0.11-dev', track="dev")
/// bucket1 = Deployment(name="bucket1", namespace="playground", module=bucket_module, region="us-west-2")
///
/// bucket1.set_variables(bucket_name="my-bucket12347ydfs3", enable_acl=False)
/// result = bucket1.apply()
/// print(f"Job ID: {result.job_id}")
/// print(f"Apply output: {result.get_output()}")
/// ```
#[pyclass(module = "infraweave")]
#[derive(Clone)]
pub struct DeploymentResult {
    #[pyo3(get)]
    pub job_id: String,
    command: String,
    base: ResultBase,
}

#[pymethods]
impl DeploymentResult {
    fn __repr__(&self) -> String {
        format!(
            "DeploymentResult(job_id='{}', command='{}')",
            self.job_id, self.command
        )
    }

    /// Gets the output from the apply or destroy operation.
    ///
    /// Returns the full text output from the Terraform command that was executed.
    ///
    /// # Returns
    ///
    /// str: The command output text.
    ///
    /// # Example
    ///
    /// ```python
    /// result = deployment.apply()
    /// output = result.get_output()
    /// print(output)
    /// ```
    fn get_output(&self) -> PyResult<String> {
        self.base.fetch_output(&self.command.to_uppercase())
    }

    #[getter]
    fn environment_id(&self) -> &str {
        self.base.get_environment_id()
    }

    #[getter]
    fn deployment_id(&self) -> &str {
        self.base.get_deployment_id()
    }

    #[getter]
    fn region(&self) -> &str {
        self.base.get_region()
    }
}

/// Represents a cloud deployment, exposing Python methods to manage infrastructure.
///
/// This class wraps a module or stack resource and provides methods to apply,
/// plan, and destroy deployments. It also supports context-manager protocol
/// for automatic cleanup.
///
/// # Example
///
/// ```python
/// from infraweave import Deployment, S3Bucket
///
/// # Given that `S3Bucket` is a valid module in your platform
/// bucket_module = S3Bucket(
///     version='0.0.11-dev',
///     track="dev"
/// )
///
/// bucket1 = Deployment(
///     name="bucket1",
///     namespace="playground",
///     module=bucket_module,
///     region="us-west-2"
/// )

/// with bucket1:
///     bucket1.set_variables(
///         bucket_name="my-bucket12347ydfs3",
///         enable_acl=False
///     )
///     result = bucket1.apply()
///     print(f"Job ID: {result.job_id}")
///     print(f"Changes: {result.changes}")
///     # Run some tests here

/// ```
///
#[pyclass(module = "infraweave")]
pub struct Deployment {
    /// Underlying module response for the deployment target
    module: ModuleResp,
    /// Variables to pass into the deployment
    variables: Value,
    /// Flag indicating if the target is a stack
    is_stack: bool,
    /// Name of the deployment
    name: String,
    /// Kubernetes namespace (with Python-specific prefix)
    namespace: String,
    /// Unique deployment identifier (module/stack + name)
    deployment_id: String,
    /// Cloud region for the deployment
    region: String,
    /// Reference string identifying the caller (always "python")
    reference: String,
    /// Information about the last executed deployment operation
    last_deployment: Option<DeploymentResp>,
    /// Whether the last operation resulted in an error
    has_error: bool,
}

#[pymethods]
impl Deployment {
    /// Creates a new Deployment object.
    ///
    /// # Arguments
    /// * `name` - Name for the deployment instance
    /// * `namespace` - Kubernetes namespace (or prefix) to use
    /// * `region` - Cloud region to target
    /// * `module` - Optional Module object for single-resource deployments
    /// * `stack` - Optional Stack object for multi-resource deployments
    ///
    /// Either `module` or `stack` must be provided, but not both.
    #[new]
    #[pyo3(signature = (name, namespace, region, module=None, stack=None))]
    fn new(
        name: String,
        namespace: String,
        region: String,
        module: Option<Bound<PyAny>>,
        stack: Option<Bound<PyAny>>,
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
                    namespace: get_namespace(&namespace),
                    region,
                    name: name.clone(),
                    variables: Value::Null,
                    module: module.module.clone(),
                    is_stack: false,
                    reference,
                    last_deployment: None,
                    has_error: false,
                })
            }
            (None, Some(stack)) => {
                let stack = extract_stack(stack)?;
                Ok(Deployment {
                    deployment_id: format!("{}/{}", stack.module.module, name.clone()),
                    namespace: get_namespace(&namespace),
                    region,
                    name,
                    variables: Value::Null,
                    module: stack.module,
                    is_stack: true,
                    reference,
                    last_deployment: None,
                    has_error: false,
                })
            }
        }
    }

    /// Sets variables for the deployment using keyword arguments.
    ///
    /// Converts Python kwargs to JSON, then stores in the `variables` field.
    #[pyo3(signature = (**kwargs))]
    fn set_variables(&mut self, kwargs: Option<Bound<PyDict>>) -> PyResult<()> {
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
                "Setting variables for deployment {} in namespace {} to:\n{}",
                self.name, self.namespace, value
            );
        } else {
            return Err(PyException::new_err("No variables provided"));
        }
        Ok(())
    }

    /// Sets the version for the module deployment.
    ///
    /// # Arguments
    /// * `track` - The release track (e.g., "stable", "dev") to use.
    /// * `version` - The version string to set for the module.
    ///
    /// # Errors
    /// Returns a `PyException` if the deployment is a stack (not a module), or if the specified version does not exist.
    ///
    /// # Example
    /// ```python
    /// deployment.set_module_version("stable", "1.2.3")
    /// ```
    fn set_module_version(&mut self, track: String, version: String) -> PyResult<()> {
        if self.is_stack {
            return Err(PyException::new_err(
                "Cannot set module version for a stack",
            ));
        }
        // Check if the version is valid
        let rt = Runtime::new().unwrap();
        let exists = rt.block_on(verify_version_exists(
            "module",
            &self.module.module,
            &track,
            &version,
        ))?;
        if !exists {
            return Err(PyException::new_err(format!(
                "Module version {} not found for module {}",
                version, self.module.module
            )));
        }

        self.module.version = version;
        Ok(())
    }

    /// Sets the version for the stack deployment.
    ///
    /// # Arguments
    /// * `track` - The release track (e.g., "stable", "dev") to use.
    /// * `version` - The version string to set for the stack.
    ///
    /// # Errors
    /// Returns a `PyException` if the deployment is a module (not a stack), or if the specified version does not exist.
    ///
    /// # Example
    /// ```python
    /// deployment.set_stack_version("dev", "1.6.2-dev")
    /// ```
    fn set_stack_version(&mut self, track: String, version: String) -> PyResult<()> {
        if !self.is_stack {
            return Err(PyException::new_err(
                "Cannot set stack version for a module",
            ));
        }
        // Check if the version is valid
        let rt = Runtime::new().unwrap();
        let exists = rt.block_on(verify_version_exists(
            "stack",
            &self.module.module,
            &track,
            &version,
        ))?;
        if !exists {
            return Err(PyException::new_err(format!(
                "Module version {} not found for module {}",
                version, self.module.module
            )));
        }

        self.module.version = version;
        Ok(())
    }

    /// Applies the deployment, creating or updating infrastructure.
    ///
    /// Returns a DeploymentResult containing the job ID and changes on success,
    /// or raises DeploymentFailure on error.
    fn apply(&mut self) -> PyResult<DeploymentResult> {
        println!(
            "Applying {} in namespace {} ({})",
            self.name, self.namespace, self.region
        );
        let rt = Runtime::new().unwrap();
        let (job_id, status, deployment) = match rt.block_on(run_job("apply", self)) {
            Ok((job_id, status, deployment)) => (job_id, status, deployment),
            Err(e) => {
                self.has_error = true;
                return Err(DeploymentFailure::new_err(format!(
                    "Failed to run apply for {}: {}",
                    self.deployment_id, e
                )));
            }
        };
        if status != "successful" {
            self.has_error = true;
            return Err(DeploymentFailure::new_err(format!(
                "Apply failed with status: {}, error: {}",
                status,
                deployment
                    .as_ref()
                    .map(|d| d.error_text.clone())
                    .unwrap_or_else(|| "No error message".to_string())
            )));
        }
        self.last_deployment = deployment;
        self.has_error = false;
        Ok(DeploymentResult {
            job_id: job_id.clone(),
            command: "apply".to_string(),
            base: ResultBase {
                job_id,
                environment_id: self.namespace.clone(),
                deployment_id: self.deployment_id.clone(),
                region: self.region.clone(),
            },
        })
    }

    /// Plans the deployment, showing prospective changes without applying.
    ///
    /// Returns a PlanResult containing the job ID and methods to analyze the plan on success,
    /// or raises DeploymentFailure on error.
    fn plan(&self) -> PyResult<PlanResult> {
        println!(
            "Planning {} in namespace {} ({})",
            self.name, self.namespace, self.region
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
        Ok(PlanResult {
            job_id: job_id.clone(),
            base: ResultBase {
                job_id,
                environment_id: self.namespace.clone(),
                deployment_id: self.deployment_id.clone(),
                region: self.region.clone(),
            },
        })
    }

    /// Destroys the deployment, tearing down infrastructure.
    ///
    /// Returns the job ID string on success, or raises DeploymentFailure on error.
    fn destroy(&mut self) -> PyResult<String> {
        println!(
            "Destroying {} in namespace {} ({})",
            self.name, self.namespace, self.region
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
        self.last_deployment = None;
        self.has_error = false;
        Ok((job_id).to_string())
    }

    /// Retrieves the outputs from the last deployment as a Python object.
    ///
    /// ## Example
    /// ```python
    /// >>> from infraweave import Deployment
    /// ...
    /// >>> # (assume `bucket1` has just been applied, and has terraform outputs "bucket_arn" and "tags")
    /// ...
    /// >>> print(bucket1.outputs.bucket_arn)
    /// 'arn:aws:s3:::my-bucket12347ydfs3'
    /// >>> print(bucket1.outputs.tags)
    /// {'Test': 'test-tag123', 'Env': 'test'}
    /// ```
    ///
    /// Returns `None` if no deployment has run yet.
    #[getter]
    fn outputs(&self, py: Python) -> PyResult<PyObject> {
        match &self.last_deployment {
            Some(deployment) => match &deployment.output {
                serde_json::Value::Object(map) => {
                    let types_mod = py.import("types")?;
                    let simple_ns = types_mod.getattr("SimpleNamespace")?;
                    let json_mod = py.import("json")?;
                    let kwargs = PyDict::new(py);
                    for (key, val) in map {
                        if let serde_json::Value::Object(inner) = val {
                            if let Some(field) = inner.get("value") {
                                let val_py =
                                    json_mod.call_method1("loads", (field.to_string(),))?;
                                kwargs.set_item(key, val_py)?;
                            }
                        }
                    }
                    let ns = simple_ns.call((), Some(&kwargs))?;
                    Ok(ns.into())
                }
                _ => Ok(py.None().into()),
            },
            None => Ok(py.None().into()),
        }
    }

    /// Enter the context manager block (`with Deployment(...) as d:`).
    fn __enter__(slf: PyRefMut<Self>) -> PyResult<PyRefMut<Self>> {
        Ok(slf)
    }

    /// Exit the context manager, automatically destroying or cleaning up.
    fn __exit__(
        mut slf: PyRefMut<Self>,
        _exc_type: Option<PyObject>,
        _exc_value: Option<PyObject>,
        _traceback: Option<PyObject>,
    ) -> PyResult<bool> {
        // If a deployment was run or an error occurred, destroy it
        if slf.last_deployment.is_some() || slf.has_error {
            if let Err(e) = slf.destroy() {
                eprintln!("Automatic {}.destroy() failed: {}", slf.name, e);
            }
        }
        Ok(false)
    }
}

/// Normalizes a namespace string by prefixing with `python/` if no slash present.
pub fn get_namespace(namespace_arg: &str) -> String {
    if !namespace_arg.contains('/') {
        format!("python/{}", namespace_arg)
    } else {
        namespace_arg.to_string()
    }
}

/// Internal helper to drive a deployment job to completion.
///
/// Repeatedly polls `is_deployment_in_progress` every 10 seconds until done.
/// Returns (job_id, status, deployment_resp, changes_output)
async fn run_job(
    command: &str,
    deployment: &Deployment,
) -> Result<(String, String, Option<DeploymentResp>), anyhow::Error> {
    let handler = &GenericCloudHandler::region(&deployment.region).await;
    let result = match command {
        "destroy" => {
            destroy_infra(
                handler,
                &deployment.deployment_id,
                &deployment.namespace,
                ExtraData::None,
                None,
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
        let (in_progress, deployment_job_result) = if command == "plan" {
            let (in_progress, _job_id, deployment) = is_deployment_plan_in_progress(
                handler,
                &deployment.deployment_id,
                &deployment.namespace,
                &job_id,
            )
            .await;
            (in_progress, deployment)
        } else {
            let (in_progress, _, _status, deployment) = is_deployment_in_progress(
                handler,
                &deployment.deployment_id,
                &deployment.namespace,
                false,
                true,
            )
            .await;
            (in_progress, deployment)
        };

        if !in_progress {
            let status = match &deployment_job_result {
                Some(deployment_job_result) => deployment_job_result.status.clone(),
                None => "unknown".to_string(),
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

/// Shared logic for `plan` and `apply` commands: constructs the deployment spec
/// and invokes `run_claim` on the cloud handler.
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
            namespace: Some(deployment.namespace.clone()),
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
        &deployment.namespace,
        command,
        vec![],
        ExtraData::None,
        &deployment.reference,
    )
    .await
    {
        Ok((job_id, deployment_id, _)) => (job_id, deployment_id),
        Err(e) => {
            return Err(anyhow::anyhow!(e));
        }
    };
    info!(
        "Deployment id: {}, namespace: {}, job id: {}",
        deployment.deployment_id, deployment.namespace, job_id
    );

    Ok(job_id)
}

/// Extracts a Module from a Python-bound object, handling wrapped attributes.
fn extract_module(obj: Bound<PyAny>) -> PyResult<Module> {
    if let Ok(module_attr) = obj.getattr("module") {
        module_attr.extract()
    } else {
        obj.extract()
    }
}

/// Extracts a Stack from a Python-bound object, handling wrapped attributes.
fn extract_stack(obj: Bound<PyAny>) -> PyResult<Stack> {
    if let Ok(module_attr) = obj.getattr("module") {
        module_attr.extract()
    } else {
        obj.extract()
    }
}

async fn verify_version_exists(
    module_type: &str,
    name: &str,
    track: &str,
    version: &str,
) -> PyResult<bool> {
    let handler = GenericCloudHandler::default().await;
    let result = match module_type {
        "module" => handler.get_module_version(name, track, version).await,
        "stack" => handler.get_stack_version(name, track, version).await,
        _ => panic!("Invalid module type"),
    }
    .map_err(|e| PyException::new_err(format!("Error trying to get module version: {}", e)))?;
    Ok(result.is_some())
}
