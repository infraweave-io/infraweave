use anyhow::Result;

use crate::current_region_handler;
use env_defs::{CloudProvider, CloudProviderCommon, ModuleResp};

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Modules,
    Stacks,
    Policies,
    Deployments,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EventsLogView {
    Events,
    Logs,
    Changelog,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PendingAction {
    None,
    LoadModules,
    LoadDeployments,
    ShowModuleDetail(usize),
    ShowDeploymentDetail(usize),
    ShowModuleVersions(usize),
    LoadModalVersions,
    ShowDeploymentEvents(usize),
    LoadJobLogs(String),
    ReapplyDeployment(usize),
    DestroyDeployment(usize),
}

#[derive(Debug, Clone)]
pub struct Module {
    pub module: String,
    pub module_name: String,
    pub version: String,
    pub track: String,
    pub reference: String,
    pub timestamp: String,
}

#[derive(Debug, Clone)]
pub struct Deployment {
    pub status: String,
    pub deployment_id: String,
    pub module: String,
    pub module_version: String,
    pub environment: String,
    pub epoch: u128,
    pub timestamp: String,
}

pub struct App {
    pub should_quit: bool,
    pub current_view: View,
    pub selected_index: usize,
    pub showing_detail: bool,
    pub modules: Vec<Module>,
    pub deployments: Vec<Deployment>,
    pub current_track: String,
    pub available_tracks: Vec<String>,
    pub selected_track_index: usize,
    pub detail_content: String,
    pub detail_module: Option<ModuleResp>,
    pub detail_browser_index: usize,
    pub detail_focus_right: bool, // true = right pane (detail), false = left pane (browser)
    pub detail_scroll: u16,
    pub detail_visible_lines: u16,
    pub detail_total_lines: u16,
    pub is_loading: bool,
    pub loading_message: String,
    pub pending_action: PendingAction,
    pub last_track_switch: Option<std::time::Instant>,
    pub search_mode: bool,
    pub search_query: String,
    pub showing_versions_modal: bool,
    pub modal_module_name: String,
    pub modal_track: String,
    pub modal_track_index: usize,
    pub modal_available_tracks: Vec<String>,
    pub modal_versions: Vec<Module>,
    pub modal_selected_index: usize,
    pub showing_events: bool,
    pub events_deployment_id: String,
    pub events_data: Vec<env_defs::EventData>,
    pub events_browser_index: usize,
    pub events_scroll: u16,
    pub events_focus_right: bool, // true = right pane (logs), false = left pane (job list)
    pub events_logs: Vec<env_defs::LogData>,
    pub events_current_job_id: String,
    pub events_log_view: EventsLogView,
    pub showing_confirmation: bool,
    pub confirmation_message: String,
    pub confirmation_deployment_index: Option<usize>,
    pub confirmation_action: PendingAction,
}

impl App {
    pub fn new() -> Self {
        Self {
            should_quit: false,
            current_view: View::Modules,
            selected_index: 0,
            showing_detail: false,
            modules: Vec::new(),
            deployments: Vec::new(),
            current_track: "all".to_string(),
            available_tracks: vec![
                "all".to_string(),
                "stable".to_string(),
                "rc".to_string(),
                "beta".to_string(),
                "alpha".to_string(),
                "dev".to_string(),
            ],
            selected_track_index: 0, // Start with "all" (first in the list)
            detail_content: String::new(),
            detail_module: None,
            detail_browser_index: 0,
            detail_focus_right: false, // Start with left pane (browser) focused
            detail_scroll: 0,
            detail_visible_lines: 20, // Default estimate, will be updated during rendering
            detail_total_lines: 0,
            is_loading: false,
            loading_message: String::new(),
            pending_action: PendingAction::LoadModules, // Start by loading modules
            last_track_switch: None,
            search_mode: false,
            search_query: String::new(),
            showing_versions_modal: false,
            modal_module_name: String::new(),
            modal_track: String::new(),
            modal_track_index: 0,
            modal_available_tracks: Vec::new(),
            modal_versions: Vec::new(),
            modal_selected_index: 0,
            showing_events: false,
            events_deployment_id: String::new(),
            events_data: Vec::new(),
            events_browser_index: 0,
            events_scroll: 0,
            events_focus_right: false, // Start with left pane (job list) focused
            events_logs: Vec::new(),
            events_current_job_id: String::new(),
            events_log_view: EventsLogView::Events,
            showing_confirmation: false,
            confirmation_message: String::new(),
            confirmation_deployment_index: None,
            confirmation_action: PendingAction::None,
        }
    }

    pub fn set_loading(&mut self, message: &str) {
        self.is_loading = true;
        self.loading_message = message.to_string();
    }

    pub fn clear_loading(&mut self) {
        self.is_loading = false;
        self.loading_message.clear();
    }

    pub fn schedule_action(&mut self, action: PendingAction) {
        self.pending_action = action;
    }

    pub async fn process_pending_action(&mut self) -> Result<()> {
        let action = self.pending_action.clone();

        // Clear pending action immediately to avoid reprocessing
        self.pending_action = PendingAction::None;

        match action {
            PendingAction::None => {
                // Nothing to do
            }
            PendingAction::LoadModules => {
                self.load_modules().await?;
            }
            PendingAction::LoadDeployments => {
                self.load_deployments().await?;
            }
            PendingAction::ShowModuleDetail(index) => {
                self.selected_index = index;
                self.show_module_detail().await?;
            }
            PendingAction::ShowDeploymentDetail(index) => {
                self.selected_index = index;
                self.show_deployment_detail().await?;
            }
            PendingAction::ShowModuleVersions(index) => {
                self.selected_index = index;
                self.show_module_versions().await?;
            }
            PendingAction::LoadModalVersions => {
                self.load_modal_versions().await?;
            }
            PendingAction::ShowDeploymentEvents(index) => {
                self.selected_index = index;
                let filtered_deployments = self.get_filtered_deployments();
                if let Some(deployment) = filtered_deployments.get(index) {
                    let deployment_id = deployment.deployment_id.clone();
                    let environment = deployment.environment.clone();
                    self.show_deployment_events(deployment_id, environment)
                        .await?;
                }
            }
            PendingAction::LoadJobLogs(job_id) => {
                self.load_logs_for_job(&job_id).await?;
            }
            PendingAction::ReapplyDeployment(index) => {
                self.selected_index = index;
                self.reapply_deployment().await?;
            }
            PendingAction::DestroyDeployment(index) => {
                self.selected_index = index;
                self.destroy_deployment().await?;
            }
        }

        Ok(())
    }

    pub fn has_pending_action(&self) -> bool {
        self.pending_action != PendingAction::None
    }

    pub fn prepare_pending_action(&mut self) {
        // Set loading state based on the pending action
        match &self.pending_action {
            PendingAction::None => {}
            PendingAction::LoadModules => {
                self.modules.clear();
                self.set_loading("Loading modules...");
            }
            PendingAction::LoadDeployments => {
                self.deployments.clear();
                self.set_loading("Loading deployments...");
            }
            PendingAction::ShowModuleDetail(_) => {
                self.set_loading("Loading module details...");
            }
            PendingAction::ShowDeploymentDetail(_) => {
                self.set_loading("Loading deployment details...");
            }
            PendingAction::ShowModuleVersions(_) => {
                self.set_loading("Loading module versions...");
            }
            PendingAction::LoadModalVersions => {
                self.modal_versions.clear();
                self.set_loading("Loading versions...");
            }
            PendingAction::ShowDeploymentEvents(_) => {
                self.set_loading("Loading deployment events...");
            }
            PendingAction::LoadJobLogs(_) => {
                self.set_loading("Loading job logs...");
            }
            PendingAction::ReapplyDeployment(_) => {
                self.set_loading("Reapplying deployment...");
            }
            PendingAction::DestroyDeployment(_) => {
                self.set_loading("Destroying deployment...");
            }
        }
    }

    pub async fn load_modules(&mut self) -> Result<()> {
        // Use empty string for "all" track to get modules from all tracks
        let track_filter = if self.current_track == "all" {
            ""
        } else {
            &self.current_track
        };

        let modules = current_region_handler()
            .await
            .get_all_latest_module(track_filter)
            .await?;

        let mut module_list: Vec<Module> = modules
            .into_iter()
            .map(|m| Module {
                module: m.module,
                module_name: m.module_name,
                version: m.version,
                track: m.track,
                reference: m.reference,
                timestamp: m.timestamp,
            })
            .collect();

        // Sort modules alphabetically by name for consistent display
        module_list.sort_by(|a, b| a.module_name.cmp(&b.module_name));

        self.modules = module_list;
        self.selected_index = 0;
        self.clear_loading();
        Ok(())
    }

    pub async fn load_deployments(&mut self) -> Result<()> {
        let deployments = current_region_handler()
            .await
            .get_all_deployments("")
            .await?;

        let mut deployments_vec: Vec<Deployment> = deployments
            .into_iter()
            .map(|d| {
                let timestamp = if d.epoch > 0 {
                    let secs = (d.epoch / 1000) as i64;
                    chrono::DateTime::from_timestamp(secs, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_else(|| "Unknown".to_string())
                } else {
                    "Unknown".to_string()
                };

                Deployment {
                    status: d.status,
                    deployment_id: d.deployment_id,
                    module: d.module,
                    module_version: d.module_version,
                    environment: d.environment,
                    epoch: d.epoch,
                    timestamp,
                }
            })
            .collect();

        // Sort by epoch descending (newest first)
        deployments_vec.sort_by(|a, b| b.epoch.cmp(&a.epoch));

        self.deployments = deployments_vec;
        self.selected_index = 0;
        self.clear_loading();
        Ok(())
    }

    pub async fn show_module_detail(&mut self) -> Result<()> {
        // Use modal versions if modal is shown, otherwise use filtered modules
        let module = if self.showing_versions_modal {
            self.modal_versions.get(self.modal_selected_index)
        } else {
            let filtered_modules = self.get_filtered_modules();
            filtered_modules.get(self.selected_index).copied()
        };

        if let Some(module) = module {
            // Clone the values we need before borrowing self mutably
            let module_name = module.module.clone();
            let module_track = module.track.clone();
            let module_version = module.version.clone();

            match current_region_handler()
                .await
                .get_module_version(&module_name, &module_track, &module_version)
                .await?
            {
                Some(module_detail) => {
                    // Store both the structured data and JSON for fallback
                    self.detail_content = serde_json::to_string_pretty(&module_detail)?;
                    self.detail_module = Some(module_detail);
                    self.showing_detail = true;
                    self.detail_scroll = 0;

                    // Close the modal if it was open
                    if self.showing_versions_modal {
                        self.close_modal();
                    }
                }
                None => {
                    self.detail_content = "Module not found".to_string();
                    self.detail_module = None;
                    self.showing_detail = true;

                    // Close the modal if it was open
                    if self.showing_versions_modal {
                        self.close_modal();
                    }
                }
            }

            self.clear_loading();
        }
        Ok(())
    }

    pub async fn show_deployment_detail(&mut self) -> Result<()> {
        let filtered_deployments = self.get_filtered_deployments();
        if let Some(deployment) = filtered_deployments.get(self.selected_index) {
            // Clone the values we need before borrowing self mutably
            let deployment_id = deployment.deployment_id.clone();
            let environment = deployment.environment.clone();

            let (deployment_detail, _) = current_region_handler()
                .await
                .get_deployment_and_dependents(&deployment_id, &environment, false)
                .await?;

            if let Some(detail) = deployment_detail {
                self.detail_content = serde_json::to_string_pretty(&detail)?;
                self.showing_detail = true;
                self.detail_scroll = 0;
            } else {
                self.detail_content = "Deployment not found".to_string();
                self.showing_detail = true;
            }

            self.clear_loading();
        }
        Ok(())
    }

    pub async fn reapply_deployment(&mut self) -> Result<()> {
        use env_common::logic::run_claim;
        use env_defs::ExtraData;

        let filtered_deployments = self.get_filtered_deployments();
        if let Some(deployment) = filtered_deployments.get(self.selected_index) {
            // Clone the values we need
            let deployment_id = deployment.deployment_id.clone();
            let environment = deployment.environment.clone();

            // Get the deployment details
            let deployment_detail = current_region_handler()
                .await
                .get_deployment(&deployment_id, &environment, false)
                .await?;

            if let Some(detail) = deployment_detail {
                // Get the module details
                let module = current_region_handler()
                    .await
                    .get_module_version(
                        &detail.module,
                        &detail.module_track,
                        &detail.module_version,
                    )
                    .await?;

                if let Some(module) = module {
                    // Generate the deployment claim using the utility function
                    let claim_yaml = env_utils::generate_deployment_claim(&detail, &module);

                    // Parse the claim YAML
                    let yaml: serde_yaml::Value = serde_yaml::from_str(&claim_yaml)?;

                    let reference_fallback = match hostname::get() {
                        Ok(hostname) => hostname.to_string_lossy().to_string(),
                        Err(e) => {
                            return Err(anyhow::anyhow!("Failed to get hostname: {}", e));
                        }
                    };

                    // Apply the deployment
                    let handler = current_region_handler().await;
                    match run_claim(
                        &handler,
                        &yaml,
                        &environment,
                        "apply",
                        vec![],
                        ExtraData::None,
                        &reference_fallback,
                    )
                    .await
                    {
                        Ok((job_id, deployment_id, _)) => {
                            self.detail_content = format!(
                                "✅ Deployment reapplied successfully!\n\nJob ID: {}\nDeployment ID: {}\nEnvironment: {}",
                                job_id, deployment_id, environment
                            );
                            self.showing_detail = true;
                            self.detail_scroll = 0;

                            // Reload deployments list
                            self.schedule_action(PendingAction::LoadDeployments);
                        }
                        Err(e) => {
                            self.detail_content =
                                format!("❌ Failed to reapply deployment:\n\n{}", e);
                            self.showing_detail = true;
                            self.detail_scroll = 0;
                        }
                    }
                } else {
                    self.detail_content = "Module not found".to_string();
                    self.showing_detail = true;
                }
            } else {
                self.detail_content = "Deployment not found".to_string();
                self.showing_detail = true;
            }

            self.clear_loading();
        }
        Ok(())
    }

    pub async fn destroy_deployment(&mut self) -> Result<()> {
        use env_common::logic::destroy_infra;
        use env_defs::ExtraData;

        let filtered_deployments = self.get_filtered_deployments();
        if let Some(deployment) = filtered_deployments.get(self.selected_index) {
            // Clone the values we need
            let deployment_id = deployment.deployment_id.clone();
            let environment = deployment.environment.clone();

            // Destroy the deployment
            match destroy_infra(
                &current_region_handler().await,
                &deployment_id,
                &environment,
                ExtraData::None,
                None, // version
            )
            .await
            {
                Ok(_) => {
                    self.detail_content = format!(
                        "✅ Deployment destroy initiated successfully!\n\nDeployment ID: {}\nEnvironment: {}\n\nThe deployment will be destroyed in the background.",
                        deployment_id, environment
                    );
                    self.showing_detail = true;
                    self.detail_scroll = 0;

                    // Reload deployments list
                    self.schedule_action(PendingAction::LoadDeployments);
                }
                Err(e) => {
                    self.detail_content = format!("❌ Failed to destroy deployment:\n\n{}", e);
                    self.showing_detail = true;
                    self.detail_scroll = 0;
                }
            }

            self.clear_loading();
        }
        Ok(())
    }

    pub async fn show_module_versions(&mut self) -> Result<()> {
        let filtered_modules = self.get_filtered_modules();
        if let Some(module) = filtered_modules.get(self.selected_index) {
            // Clone the module name
            let module_name = module.module.clone();

            // Determine available tracks and initial selection based on current view
            let (modal_track, available_tracks) = if self.current_track == "all" {
                // When "all" is selected, find which tracks have versions for this module
                // by looking at the modules list (which shows one version per track)
                let module_tracks: Vec<String> = self
                    .modules
                    .iter()
                    .filter(|m| m.module == module_name)
                    .map(|m| m.track.clone())
                    .collect();

                // Select the first available track (prefer stable, rc, beta, alpha, dev order)
                let preferred_order = ["stable", "rc", "beta", "alpha", "dev"];
                let first_track = preferred_order
                    .iter()
                    .find(|&&track| module_tracks.contains(&track.to_string()))
                    .map(|&s| s.to_string())
                    .unwrap_or_else(|| {
                        module_tracks
                            .first()
                            .cloned()
                            .unwrap_or("stable".to_string())
                    });

                (first_track, module_tracks)
            } else {
                // When a specific track is selected, use that track and enable all tracks
                (self.current_track.clone(), vec![])
            };

            // Find the index of the modal track in available_tracks
            let modal_track_index = self
                .available_tracks
                .iter()
                .position(|t| t == &modal_track)
                .unwrap_or(1); // Default to index 1 (stable) if not found

            self.modal_module_name = module_name;
            self.modal_track = modal_track;
            self.modal_track_index = modal_track_index;
            self.modal_available_tracks = available_tracks;
            self.modal_selected_index = 0;
            self.showing_versions_modal = true;

            // Load versions for this module and track
            self.schedule_action(PendingAction::LoadModalVersions);
            self.clear_loading();
        }
        Ok(())
    }

    pub async fn load_modal_versions(&mut self) -> Result<()> {
        let versions = current_region_handler()
            .await
            .get_all_module_versions(&self.modal_module_name, &self.modal_track)
            .await?;

        let mut versions: Vec<Module> = versions
            .into_iter()
            .map(|m| Module {
                module: m.module,
                module_name: m.module_name,
                version: m.version,
                track: m.track,
                reference: m.reference,
                timestamp: m.timestamp,
            })
            .collect();

        // Sort by timestamp in descending order (newest first)
        versions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        self.modal_versions = versions;
        self.modal_selected_index = 0;
        self.clear_loading();
        Ok(())
    }

    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn move_down(&mut self) {
        let max_index = if self.search_mode || !self.search_query.is_empty() {
            match self.current_view {
                View::Modules => self.get_filtered_modules().len().saturating_sub(1),
                View::Deployments => self.get_filtered_deployments().len().saturating_sub(1),
                _ => 0,
            }
        } else {
            match self.current_view {
                View::Modules => self.modules.len().saturating_sub(1),
                View::Deployments => self.deployments.len().saturating_sub(1),
                _ => 0,
            }
        };
        if self.selected_index < max_index {
            self.selected_index += 1;
        }
    }

    pub fn page_up(&mut self) {
        // Move up by 10 items (approximately one page)
        const PAGE_SIZE: usize = 10;
        self.selected_index = self.selected_index.saturating_sub(PAGE_SIZE);
    }

    pub fn page_down(&mut self) {
        // Move down by 10 items (approximately one page)
        const PAGE_SIZE: usize = 10;
        let max_index = match self.current_view {
            View::Modules => self.modules.len().saturating_sub(1),
            View::Deployments => self.deployments.len().saturating_sub(1),
            _ => 0,
        };
        self.selected_index = std::cmp::min(self.selected_index + PAGE_SIZE, max_index);
    }

    pub fn scroll_detail_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(1);
    }

    pub fn scroll_detail_down(&mut self) {
        let max_scroll = self.get_max_detail_scroll();
        self.detail_scroll = std::cmp::min(self.detail_scroll.saturating_add(1), max_scroll);
    }

    pub fn scroll_detail_page_up(&mut self) {
        // Scroll up by 10 lines in detail view
        const PAGE_SIZE: u16 = 10;
        self.detail_scroll = self.detail_scroll.saturating_sub(PAGE_SIZE);
    }

    pub fn scroll_detail_page_down(&mut self) {
        // Scroll down by 10 lines in detail view
        const PAGE_SIZE: u16 = 10;
        let max_scroll = self.get_max_detail_scroll();
        self.detail_scroll =
            std::cmp::min(self.detail_scroll.saturating_add(PAGE_SIZE), max_scroll);
    }

    fn get_max_detail_scroll(&self) -> u16 {
        // Calculate the maximum scroll position
        // We want to prevent scrolling past the end of the content
        // Use the actual visible lines from the rendered area
        // Add some extra padding to account for text wrapping (long lines become multiple rendered lines)
        let base_max = self
            .detail_total_lines
            .saturating_sub(self.detail_visible_lines);
        // Add 50% extra to account for wrapped lines
        base_max + (self.detail_total_lines / 2)
    }

    pub fn detail_browser_up(&mut self) {
        if self.detail_browser_index > 0 {
            self.detail_browser_index -= 1;
            self.detail_scroll = 0; // Reset scroll when changing item
        }
    }

    pub fn detail_browser_down(&mut self) {
        if let Some(module) = &self.detail_module {
            // Calculate max index: General (1) + Variables category (1) + Variable items + Outputs category (1) + Output items
            let mut max_index = 1; // Start with General

            if !module.tf_variables.is_empty() {
                max_index += 1; // Variables category header
                max_index += module.tf_variables.len(); // Individual variables
            }

            if !module.tf_outputs.is_empty() {
                max_index += 1; // Outputs category header
                max_index += module.tf_outputs.len(); // Individual outputs
            }

            if self.detail_browser_index < max_index.saturating_sub(1) {
                self.detail_browser_index += 1;
                self.detail_scroll = 0; // Reset scroll when changing item
            }
        }
    }

    pub fn detail_focus_left(&mut self) {
        self.detail_focus_right = false;
    }

    pub fn detail_focus_right(&mut self) {
        self.detail_focus_right = true;
    }

    pub fn close_detail(&mut self) {
        self.showing_detail = false;
        self.detail_scroll = 0;
        self.detail_browser_index = 0;
        self.detail_focus_right = false;
        self.detail_module = None;
        self.detail_total_lines = 0;
    }

    pub async fn show_deployment_events(
        &mut self,
        deployment_id: String,
        environment: String,
    ) -> Result<()> {
        self.showing_events = true;
        self.events_deployment_id = deployment_id.clone();
        self.events_browser_index = 0;
        self.events_scroll = 0;
        self.set_loading("Loading deployment events...");

        match current_region_handler()
            .await
            .get_events(&deployment_id, &environment)
            .await
        {
            Ok(events) => {
                // Sort events by epoch (oldest first for chronological order)
                let mut sorted_events = events;
                sorted_events.sort_by(|a, b| a.epoch.cmp(&b.epoch));
                self.events_data = sorted_events;
                self.clear_loading();
                Ok(())
            }
            Err(e) => {
                self.events_data.clear();
                self.clear_loading();
                Err(e)
            }
        }
    }

    pub fn close_events(&mut self) {
        self.showing_events = false;
        self.events_deployment_id.clear();
        self.events_data.clear();
        self.events_browser_index = 0;
        self.events_scroll = 0;
        self.events_focus_right = false;
        self.events_logs.clear();
        self.events_current_job_id.clear();
        self.events_log_view = EventsLogView::Events;
    }

    pub fn events_browser_up(&mut self) {
        if self.events_browser_index > 0 {
            self.events_browser_index -= 1;
            self.events_scroll = 0; // Reset scroll when changing job
            self.events_log_view = EventsLogView::Events; // Switch back to Events view
        }
    }

    pub fn events_browser_down(&mut self) {
        // Group events by job_id to get the count
        let job_count = self.get_grouped_events().len();
        if job_count > 0 && self.events_browser_index < job_count.saturating_sub(1) {
            self.events_browser_index += 1;
            self.events_scroll = 0; // Reset scroll when changing job
            self.events_log_view = EventsLogView::Events; // Switch back to Events view
        }
    }

    pub async fn load_logs_for_job(&mut self, job_id: &str) -> Result<()> {
        // Only reload if it's a different job
        if self.events_current_job_id == job_id {
            self.clear_loading();
            return Ok(());
        }

        self.events_current_job_id = job_id.to_string();

        match current_region_handler().await.read_logs(job_id).await {
            Ok(logs) => {
                self.events_logs = logs;
                self.clear_loading();
                Ok(())
            }
            Err(e) => {
                // Don't fail, just clear logs and continue
                self.events_logs.clear();
                self.clear_loading();
                // Log the error but don't propagate it
                eprintln!("Warning: Failed to load logs for job {}: {}", job_id, e);
                Ok(())
            }
        }
    }

    pub fn get_grouped_events(&self) -> Vec<(String, Vec<&env_defs::EventData>)> {
        use std::collections::HashMap;

        let mut jobs: HashMap<String, Vec<&env_defs::EventData>> = HashMap::new();

        for event in &self.events_data {
            jobs.entry(event.job_id.clone())
                .or_insert_with(Vec::new)
                .push(event);
        }

        // Convert to sorted vec (by first event epoch in each job, most recent first)
        let mut job_list: Vec<(String, Vec<&env_defs::EventData>)> = jobs.into_iter().collect();
        job_list.sort_by(|a, b| {
            let a_epoch = a.1.first().map(|e| e.epoch).unwrap_or(0);
            let b_epoch = b.1.first().map(|e| e.epoch).unwrap_or(0);
            b_epoch.cmp(&a_epoch) // Reversed to show most recent first
        });

        job_list
    }

    pub fn scroll_events_up(&mut self) {
        self.events_scroll = self.events_scroll.saturating_sub(1);
    }

    pub fn scroll_events_down(&mut self) {
        let max_scroll = self
            .detail_total_lines
            .saturating_sub(self.detail_visible_lines);
        self.events_scroll = std::cmp::min(self.events_scroll.saturating_add(1), max_scroll);
    }

    pub fn scroll_events_page_up(&mut self) {
        const PAGE_SIZE: u16 = 10;
        self.events_scroll = self.events_scroll.saturating_sub(PAGE_SIZE);
    }

    pub fn scroll_events_page_down(&mut self) {
        const PAGE_SIZE: u16 = 10;
        let max_scroll = self
            .detail_total_lines
            .saturating_sub(self.detail_visible_lines);
        self.events_scroll =
            std::cmp::min(self.events_scroll.saturating_add(PAGE_SIZE), max_scroll);
    }

    pub fn events_toggle_focus(&mut self) {
        self.events_focus_right = !self.events_focus_right;
    }

    pub fn events_focus_left(&mut self) {
        self.events_focus_right = false;
    }

    pub fn events_focus_right(&mut self) {
        self.events_focus_right = true;
    }

    pub fn events_log_view_next(&mut self) {
        self.events_log_view = match self.events_log_view {
            EventsLogView::Events => EventsLogView::Logs,
            EventsLogView::Logs => EventsLogView::Changelog,
            EventsLogView::Changelog => EventsLogView::Events,
        };
        self.events_scroll = 0; // Reset scroll when changing view
    }

    pub fn events_log_view_previous(&mut self) {
        self.events_log_view = match self.events_log_view {
            EventsLogView::Events => EventsLogView::Changelog,
            EventsLogView::Logs => EventsLogView::Events,
            EventsLogView::Changelog => EventsLogView::Logs,
        };
        self.events_scroll = 0; // Reset scroll when changing view
    }

    pub fn close_modal(&mut self) {
        self.showing_versions_modal = false;
        self.modal_versions.clear();
        self.modal_module_name.clear();
        self.modal_available_tracks.clear();
        self.modal_selected_index = 0;
    }

    pub fn modal_move_up(&mut self) {
        if self.modal_selected_index > 0 {
            self.modal_selected_index -= 1;
        }
    }

    pub fn modal_move_down(&mut self) {
        let max_index = self.modal_versions.len().saturating_sub(1);
        if self.modal_selected_index < max_index {
            self.modal_selected_index += 1;
        }
    }

    pub fn modal_next_track(&mut self) {
        // Find the next available track
        let mut next_index = self.modal_track_index + 1;
        while next_index < self.available_tracks.len() {
            let track = &self.available_tracks[next_index];
            // Skip "all" and unavailable tracks
            if track != "all" && self.modal_available_tracks.contains(track) {
                self.modal_track_index = next_index;
                self.modal_track = track.clone();
                // Non-blocking: don't automatically reload, user must press 'r' to reload
                return;
            }
            next_index += 1;
        }
        // If we didn't find any, stay where we are
    }

    pub fn modal_previous_track(&mut self) {
        // Find the previous available track
        if self.modal_track_index > 0 {
            let mut prev_index = self.modal_track_index - 1;
            loop {
                let track = &self.available_tracks[prev_index];
                // Skip "all" and unavailable tracks
                if track != "all" && self.modal_available_tracks.contains(track) {
                    self.modal_track_index = prev_index;
                    self.modal_track = track.clone();
                    // Non-blocking: don't automatically reload, user must press 'r' to reload
                    return;
                }
                if prev_index == 0 {
                    break;
                }
                prev_index -= 1;
            }
        }
        // If we didn't find any, stay where we are
    }

    pub fn modal_reload_versions(&mut self) {
        self.schedule_action(PendingAction::LoadModalVersions);
    }

    pub fn next_track(&mut self) {
        if self.selected_track_index < self.available_tracks.len() - 1 {
            self.selected_track_index += 1;
            self.current_track = self.available_tracks[self.selected_track_index].clone();
            // Record the time of track switch for debounced reload
            self.last_track_switch = Some(std::time::Instant::now());
        }
    }

    pub fn previous_track(&mut self) {
        if self.selected_track_index > 0 {
            self.selected_track_index -= 1;
            self.current_track = self.available_tracks[self.selected_track_index].clone();
            // Record the time of track switch for debounced reload
            self.last_track_switch = Some(std::time::Instant::now());
        }
    }

    pub fn check_track_switch_timeout(&mut self) {
        if let Some(switch_time) = self.last_track_switch {
            if switch_time.elapsed() >= std::time::Duration::from_secs(1) {
                // It's been 1 second since the last track switch, trigger reload
                self.last_track_switch = None;
                if matches!(self.current_view, View::Modules) && !self.is_loading {
                    self.schedule_action(PendingAction::LoadModules);
                }
            }
        }
    }

    pub fn change_view(&mut self, view: View) {
        if self.current_view != view {
            // Clear old data when changing views to avoid showing stale data
            match &view {
                View::Modules => {
                    self.modules.clear();
                }
                View::Deployments => {
                    self.deployments.clear();
                }
                _ => {}
            }
            self.current_view = view;
            self.selected_index = 0;
            self.showing_detail = false;
        }
    }

    pub fn enter_search_mode(&mut self) {
        self.search_mode = true;
        self.search_query.clear();
        self.selected_index = 0;
    }

    pub fn exit_search_mode(&mut self) {
        self.search_mode = false;
        self.search_query.clear();
        self.selected_index = 0;
    }

    pub fn search_input(&mut self, c: char) {
        self.search_query.push(c);
        self.selected_index = 0;
    }

    pub fn search_backspace(&mut self) {
        self.search_query.pop();
        self.selected_index = 0;
    }

    pub fn get_filtered_modules(&self) -> Vec<&Module> {
        // Only filter when in search mode with a non-empty query
        if self.search_mode && !self.search_query.is_empty() {
            let query_lower = self.search_query.to_lowercase();
            self.modules
                .iter()
                .filter(|m| {
                    m.module_name.to_lowercase().contains(&query_lower)
                        || m.module.to_lowercase().contains(&query_lower)
                        || m.version.to_lowercase().contains(&query_lower)
                        || m.track.to_lowercase().contains(&query_lower)
                })
                .collect()
        } else {
            self.modules.iter().collect()
        }
    }

    pub fn get_filtered_deployments(&self) -> Vec<&Deployment> {
        // Only filter when in search mode with a non-empty query
        if self.search_mode && !self.search_query.is_empty() {
            let query_lower = self.search_query.to_lowercase();
            self.deployments
                .iter()
                .filter(|d| {
                    d.module.to_lowercase().contains(&query_lower)
                        || d.module_version.to_lowercase().contains(&query_lower)
                        || d.environment.to_lowercase().contains(&query_lower)
                        || d.deployment_id.to_lowercase().contains(&query_lower)
                })
                .collect()
        } else {
            self.deployments.iter().collect()
        }
    }

    pub fn show_confirmation(
        &mut self,
        message: String,
        deployment_index: usize,
        action: PendingAction,
    ) {
        self.showing_confirmation = true;
        self.confirmation_message = message;
        self.confirmation_deployment_index = Some(deployment_index);
        self.confirmation_action = action;
    }

    pub fn close_confirmation(&mut self) {
        self.showing_confirmation = false;
        self.confirmation_message.clear();
        self.confirmation_deployment_index = None;
        self.confirmation_action = PendingAction::None;
    }

    pub fn confirm_action(&mut self) {
        if self.confirmation_deployment_index.is_some() {
            let action = self.confirmation_action.clone();
            self.schedule_action(action);
        }
        self.close_confirmation();
    }
}
