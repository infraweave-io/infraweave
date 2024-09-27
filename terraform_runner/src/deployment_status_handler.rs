use std::env;

use env_defs::{Dependency, DeploymentResp, EventData};
use serde_json::Value;

pub struct DeploymentStatusHandler<'a> {
    cloud_handler: &'a Box<dyn env_common::ModuleEnvironmentHandler>,
    command: &'a str,
    module: &'a str,
    module_version: &'a str,
    status: &'a str,
    environment: &'a str,
    deployment_id: &'a str,
    error_text: &'a str,
    job_id: &'a str,
    name: &'a str,
    variables: Value,
    deleted: bool,
    dependencies: Vec<Dependency>,
}

impl<'a> DeploymentStatusHandler<'a> {
    // Constructor
    pub fn new(
        cloud_handler: &'a Box<dyn env_common::ModuleEnvironmentHandler>,
        command: &'a str,
        module: &'a str,
        module_version: &'a str,
        status: &'a str,
        environment: &'a str,
        deployment_id: &'a str,
        error_text: &'a str,
        job_id: &'a str,
        name: &'a str,
        variables: Value,
        dependencies: Vec<Dependency>,
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
        }
    }

    pub fn set_status(&mut self, status: &'a str) {
        self.status = status;
    }

    pub fn set_command(&mut self, command: &'a str) {
        self.command = command;
    }

    pub fn set_error_text(&mut self, error_text: &'a str) {
        self.error_text = error_text;
    }

    pub fn set_deleted(&mut self, deleted: bool) {
        self.deleted = deleted;
    }

    pub async fn send_event(
        &self,
        // cloud_handler: &Box<dyn env_common::ModuleEnvironmentHandler>,
        // command: String,
        // module: &String,
        // status: &String,
        // deployment_id: &String,
        // error_text: String,
        // job_id: String,
        // metadata: serde_json::Value,
        // name: &String,
    ) {
        let epoch = std::time::UNIX_EPOCH.elapsed().unwrap().as_millis();
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
            timestamp: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
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
        // cloud_handler: &Box<dyn env_common::ModuleEnvironmentHandler>,
        // deployment_id: &String,
        // status: &String,
        // module: &String,
        // module_version: &String,
        // variables: serde_json::Value,
    ) {
        let epoch = std::time::UNIX_EPOCH.elapsed().unwrap().as_millis();
        let deployment = DeploymentResp {
            epoch: epoch,
            deployment_id: self.deployment_id.to_string(),
            status: self.status.to_string(),
            environment: self.environment.to_string(),
            module: self.module.to_string(),
            module_version: self.module_version.to_string(),
            variables: self.variables.clone(),
            error_text: self.error_text.to_string(),
            deleted: self.deleted,
            dependencies: self.dependencies.clone(),
        };

        match self.cloud_handler.set_deployment(deployment).await {
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
