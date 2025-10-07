use anyhow::Result;

use super::state::{
    detail_state::DetailState, events_state::EventsState, modal_state::ModalState,
    search_state::SearchState, view_state::ViewState,
};
use super::utils::NavItem;
use crate::current_region_handler;
use env_defs::{CloudProvider, CloudProviderCommon, ModuleResp};

// Re-export EventsLogView for backward compatibility with existing code
pub use super::state::events_state::EventsLogView;

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Modules,
    Stacks,
    Policies,
    Deployments,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PendingAction {
    None,
    LoadModules,
    LoadStacks,
    LoadDeployments,
    ShowModuleDetail(usize),
    ShowStackDetail(usize),
    ShowDeploymentDetail(usize),
    ShowModuleVersions(usize),
    ShowStackVersions(usize),
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
pub struct GroupedModule {
    pub module: String,
    pub module_name: String,
    pub stable_version: Option<String>,
    pub rc_version: Option<String>,
    pub beta_version: Option<String>,
    pub alpha_version: Option<String>,
    pub dev_version: Option<String>,
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

/// Main application state
///
/// This struct is in transition to a composition-based architecture.
/// State modules (view_state, detail_state, etc.) are the new approach,
/// while individual fields are kept for backward compatibility during migration.
///
/// Eventually, all access should go through the state modules for better
/// testability and maintainability.
pub struct App {
    // ==================== CORE APP STATE ====================
    pub should_quit: bool,
    pub current_view: View,
    pub is_loading: bool,
    pub loading_message: String,
    pub pending_action: PendingAction,

    // ==================== BACKGROUND TASKS ====================
    pub background_sender:
        Option<tokio::sync::mpsc::UnboundedSender<crate::tui::background::BackgroundMessage>>,
    pub last_log_refresh: Option<std::time::Instant>,
    pub auto_refresh_logs: bool,
    pub last_deployments_refresh: Option<std::time::Instant>,
    pub is_refreshing: bool,

    // ==================== STATE MODULES (NEW) ====================
    // These are the future - use these in new code!
    pub view_state: ViewState,
    pub detail_state: DetailState,
    pub events_state: EventsState,
    pub modal_state: ModalState,
    pub search_state: SearchState,

    // ==================== LEGACY FIELDS (TRANSITIONING) ====================
    // These are kept for backward compatibility during migration.
    // New code should use the state modules above instead.
    // TODO: Remove these once all code is migrated to use state modules.

    // View state fields (use view_state instead)
    pub selected_index: usize,
    pub modules: Vec<Module>,
    pub stacks: Vec<Module>,
    pub deployments: Vec<Deployment>,
    pub current_track: String,
    pub available_tracks: Vec<String>,
    pub selected_track_index: usize,
    pub last_track_switch: Option<std::time::Instant>,

    // Detail state fields (use detail_state instead)
    pub showing_detail: bool,
    pub detail_content: String,
    pub detail_module: Option<ModuleResp>,
    pub detail_stack: Option<ModuleResp>,
    pub detail_deployment: Option<env_defs::DeploymentResp>,
    pub detail_nav_items: Vec<NavItem>,
    pub detail_browser_index: usize,
    pub detail_focus_right: bool,
    pub detail_scroll: u16,
    pub detail_visible_lines: u16,
    pub detail_total_lines: u16,
    pub detail_wrap_text: bool,

    // Search state fields (use search_state instead)
    pub search_mode: bool,
    pub search_query: String,

    // Modal state fields (use modal_state instead)
    pub showing_versions_modal: bool,
    pub modal_module_name: String,
    pub modal_track: String,
    pub modal_track_index: usize,
    pub modal_available_tracks: Vec<String>,
    pub modal_versions: Vec<Module>,
    pub modal_selected_index: usize,
    pub showing_confirmation: bool,
    pub confirmation_message: String,
    pub confirmation_deployment_index: Option<usize>,
    pub confirmation_action: PendingAction,

    // Events state fields (use events_state instead)
    pub showing_events: bool,
    pub events_deployment_id: String,
    pub events_data: Vec<env_defs::EventData>,
    pub events_browser_index: usize,
    pub events_scroll: u16,
    pub events_focus_right: bool,
    pub events_logs: Vec<env_defs::LogData>,
    pub events_current_job_id: String,
    pub events_log_view: EventsLogView,
}

impl App {
    pub fn new() -> Self {
        // Initialize state modules
        let view_state = ViewState::new();
        let detail_state = DetailState::new();
        let events_state = EventsState::new();
        let modal_state = ModalState::new();
        let search_state = SearchState::new();

        Self {
            // Core app state
            should_quit: false,
            current_view: View::Modules,
            is_loading: false,
            loading_message: String::new(),
            pending_action: PendingAction::LoadModules,

            // Background tasks
            background_sender: None,
            last_log_refresh: None,
            auto_refresh_logs: true,
            last_deployments_refresh: None,
            is_refreshing: false,

            // State modules (new architecture)
            view_state,
            detail_state,
            events_state,
            modal_state,
            search_state,

            // Legacy fields - initialized from defaults for backward compatibility
            // View state
            selected_index: 0,
            modules: Vec::new(),
            stacks: Vec::new(),
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
            selected_track_index: 0,
            last_track_switch: None,

            // Detail state
            showing_detail: false,
            detail_content: String::new(),
            detail_module: None,
            detail_stack: None,
            detail_deployment: None,
            detail_nav_items: Vec::new(),
            detail_browser_index: 0,
            detail_focus_right: false,
            detail_scroll: 0,
            detail_visible_lines: 0,
            detail_total_lines: 0,
            detail_wrap_text: true,

            // Search state
            search_mode: false,
            search_query: String::new(),

            // Modal state
            showing_versions_modal: false,
            modal_module_name: String::new(),
            modal_track: String::new(),
            modal_track_index: 0,
            modal_available_tracks: Vec::new(),
            modal_versions: Vec::new(),
            modal_selected_index: 0,
            showing_confirmation: false,
            confirmation_message: String::new(),
            confirmation_deployment_index: None,
            confirmation_action: PendingAction::None,

            // Events state
            showing_events: false,
            events_deployment_id: String::new(),
            events_data: Vec::new(),
            events_browser_index: 0,
            events_scroll: 0,
            events_focus_right: false,
            events_logs: Vec::new(),
            events_current_job_id: String::new(),
            events_log_view: EventsLogView::Events,
        }
    }

    pub fn set_background_sender(
        &mut self,
        sender: tokio::sync::mpsc::UnboundedSender<crate::tui::background::BackgroundMessage>,
    ) {
        self.background_sender = Some(sender);
    }

    pub fn process_background_message(
        &mut self,
        message: crate::tui::background::BackgroundMessage,
    ) {
        use crate::tui::background::BackgroundMessage;

        match message {
            BackgroundMessage::ModulesLoaded(result) => {
                match result {
                    Ok(modules) => {
                        self.modules = modules;
                        self.view_state.modules = self.modules.clone();
                    }
                    Err(e) => {
                        eprintln!("Failed to load modules: {}", e);
                    }
                }
                self.clear_loading();
            }
            BackgroundMessage::StacksLoaded(result) => {
                match result {
                    Ok(stacks) => {
                        self.stacks = stacks;
                        self.view_state.stacks = self.stacks.clone();
                    }
                    Err(e) => {
                        eprintln!("Failed to load stacks: {}", e);
                    }
                }
                self.clear_loading();
            }
            BackgroundMessage::DeploymentsLoaded(result) => {
                match result {
                    Ok(mut deployments) => {
                        // Sort by epoch (newest first)
                        deployments.sort_by(|a, b| b.epoch.cmp(&a.epoch));

                        // Preserve user's selection by deployment_id during refresh
                        let selected_deployment_id = if self.selected_index < self.deployments.len()
                        {
                            Some(self.deployments[self.selected_index].deployment_id.clone())
                        } else {
                            None
                        };

                        self.deployments = deployments;
                        self.view_state.deployments = self.deployments.clone();

                        // Restore selection to the same deployment if it still exists
                        if let Some(deployment_id) = selected_deployment_id {
                            if let Some(new_index) = self
                                .deployments
                                .iter()
                                .position(|d| d.deployment_id == deployment_id)
                            {
                                self.selected_index = new_index;
                            } else {
                                // If deployment no longer exists, keep selection within bounds
                                self.selected_index = self
                                    .selected_index
                                    .min(self.deployments.len().saturating_sub(1));
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to load deployments: {}", e);
                    }
                }
                self.clear_loading();
                self.set_refreshing(false); // Clear refreshing state
            }
            BackgroundMessage::JobLogsLoaded(result) => {
                match result {
                    Ok((job_id, logs)) => {
                        self.events_logs = logs;
                        self.events_current_job_id = job_id;
                        self.events_state.events_logs = self.events_logs.clone();
                        self.events_state.events_current_job_id =
                            self.events_current_job_id.clone();
                        self.last_log_refresh = Some(std::time::Instant::now());
                    }
                    Err(e) => {
                        eprintln!("Failed to load logs: {}", e);
                        self.events_logs.clear();
                    }
                }
                self.clear_loading();
                self.set_refreshing(false); // Clear refreshing state
            }
            BackgroundMessage::DeploymentEventsLoaded(result) => {
                match result {
                    Ok((deployment_id, _environment, events)) => {
                        self.events_deployment_id = deployment_id;
                        self.events_data = events;
                        self.events_state.events_deployment_id = self.events_deployment_id.clone();
                        self.events_state.events_data = self.events_data.clone();
                        self.showing_events = true;
                        self.events_state.showing_events = true;
                        self.events_browser_index = 0;
                        self.events_state.events_browser_index = 0;
                        self.events_scroll = 0;
                        self.events_state.events_scroll = 0;
                        self.events_focus_right = false;
                        self.events_state.events_focus_right = false;
                    }
                    Err(e) => {
                        eprintln!("Failed to load deployment events: {}", e);
                    }
                }
                self.clear_loading();
            }
            BackgroundMessage::DeploymentDetailLoaded(result) => {
                match result {
                    Ok(deployment) => {
                        self.detail_deployment = deployment;
                        self.detail_state.detail_deployment = self.detail_deployment.clone();
                        self.showing_detail = true;
                        self.detail_state.showing_detail = true;
                    }
                    Err(e) => {
                        eprintln!("Failed to load deployment detail: {}", e);
                    }
                }
                self.clear_loading();
            }
            // Handle other message types as needed
            _ => {
                // For now, just clear loading for unhandled messages
                self.clear_loading();
            }
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

    pub fn set_refreshing(&mut self, refreshing: bool) {
        self.is_refreshing = refreshing;
    }

    pub fn schedule_action(&mut self, action: PendingAction) {
        self.pending_action = action;
    }

    // ==================== STATE SYNCHRONIZATION HELPERS ====================
    // These methods keep legacy fields in sync with state modules during transition.
    // Call these after modifying state modules to ensure backward compatibility.
    // Once migration is complete, these can be removed.

    /// Sync all state modules to legacy fields
    pub fn sync_to_legacy(&mut self) {
        // View state
        self.selected_index = self.view_state.selected_index;
        self.modules = self.view_state.modules.clone();
        self.stacks = self.view_state.stacks.clone();
        self.deployments = self.view_state.deployments.clone();
        self.current_track = self.view_state.current_track.clone();
        self.available_tracks = self.view_state.available_tracks.clone();
        self.selected_track_index = self.view_state.selected_track_index;
        self.last_track_switch = self.view_state.last_track_switch;

        // Detail state
        self.showing_detail = self.detail_state.showing_detail;
        self.detail_content = self.detail_state.detail_content.clone();
        self.detail_module = self.detail_state.detail_module.clone();
        self.detail_stack = self.detail_state.detail_stack.clone();
        self.detail_deployment = self.detail_state.detail_deployment.clone();
        self.detail_nav_items = self.detail_state.detail_nav_items.clone();
        self.detail_browser_index = self.detail_state.detail_browser_index;
        self.detail_focus_right = self.detail_state.detail_focus_right;
        self.detail_scroll = self.detail_state.detail_scroll;
        self.detail_visible_lines = self.detail_state.detail_visible_lines;
        self.detail_total_lines = self.detail_state.detail_total_lines;
        self.detail_wrap_text = self.detail_state.detail_wrap_text;

        // Search state
        self.search_mode = self.search_state.search_mode;
        self.search_query = self.search_state.search_query.clone();

        // Modal state
        self.showing_versions_modal = self.modal_state.showing_versions_modal;
        self.modal_module_name = self.modal_state.modal_module_name.clone();
        self.modal_track = self.modal_state.modal_track.clone();
        self.modal_track_index = self.modal_state.modal_track_index;
        self.modal_available_tracks = self.modal_state.modal_available_tracks.clone();
        self.modal_versions = self.modal_state.modal_versions.clone();
        self.modal_selected_index = self.modal_state.modal_selected_index;
        self.showing_confirmation = self.modal_state.showing_confirmation;
        self.confirmation_message = self.modal_state.confirmation_message.clone();
        self.confirmation_deployment_index = self.modal_state.confirmation_deployment_index;
        self.confirmation_action = self.modal_state.confirmation_action.clone();

        // Events state
        self.showing_events = self.events_state.showing_events;
        self.events_deployment_id = self.events_state.events_deployment_id.clone();
        self.events_data = self.events_state.events_data.clone();
        self.events_browser_index = self.events_state.events_browser_index;
        self.events_scroll = self.events_state.events_scroll;
        self.events_focus_right = self.events_state.events_focus_right;
        self.events_logs = self.events_state.events_logs.clone();
        self.events_current_job_id = self.events_state.events_current_job_id.clone();
        self.events_log_view = self.events_state.events_log_view.clone();
    }

    /// Sync all legacy fields to state modules
    pub fn sync_from_legacy(&mut self) {
        // View state
        self.view_state.selected_index = self.selected_index;
        self.view_state.modules = self.modules.clone();
        self.view_state.stacks = self.stacks.clone();
        self.view_state.deployments = self.deployments.clone();
        self.view_state.current_track = self.current_track.clone();
        self.view_state.available_tracks = self.available_tracks.clone();
        self.view_state.selected_track_index = self.selected_track_index;
        self.view_state.last_track_switch = self.last_track_switch;

        // Detail state
        self.detail_state.showing_detail = self.showing_detail;
        self.detail_state.detail_content = self.detail_content.clone();
        self.detail_state.detail_module = self.detail_module.clone();
        self.detail_state.detail_stack = self.detail_stack.clone();
        self.detail_state.detail_deployment = self.detail_deployment.clone();
        self.detail_state.detail_nav_items = self.detail_nav_items.clone();
        self.detail_state.detail_browser_index = self.detail_browser_index;
        self.detail_state.detail_focus_right = self.detail_focus_right;
        self.detail_state.detail_scroll = self.detail_scroll;
        self.detail_state.detail_visible_lines = self.detail_visible_lines;
        self.detail_state.detail_total_lines = self.detail_total_lines;
        self.detail_state.detail_wrap_text = self.detail_wrap_text;

        // Search state
        self.search_state.search_mode = self.search_mode;
        self.search_state.search_query = self.search_query.clone();

        // Modal state
        self.modal_state.showing_versions_modal = self.showing_versions_modal;
        self.modal_state.modal_module_name = self.modal_module_name.clone();
        self.modal_state.modal_track = self.modal_track.clone();
        self.modal_state.modal_track_index = self.modal_track_index;
        self.modal_state.modal_available_tracks = self.modal_available_tracks.clone();
        self.modal_state.modal_versions = self.modal_versions.clone();
        self.modal_state.modal_selected_index = self.modal_selected_index;
        self.modal_state.showing_confirmation = self.showing_confirmation;
        self.modal_state.confirmation_message = self.confirmation_message.clone();
        self.modal_state.confirmation_deployment_index = self.confirmation_deployment_index;
        self.modal_state.confirmation_action = self.confirmation_action.clone();

        // Events state
        self.events_state.showing_events = self.showing_events;
        self.events_state.events_deployment_id = self.events_deployment_id.clone();
        self.events_state.events_data = self.events_data.clone();
        self.events_state.events_browser_index = self.events_browser_index;
        self.events_state.events_scroll = self.events_scroll;
        self.events_state.events_focus_right = self.events_focus_right;
        self.events_state.events_logs = self.events_logs.clone();
        self.events_state.events_current_job_id = self.events_current_job_id.clone();
        self.events_state.events_log_view = self.events_log_view.clone();
    }

    pub async fn process_pending_action(&mut self) -> Result<()> {
        let action = self.pending_action.clone();

        self.pending_action = PendingAction::None;

        match action {
            PendingAction::None => {}
            PendingAction::LoadModules => {
                self.load_modules().await?;
            }
            PendingAction::LoadStacks => {
                self.load_stacks().await?;
            }
            PendingAction::LoadDeployments => {
                self.load_deployments().await?;
            }
            PendingAction::ShowModuleDetail(index) => {
                self.selected_index = index;
                self.show_module_detail().await?;
            }
            PendingAction::ShowStackDetail(index) => {
                self.selected_index = index;
                self.show_stack_detail().await?;
            }
            PendingAction::ShowDeploymentDetail(index) => {
                self.selected_index = index;
                self.show_deployment_detail().await?;
            }
            PendingAction::ShowModuleVersions(index) => {
                self.selected_index = index;
                self.show_module_versions().await?;
            }
            PendingAction::ShowStackVersions(index) => {
                self.selected_index = index;
                self.show_stack_versions().await?;
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
        match &self.pending_action {
            PendingAction::None => {}
            PendingAction::LoadModules => {
                self.modules.clear();
                self.set_loading("Loading modules...");
            }
            PendingAction::LoadStacks => {
                self.stacks.clear();
                self.set_loading("Loading stacks...");
            }
            PendingAction::LoadDeployments => {
                self.deployments.clear();
                self.set_loading("Loading deployments...");
            }
            PendingAction::ShowModuleDetail(_) => {
                self.set_loading("Loading module details...");
            }
            PendingAction::ShowStackDetail(_) => {
                self.set_loading("Loading stack details...");
            }
            PendingAction::ShowDeploymentDetail(_) => {
                self.set_loading("Loading deployment details...");
            }
            PendingAction::ShowModuleVersions(_) => {
                self.set_loading("Loading module versions...");
            }
            PendingAction::ShowStackVersions(_) => {
                self.set_loading("Loading stack versions...");
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

        module_list.sort_by(|a, b| a.module_name.cmp(&b.module_name));

        self.modules = module_list;
        self.selected_index = 0;
        self.clear_loading();
        Ok(())
    }

    pub async fn load_stacks(&mut self) -> Result<()> {
        let track_filter = if self.current_track == "all" {
            ""
        } else {
            &self.current_track
        };

        let stacks = current_region_handler()
            .await
            .get_all_latest_stack(track_filter)
            .await?;

        let mut stack_list: Vec<Module> = stacks
            .into_iter()
            .map(|s| Module {
                module: s.module,
                module_name: s.module_name,
                version: s.version,
                track: s.track,
                reference: s.reference,
                timestamp: s.timestamp,
            })
            .collect();

        stack_list.sort_by(|a, b| a.module_name.cmp(&b.module_name));

        self.stacks = stack_list;
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

        deployments_vec.sort_by(|a, b| b.epoch.cmp(&a.epoch));

        self.deployments = deployments_vec;
        self.selected_index = 0;
        self.clear_loading();
        Ok(())
    }

    pub async fn show_module_detail(&mut self) -> Result<()> {
        // Use modal versions if modal is shown, otherwise use filtered modules
        let module = if self.showing_versions_modal {
            self.modal_versions.get(self.modal_selected_index).cloned()
        } else {
            let filtered_modules = self.get_filtered_modules();
            // Get the grouped module and then find the first matching module from the original list
            if let Some(grouped) = filtered_modules.get(self.selected_index) {
                self.modules
                    .iter()
                    .find(|m| m.module_name == grouped.module_name)
                    .cloned()
            } else {
                None
            }
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
                    use super::utils::build_module_nav_items;
                    let nav_items = build_module_nav_items(&module_detail);
                    let content = serde_json::to_string_pretty(&module_detail)?;

                    // Use detail_state instead of legacy fields
                    self.detail_state.show_module(
                        module_detail.clone(),
                        nav_items.clone(),
                        content.clone(),
                    );

                    // Also update legacy fields for backward compatibility with internal methods
                    self.detail_nav_items = nav_items;
                    self.detail_content = content;
                    self.detail_module = Some(module_detail);
                    self.showing_detail = true;
                    self.detail_scroll = 0;

                    if self.showing_versions_modal {
                        self.close_modal();
                    }
                }
                None => {
                    // Use detail_state instead of legacy fields
                    self.detail_state
                        .show_message("Module not found".to_string());

                    // Also update legacy fields for backward compatibility with internal methods
                    self.detail_content = "Module not found".to_string();
                    self.detail_module = None;
                    self.detail_nav_items = Vec::new();
                    self.showing_detail = true;

                    if self.showing_versions_modal {
                        self.close_modal();
                    }
                }
            }

            self.clear_loading();
        }
        Ok(())
    }

    pub async fn show_stack_detail(&mut self) -> Result<()> {
        let stack = if self.showing_versions_modal {
            self.modal_versions.get(self.modal_selected_index).cloned()
        } else {
            let filtered_stacks = self.get_filtered_stacks();
            // Get the grouped stack and then find the first matching stack from the original list
            if let Some(grouped) = filtered_stacks.get(self.selected_index) {
                self.stacks
                    .iter()
                    .find(|s| s.module_name == grouped.module_name)
                    .cloned()
            } else {
                None
            }
        };

        if let Some(stack) = stack {
            // Clone the values we need before borrowing self mutably
            let stack_name = stack.module.clone();
            let stack_track = stack.track.clone();
            let stack_version = stack.version.clone();

            match current_region_handler()
                .await
                .get_stack_version(&stack_name, &stack_track, &stack_version)
                .await?
            {
                Some(stack_detail) => {
                    // Build navigation items for this stack
                    use super::utils::build_stack_nav_items;
                    let nav_items = build_stack_nav_items(&stack_detail);
                    let content = serde_json::to_string_pretty(&stack_detail)?;

                    // Use detail_state instead of legacy fields
                    self.detail_state.show_stack(
                        stack_detail.clone(),
                        nav_items.clone(),
                        content.clone(),
                    );

                    // Also update legacy fields for backward compatibility with internal methods
                    self.detail_nav_items = nav_items;
                    self.detail_content = content;
                    self.detail_stack = Some(stack_detail);
                    self.showing_detail = true;
                    self.detail_scroll = 0;
                    self.detail_browser_index = 0;

                    // Close the modal if it was open
                    if self.showing_versions_modal {
                        self.close_modal();
                    }
                }
                None => {
                    // Use detail_state instead of legacy fields
                    self.detail_state
                        .show_message("Stack not found".to_string());

                    // Also update legacy fields for backward compatibility with internal methods
                    self.detail_content = "Stack not found".to_string();
                    self.detail_stack = None;
                    self.detail_nav_items = Vec::new();
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
                // Build navigation items for this deployment
                use super::utils::build_deployment_nav_items;
                let nav_items = build_deployment_nav_items(&detail);
                let content = serde_json::to_string_pretty(&detail)?;

                // Use detail_state instead of legacy fields
                self.detail_state.show_deployment(
                    detail.clone(),
                    nav_items.clone(),
                    content.clone(),
                );

                // Also update legacy fields for backward compatibility with internal methods
                self.detail_nav_items = nav_items;
                self.detail_deployment = Some(detail);
                self.detail_content = content;
                self.showing_detail = true;
                self.detail_scroll = 0;
                self.detail_browser_index = 0;
                self.detail_focus_right = false;
            } else {
                // Use detail_state instead of legacy fields
                self.detail_state
                    .show_message("Deployment not found".to_string());

                // Also update legacy fields for backward compatibility with internal methods
                self.detail_content = "Deployment not found".to_string();
                self.detail_deployment = None;
                self.detail_nav_items = Vec::new();
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
                            let message = format!(
                                "✅ Deployment reapplied successfully!\n\nJob ID: {}\nDeployment ID: {}\nEnvironment: {}",
                                job_id, deployment_id, environment
                            );

                            // Use detail_state instead of legacy fields
                            self.detail_state.show_message(message.clone());

                            // Also update legacy fields for backward compatibility
                            self.detail_content = message;
                            self.showing_detail = true;
                            self.detail_scroll = 0;

                            // Reload deployments list
                            self.schedule_action(PendingAction::LoadDeployments);
                        }
                        Err(e) => {
                            let message = format!("❌ Failed to reapply deployment:\n\n{}", e);

                            // Use detail_state instead of legacy fields
                            self.detail_state.show_message(message.clone());

                            // Also update legacy fields for backward compatibility
                            self.detail_content = message;
                            self.showing_detail = true;
                            self.detail_scroll = 0;
                        }
                    }
                } else {
                    // Use detail_state instead of legacy fields
                    self.detail_state
                        .show_message("Module not found".to_string());

                    // Also update legacy fields for backward compatibility
                    self.detail_content = "Module not found".to_string();
                    self.showing_detail = true;
                }
            } else {
                // Use detail_state instead of legacy fields
                self.detail_state
                    .show_message("Deployment not found".to_string());

                // Also update legacy fields for backward compatibility
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
                    let message = format!(
                        "✅ Deployment destroy initiated successfully!\n\nDeployment ID: {}\nEnvironment: {}\n\nThe deployment will be destroyed in the background.",
                        deployment_id, environment
                    );

                    // Use detail_state instead of legacy fields
                    self.detail_state.show_message(message.clone());

                    // Also update legacy fields for backward compatibility
                    self.detail_content = message;
                    self.showing_detail = true;
                    self.detail_scroll = 0;

                    // Reload deployments list
                    self.schedule_action(PendingAction::LoadDeployments);
                }
                Err(e) => {
                    let message = format!("❌ Failed to destroy deployment:\n\n{}", e);

                    // Use detail_state instead of legacy fields
                    self.detail_state.show_message(message.clone());

                    // Also update legacy fields for backward compatibility
                    self.detail_content = message;
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
        if let Some(grouped_module) = filtered_modules.get(self.selected_index) {
            // Clone the module name
            let module_name = grouped_module.module.clone();

            // Determine available tracks and initial selection based on current view
            let (modal_track, available_tracks) = if self.current_track == "all" {
                // When "all" is selected, collect tracks that have versions
                let mut module_tracks = Vec::new();
                if grouped_module.stable_version.is_some() {
                    module_tracks.push("stable".to_string());
                }
                if grouped_module.rc_version.is_some() {
                    module_tracks.push("rc".to_string());
                }
                if grouped_module.beta_version.is_some() {
                    module_tracks.push("beta".to_string());
                }
                if grouped_module.alpha_version.is_some() {
                    module_tracks.push("alpha".to_string());
                }
                if grouped_module.dev_version.is_some() {
                    module_tracks.push("dev".to_string());
                }

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

            // Use modal_state instead of legacy fields
            self.modal_state.show_versions_modal(
                module_name,
                modal_track,
                modal_track_index,
                available_tracks,
            );

            // Also update legacy fields for backward compatibility with internal methods
            self.modal_module_name = self.modal_state.modal_module_name.clone();
            self.modal_track = self.modal_state.modal_track.clone();
            self.modal_track_index = self.modal_state.modal_track_index;
            self.modal_available_tracks = self.modal_state.modal_available_tracks.clone();
            self.modal_selected_index = 0;
            self.showing_versions_modal = true;

            // Load versions for this module and track
            self.schedule_action(PendingAction::LoadModalVersions);
            self.clear_loading();
        }
        Ok(())
    }

    pub async fn show_stack_versions(&mut self) -> Result<()> {
        let filtered_stacks = self.get_filtered_stacks();
        if let Some(grouped_stack) = filtered_stacks.get(self.selected_index) {
            // Clone the stack name
            let stack_name = grouped_stack.module.clone();

            // Determine available tracks and initial selection based on current view
            let (modal_track, available_tracks) = if self.current_track == "all" {
                // When "all" is selected, collect tracks that have versions
                let mut stack_tracks = Vec::new();
                if grouped_stack.stable_version.is_some() {
                    stack_tracks.push("stable".to_string());
                }
                if grouped_stack.rc_version.is_some() {
                    stack_tracks.push("rc".to_string());
                }
                if grouped_stack.beta_version.is_some() {
                    stack_tracks.push("beta".to_string());
                }
                if grouped_stack.alpha_version.is_some() {
                    stack_tracks.push("alpha".to_string());
                }
                if grouped_stack.dev_version.is_some() {
                    stack_tracks.push("dev".to_string());
                }

                // Select the first available track (prefer stable, rc, beta, alpha, dev order)
                let preferred_order = ["stable", "rc", "beta", "alpha", "dev"];
                let first_track = preferred_order
                    .iter()
                    .find(|&&track| stack_tracks.contains(&track.to_string()))
                    .map(|&s| s.to_string())
                    .unwrap_or_else(|| {
                        stack_tracks
                            .first()
                            .cloned()
                            .unwrap_or("stable".to_string())
                    });

                (first_track, stack_tracks)
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

            // Use modal_state instead of legacy fields
            self.modal_state.show_versions_modal(
                stack_name,
                modal_track,
                modal_track_index,
                available_tracks,
            );

            // Also update legacy fields for backward compatibility with internal methods
            self.modal_module_name = self.modal_state.modal_module_name.clone();
            self.modal_track = self.modal_state.modal_track.clone();
            self.modal_track_index = self.modal_state.modal_track_index;
            self.modal_available_tracks = self.modal_state.modal_available_tracks.clone();
            self.modal_selected_index = 0;
            self.showing_versions_modal = true;

            // Load versions for this stack and track
            self.schedule_action(PendingAction::LoadModalVersions);
            self.clear_loading();
        }
        Ok(())
    }

    pub async fn load_modal_versions(&mut self) -> Result<()> {
        // Load versions based on current view (modules or stacks)
        let versions = if matches!(self.current_view, View::Stacks) {
            current_region_handler()
                .await
                .get_all_stack_versions(&self.modal_module_name, &self.modal_track)
                .await?
        } else {
            current_region_handler()
                .await
                .get_all_module_versions(&self.modal_module_name, &self.modal_track)
                .await?
        };

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
                View::Stacks => self.get_filtered_stacks().len().saturating_sub(1),
                View::Deployments => self.get_filtered_deployments().len().saturating_sub(1),
                _ => 0,
            }
        } else {
            match self.current_view {
                View::Modules => self.modules.len().saturating_sub(1),
                View::Stacks => self.stacks.len().saturating_sub(1),
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
            View::Stacks => self.stacks.len().saturating_sub(1),
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

    pub fn get_max_detail_scroll(&self) -> u16 {
        // Calculate the maximum scroll position
        // We want to prevent scrolling past the end of the content
        // The max scroll is total lines minus visible lines
        // Add a small buffer to account for line wrapping (20% of total lines)
        if self.detail_total_lines <= self.detail_visible_lines {
            0
        } else {
            let base_max = self
                .detail_total_lines
                .saturating_sub(self.detail_visible_lines);
            // Add 20% buffer for wrapped lines
            let buffer = self.detail_total_lines / 5;
            base_max.saturating_add(buffer)
        }
    }

    pub fn detail_browser_up(&mut self) {
        if self.detail_browser_index > 0 {
            self.detail_browser_index -= 1;
            self.detail_scroll = 0; // Reset scroll when changing item
            self.check_and_load_logs_if_needed();
        }
    }

    pub fn detail_browser_down(&mut self) {
        let max_index = self.detail_nav_items.len().saturating_sub(1);

        if self.detail_browser_index < max_index {
            self.detail_browser_index += 1;
            self.detail_scroll = 0; // Reset scroll when changing item
            self.check_and_load_logs_if_needed();
        }
    }

    /// Check if we're viewing the Logs section of a deployment and trigger loading if needed
    pub fn check_and_load_logs_if_needed(&mut self) {
        if let Some(deployment) = &self.detail_deployment {
            // Calculate the index of the Logs section
            let logs_index = self.calculate_logs_section_index();

            // If we're on the Logs section and haven't loaded logs for this job yet
            if self.detail_browser_index == logs_index {
                let job_id = deployment.job_id.clone();
                // Only schedule if we haven't loaded this job's logs yet
                if self.events_current_job_id != job_id {
                    self.schedule_action(PendingAction::LoadJobLogs(job_id));
                }
            }
        }
    }

    /// Calculate which browser index corresponds to the Logs section
    pub fn calculate_logs_section_index(&self) -> usize {
        if let Some(deployment) = &self.detail_deployment {
            let mut idx = 1; // Start after General

            // Variables section
            if !deployment.variables.is_null() && deployment.variables.is_object() {
                if let Some(obj) = deployment.variables.as_object() {
                    if !obj.is_empty() {
                        idx += 1;
                    }
                }
            }

            // Outputs section
            if !deployment.output.is_null() && deployment.output.is_object() {
                if let Some(obj) = deployment.output.as_object() {
                    if !obj.is_empty() {
                        idx += 1;
                    }
                }
            }

            // Dependencies section
            if !deployment.dependencies.is_empty() {
                idx += 1;
            }

            // Policy Results section
            if !deployment.policy_results.is_empty() {
                idx += 1;
            }

            // Logs section is at this index
            idx
        } else {
            0
        }
    }

    pub fn detail_focus_left(&mut self) {
        self.detail_focus_right = false;
    }

    pub fn detail_focus_right(&mut self) {
        self.detail_focus_right = true;
    }

    pub fn toggle_detail_wrap(&mut self) {
        self.detail_wrap_text = !self.detail_wrap_text;
    }

    pub fn close_detail(&mut self) {
        // Use detail_state instead of legacy fields
        self.detail_state.close();

        // Also update legacy fields for backward compatibility with internal methods
        self.showing_detail = false;
        self.detail_scroll = 0;
        self.detail_browser_index = 0;
        self.detail_focus_right = false;
        self.detail_module = None;
        self.detail_stack = None;
        self.detail_deployment = None;
        self.detail_nav_items.clear();
        self.detail_total_lines = 0;
    }

    pub async fn show_deployment_events(
        &mut self,
        deployment_id: String,
        environment: String,
    ) -> Result<()> {
        // Use events_state instead of legacy fields
        self.events_state.show_events(deployment_id.clone());

        // Also update legacy fields for backward compatibility with internal methods
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
        // Use events_state instead of legacy fields
        self.events_state.close();

        // Also update legacy fields for backward compatibility with internal methods
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
        self.load_logs_for_job_with_options(job_id, true).await
    }

    pub async fn load_logs_for_job_with_options(
        &mut self,
        job_id: &str,
        show_loading: bool,
    ) -> Result<()> {
        if let Some(sender) = &self.background_sender {
            let job_id = job_id.to_string();
            let sender_clone = sender.clone();

            // Only clear logs and show loading for initial load, not auto-refresh
            if show_loading {
                self.events_logs.clear();
                self.set_loading("Loading logs...");
            }
            self.events_current_job_id = job_id.clone();

            // Spawn background task to load logs
            tokio::spawn(async move {
                let result = current_region_handler().await.read_logs(&job_id).await;
                let message = match result {
                    Ok(logs) => {
                        crate::tui::background::BackgroundMessage::JobLogsLoaded(Ok((job_id, logs)))
                    }
                    Err(e) => {
                        crate::tui::background::BackgroundMessage::JobLogsLoaded(Err(e.to_string()))
                    }
                };
                let _ = sender_clone.send(message);
            });

            Ok(())
        } else {
            // Fallback to blocking mode if no sender
            self.events_current_job_id = job_id.to_string();

            if show_loading {
                self.events_logs.clear();
                self.set_loading("Loading logs...");
            }

            match current_region_handler().await.read_logs(job_id).await {
                Ok(logs) => {
                    self.events_logs = logs;
                    if show_loading {
                        self.clear_loading();
                    }
                    Ok(())
                }
                Err(e) => {
                    if show_loading {
                        self.events_logs.clear();
                        self.clear_loading();
                    }
                    eprintln!("Warning: Failed to load logs for job {}: {}", job_id, e);
                    Ok(())
                }
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
        // Use modal_state instead of legacy fields
        self.modal_state.close_versions_modal();

        // Also update legacy fields for backward compatibility with internal methods
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
                View::Stacks => {
                    self.stacks.clear();
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
        // Use search_state instead of legacy fields
        self.search_state.enter_search_mode();

        // Also update legacy fields for backward compatibility with internal methods
        self.search_mode = true;
        self.search_query.clear();
        self.selected_index = 0;
    }

    pub fn exit_search_mode(&mut self) {
        // Use search_state instead of legacy fields
        self.search_state.exit_search_mode();

        // Also update legacy fields for backward compatibility with internal methods
        self.search_mode = false;
        self.search_query.clear();
        self.selected_index = 0;
    }

    pub fn search_input(&mut self, c: char) {
        // Use search_state instead of legacy fields
        self.search_state.input(c);

        // Also update legacy fields for backward compatibility with internal methods
        self.search_query.push(c);
        self.selected_index = 0;
    }

    pub fn search_backspace(&mut self) {
        // Use search_state instead of legacy fields
        self.search_state.backspace();

        // Also update legacy fields for backward compatibility with internal methods
        self.search_query.pop();
        self.selected_index = 0;
    }

    pub fn get_filtered_modules(&self) -> Vec<GroupedModule> {
        use std::collections::HashMap;

        // First, apply search filter if needed
        let filtered: Vec<&Module> = if self.search_mode && !self.search_query.is_empty() {
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
        };

        // Group modules by module_name
        let mut grouped_map: HashMap<String, GroupedModule> = HashMap::new();

        for module in filtered {
            grouped_map
                .entry(module.module_name.clone())
                .and_modify(|gm| {
                    // Update the version for the appropriate track
                    match module.track.as_str() {
                        "stable" => gm.stable_version = Some(module.version.clone()),
                        "rc" => gm.rc_version = Some(module.version.clone()),
                        "beta" => gm.beta_version = Some(module.version.clone()),
                        "alpha" => gm.alpha_version = Some(module.version.clone()),
                        "dev" => gm.dev_version = Some(module.version.clone()),
                        _ => {}
                    }
                })
                .or_insert_with(|| {
                    let mut gm = GroupedModule {
                        module: module.module.clone(),
                        module_name: module.module_name.clone(),
                        stable_version: None,
                        rc_version: None,
                        beta_version: None,
                        alpha_version: None,
                        dev_version: None,
                    };
                    // Set the version for the current track
                    match module.track.as_str() {
                        "stable" => gm.stable_version = Some(module.version.clone()),
                        "rc" => gm.rc_version = Some(module.version.clone()),
                        "beta" => gm.beta_version = Some(module.version.clone()),
                        "alpha" => gm.alpha_version = Some(module.version.clone()),
                        "dev" => gm.dev_version = Some(module.version.clone()),
                        _ => {}
                    }
                    gm
                });
        }

        // Convert to vec and sort by module name
        let mut result: Vec<GroupedModule> = grouped_map.into_values().collect();
        result.sort_by(|a, b| a.module_name.cmp(&b.module_name));

        result
    }

    pub fn get_filtered_stacks(&self) -> Vec<GroupedModule> {
        use std::collections::HashMap;

        // First, apply search filter if needed
        let filtered: Vec<&Module> = if self.search_mode && !self.search_query.is_empty() {
            let query_lower = self.search_query.to_lowercase();
            self.stacks
                .iter()
                .filter(|s| {
                    s.module_name.to_lowercase().contains(&query_lower)
                        || s.module.to_lowercase().contains(&query_lower)
                        || s.version.to_lowercase().contains(&query_lower)
                        || s.track.to_lowercase().contains(&query_lower)
                })
                .collect()
        } else {
            self.stacks.iter().collect()
        };

        // Group stacks by module_name
        let mut grouped_map: HashMap<String, GroupedModule> = HashMap::new();

        for stack in filtered {
            grouped_map
                .entry(stack.module_name.clone())
                .and_modify(|gm| {
                    // Update the version for the appropriate track
                    match stack.track.as_str() {
                        "stable" => gm.stable_version = Some(stack.version.clone()),
                        "rc" => gm.rc_version = Some(stack.version.clone()),
                        "beta" => gm.beta_version = Some(stack.version.clone()),
                        "alpha" => gm.alpha_version = Some(stack.version.clone()),
                        "dev" => gm.dev_version = Some(stack.version.clone()),
                        _ => {}
                    }
                })
                .or_insert_with(|| {
                    let mut gm = GroupedModule {
                        module: stack.module.clone(),
                        module_name: stack.module_name.clone(),
                        stable_version: None,
                        rc_version: None,
                        beta_version: None,
                        alpha_version: None,
                        dev_version: None,
                    };
                    // Set the version for the current track
                    match stack.track.as_str() {
                        "stable" => gm.stable_version = Some(stack.version.clone()),
                        "rc" => gm.rc_version = Some(stack.version.clone()),
                        "beta" => gm.beta_version = Some(stack.version.clone()),
                        "alpha" => gm.alpha_version = Some(stack.version.clone()),
                        "dev" => gm.dev_version = Some(stack.version.clone()),
                        _ => {}
                    }
                    gm
                });
        }

        // Convert to vec and sort by module name
        let mut result: Vec<GroupedModule> = grouped_map.into_values().collect();
        result.sort_by(|a, b| a.module_name.cmp(&b.module_name));

        result
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
        self.confirmation_message = message.clone();
        self.confirmation_deployment_index = Some(deployment_index);
        self.confirmation_action = action.clone();

        // Also update modal_state directly
        self.modal_state
            .show_confirmation(message, deployment_index, action);
    }

    pub fn close_confirmation(&mut self) {
        self.showing_confirmation = false;
        self.confirmation_message.clear();
        self.confirmation_deployment_index = None;
        self.confirmation_action = PendingAction::None;

        // Also update modal_state directly
        self.modal_state.close_confirmation();
    }

    pub fn confirm_action(&mut self) {
        if self.confirmation_deployment_index.is_some() {
            let action = self.confirmation_action.clone();
            self.schedule_action(action);
        }
        self.close_confirmation();
    }
}

// Implement VersionItem trait for Module to work with VersionsModal widget
impl crate::tui::widgets::modal::VersionItem for Module {
    fn get_version(&self) -> &str {
        &self.version
    }

    fn get_timestamp(&self) -> &str {
        &self.timestamp
    }
}
