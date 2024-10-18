use env_defs::{Dependency, DeploymentResp, EventData, PolicyResult};
use env_utils::{get_epoch, get_timestamp};
use serde_json::Value;
use crate::interface::ModuleEnvironmentHandler;

pub struct DeploymentStatusHandler<'a> {
    cloud_handler: &'a Box<dyn ModuleEnvironmentHandler>,
    command: &'a str,
    module: &'a str,
    module_version: &'a str,
    status: String,
    environment: &'a str,
    deployment_id: &'a str,
    error_text: &'a str,
    job_id: &'a str,
    name: &'a str,
    variables: Value,
    deleted: bool,
    dependencies: Vec<Dependency>,
    output: Value,
    policy_results: Vec<PolicyResult>,
}

impl<'a> DeploymentStatusHandler<'a> {
    // Constructor
    pub fn new(
        cloud_handler: &'a Box<dyn ModuleEnvironmentHandler>,
        command: &'a str,
        module: &'a str,
        module_version: &'a str,
        status: String,
        environment: &'a str,
        deployment_id: &'a str,
        error_text: &'a str,
        job_id: &'a str,
        name: &'a str,
        variables: Value,
        dependencies: Vec<Dependency>,
        output: Value,
        policy_results: Vec<PolicyResult>,
    ) -> Self {
        DeploymentStatusHandler {
            cloud_handler,
            command,
            module,
            module_version,
            status,
            environment,
            deployment_id,
            error_text,
            job_id,
            name,
            variables,
            deleted: false,
            dependencies,
            output,
            policy_results,
        }
    }

    pub fn set_status(&mut self, status: String) {
        self.status = status;
    }

    pub fn set_command(&mut self, command: &'a str) {
        self.command = command;
    }

    pub fn set_output(&mut self, output: Value) {
        self.output = output;
    }

    pub fn set_error_text(&mut self, error_text: &'a str) {
        self.error_text = error_text;
    }

    pub fn set_deleted(&mut self, deleted: bool) {
        self.deleted = deleted;
    }

    pub fn set_policy_results(&mut self, policy_results: Vec<PolicyResult>) {
        self.policy_results = policy_results;
    }

    pub async fn send_event(
        &self,
    ) {
        let epoch = get_epoch();
        let event = EventData {
            event: self.command.to_string(),
            epoch: epoch,
            status: self.status.to_string(),
            module: self.module.to_string(),
            deployment_id: self.deployment_id.to_string(),
            error_text: self.error_text.to_string(),
            id: format!(
                "{}-{}-{}-{}-{}",
                self.module, self.deployment_id, epoch, self.command, self.status
            ),
            job_id: self.job_id.to_string(),
            metadata: serde_json::Value::Null,
            name: self.name.to_string(),
            output: self.output.clone(),
            policy_results: self.policy_results.clone(),
            timestamp: get_timestamp(),
        };

        match self.cloud_handler.insert_event(event).await {
            Ok(_) => {
                println!("Event inserted");
            }
            Err(e) => {
                println!("Error: {:?}", e);
                panic!("Error inserting event");
            }
        }
    }

    pub async fn send_deployment(
        &self,
    ) {
        let epoch = std::time::UNIX_EPOCH.elapsed().unwrap().as_millis();
        let deployment = DeploymentResp {
            epoch: epoch,
            deployment_id: self.deployment_id.to_string(),
            status: self.status.to_string(),
            job_id: self.job_id.to_string(),
            environment: self.environment.to_string(),
            module: self.module.to_string(),
            module_version: self.module_version.to_string(),
            variables: self.variables.clone(),
            output: self.output.clone(),
            policy_results: self.policy_results.clone(),
            error_text: self.error_text.to_string(),
            deleted: self.deleted,
            dependencies: self.dependencies.clone(),
        };

        let is_plan = self.command == "plan";
        match self.cloud_handler.set_deployment(deployment, is_plan).await {
            Ok(_) => {
                println!("Deployment inserted");
            }
            Err(e) => {
                println!("Error: {:?}", e);
                panic!("Error inserting deployment");
            }
        }
    }
}
