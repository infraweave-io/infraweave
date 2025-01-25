use env_defs::{Dependency, DeploymentResp, DriftDetection, EventData, PolicyResult};
use env_utils::{get_epoch, get_timestamp};
use humantime::parse_duration;
use log::{debug, error, info};
use serde_json::Value;

use super::CloudHandler;

pub struct DeploymentStatusHandler<'a> {
    command: &'a str,
    module: &'a str,
    module_version: &'a str,
    module_type: &'a str,
    module_track: &'a str,
    status: String,
    environment: &'a str,
    deployment_id: &'a str,
    project_id: &'a str,
    region: &'a str,
    error_text: &'a str,
    job_id: &'a str,
    name: &'a str,
    variables: Value,
    drift_detection: DriftDetection,
    next_drift_check_epoch: i128,
    has_drifted: bool,
    is_drift_check: bool,
    deleted: bool,
    dependencies: Vec<Dependency>,
    output: Value,
    policy_results: Vec<PolicyResult>,
    initiated_by: &'a str,
    last_event_epoch: u128, // During lifetime of this status handler (useful for calculating duration between events)
    event_duration: u128,
    cpu: String,
    memory: String,
    reference: String,
}

impl<'a> DeploymentStatusHandler<'a> {
    // Constructor
    pub fn new(
        command: &'a str,
        module: &'a str,
        module_version: &'a str,
        module_type: &'a str,
        module_track: &'a str,
        status: String,
        environment: &'a str,
        deployment_id: &'a str,
        project_id: &'a str,
        region: &'a str,
        error_text: &'a str,
        job_id: &'a str,
        name: &'a str,
        variables: Value,
        drift_detection: DriftDetection,
        next_drift_check_epoch: i128,
        dependencies: Vec<Dependency>,
        output: Value,
        policy_results: Vec<PolicyResult>,
        initiated_by: &'a str,
        cpu: String,
        memory: String,
        reference: String,
    ) -> Self {
        DeploymentStatusHandler {
            command,
            module,
            module_version,
            module_type,
            module_track,
            status,
            environment,
            deployment_id,
            project_id,
            region,
            error_text,
            job_id,
            name,
            variables,
            drift_detection,
            next_drift_check_epoch,
            has_drifted: false,
            is_drift_check: false,
            deleted: false,
            dependencies,
            output,
            policy_results,
            initiated_by,
            last_event_epoch: 0,
            event_duration: 0,
            cpu,
            memory,
            reference,
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

    pub fn set_drift_has_occurred(&mut self, drift_has_occurred: bool) {
        self.has_drifted = drift_has_occurred;
    }

    pub fn set_is_drift_check(&mut self) {
        self.is_drift_check = true;
    }

    pub fn set_last_event_epoch(&mut self) {
        let epoch: u128 = get_epoch().try_into().unwrap();
        self.last_event_epoch = epoch;
    }

    pub fn set_event_duration(&mut self) {
        let epoch: u128 = get_epoch().try_into().unwrap();
        let duration: u128 = epoch - self.last_event_epoch;
        self.event_duration = duration;
    }

    pub async fn send_event<T: CloudHandler>(&self, handler: &T) {
        let epoch = get_epoch();
        let event = EventData {
            environment: self.environment.to_string(),
            event: self.command.to_string(),
            epoch,
            status: self.status.to_string(),
            module: self.module.to_string(),
            drift_detection: self.drift_detection.clone(),
            next_drift_check_epoch: self.next_drift_check_epoch,
            has_drifted: self.has_drifted,
            deployment_id: self.deployment_id.to_string(),
            project_id: self.project_id.to_string(),
            region: self.region.to_string(),
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
            initiated_by: self.initiated_by.to_string(),
            event_duration: self.event_duration,
        };
        match handler.insert_event(event).await {
            Ok(_) => {
                info!("Event inserted");
            }
            Err(e) => {
                error!("Error inserting event: {}", e);
            }
        }
    }

    fn get_next_drift_check_epoch(&self) -> i128 {
        if !self.drift_detection.enabled || self.drift_detection.interval.is_empty() {
            debug!("Drift detection not enabled");
            return -1;
        }
        if !self.is_final_update() {
            debug!("Not a final update, not scheduling next drift detection yet");
            return -1;
        }
        if self.command == "destroy" {
            debug!("Destroy command, not scheduling next drift detection");
            return -1;
        }
        match parse_duration(&self.drift_detection.interval) {
            Ok(dur) => {
                info!("Final step, deployment either succeeded or failed, scheduling next drift detection");
                debug!(
                    "{} -> {} milliseconds",
                    &self.drift_detection.interval,
                    dur.as_millis()
                );
                let epoch: i128 = get_epoch().try_into().unwrap();
                let wait_duration: i128 = dur.as_millis() as i128;
                debug!(
                    "Current epoch: {} + {} = {}",
                    epoch,
                    wait_duration,
                    epoch + wait_duration
                );
                epoch + wait_duration
            }
            Err(e) => {
                error!("Error parsing {}: {}", &self.drift_detection.interval, e);
                -1
            }
        }
    }

    pub async fn send_deployment<T: CloudHandler>(&self, handler: &T) {
        let deployment = DeploymentResp {
            epoch: get_epoch(),
            deployment_id: self.deployment_id.to_string(),
            project_id: self.project_id.to_string(),
            region: self.region.to_string(),
            status: self.status.to_string(),
            job_id: self.job_id.to_string(),
            environment: self.environment.to_string(),
            module: self.module.to_string(),
            module_version: self.module_version.to_string(),
            module_type: self.module_type.to_string(),
            module_track: self.module_track.to_string(),
            variables: self.variables.clone(),
            drift_detection: self.drift_detection.clone(),
            next_drift_check_epoch: self.get_next_drift_check_epoch(),
            has_drifted: self.has_drifted,
            output: self.output.clone(),
            policy_results: self.policy_results.clone(),
            error_text: self.error_text.to_string(),
            deleted: self.deleted,
            dependencies: self.dependencies.clone(),
            initiated_by: self.initiated_by.to_string(),
            cpu: self.cpu.to_string(),
            memory: self.memory.to_string(),
            reference: self.reference.to_string(),
        };

        match handler.set_deployment(&deployment, self.is_plan()).await {
            Ok(_) => {
                info!("Deployment inserted");
            }
            Err(e) => {
                println!("Error: {:?}", e);
                panic!("Error inserting deployment");
            }
        }

        // If is drift check, also update existing deployment to indicate drift (or in sync)
        if self.is_drift_check && self.is_final_update() {
            match handler.set_deployment(&deployment, false).await {
                Ok(_) => {
                    info!("Drifted deployment inserted");
                }
                Err(e) => {
                    error!("Error: {:?}", e);
                    panic!("Error inserting drifted deployment");
                }
            }
        }
    }

    fn is_plan(&self) -> bool {
        self.command == "plan"
    }

    fn is_final_update(&self) -> bool {
        ["successful", "failed"].contains(&self.status.as_str())
    }
}
