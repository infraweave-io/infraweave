use crate::tui::app::{Module, PendingAction};

pub struct ModalState {
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
}

impl ModalState {
    pub fn new() -> Self {
        Self {
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
        }
    }

    pub fn show_versions_modal(
        &mut self,
        module_name: String,
        track: String,
        track_index: usize,
        available_tracks: Vec<String>,
    ) {
        self.showing_versions_modal = true;
        self.modal_module_name = module_name;
        self.modal_track = track;
        self.modal_track_index = track_index;
        self.modal_available_tracks = available_tracks;
        self.modal_selected_index = 0;
    }

    pub fn close_versions_modal(&mut self) {
        self.showing_versions_modal = false;
        self.modal_versions.clear();
        self.modal_module_name.clear();
        self.modal_available_tracks.clear();
        self.modal_selected_index = 0;
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

    pub fn modal_move_up(&mut self) {
        if self.modal_selected_index > 0 {
            self.modal_selected_index -= 1;
        }
    }

    pub fn modal_move_down(&mut self, max_index: usize) {
        if self.modal_selected_index < max_index {
            self.modal_selected_index += 1;
        }
    }

    pub fn is_any_modal_showing(&self) -> bool {
        self.showing_versions_modal || self.showing_confirmation
    }
}

impl Default for ModalState {
    fn default() -> Self {
        Self::new()
    }
}
