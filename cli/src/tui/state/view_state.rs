use crate::tui::app::{Deployment, Module};

#[derive(Debug, Clone, PartialEq)]
pub enum CurrentView {
    Modules,
    Stacks,
    Policies,
    Deployments,
}

pub struct ViewState {
    pub current_view: CurrentView,
    pub selected_index: usize,
    pub modules: Vec<Module>,
    pub stacks: Vec<Module>,
    pub deployments: Vec<Deployment>,
    pub current_track: String,
    pub available_tracks: Vec<String>,
    pub selected_track_index: usize,
    pub last_track_switch: Option<std::time::Instant>,
}

impl ViewState {
    pub fn new() -> Self {
        Self {
            current_view: CurrentView::Modules,
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
        }
    }

    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn move_down(&mut self, max_index: usize) {
        if self.selected_index < max_index {
            self.selected_index += 1;
        }
    }

    pub fn page_up(&mut self) {
        const PAGE_SIZE: usize = 10;
        self.selected_index = self.selected_index.saturating_sub(PAGE_SIZE);
    }

    pub fn page_down(&mut self, max_index: usize) {
        const PAGE_SIZE: usize = 10;
        self.selected_index = std::cmp::min(self.selected_index + PAGE_SIZE, max_index);
    }

    pub fn next_track(&mut self) {
        if self.selected_track_index < self.available_tracks.len() - 1 {
            self.selected_track_index += 1;
            self.current_track = self.available_tracks[self.selected_track_index].clone();
            self.last_track_switch = Some(std::time::Instant::now());
        }
    }

    pub fn previous_track(&mut self) {
        if self.selected_track_index > 0 {
            self.selected_track_index -= 1;
            self.current_track = self.available_tracks[self.selected_track_index].clone();
            self.last_track_switch = Some(std::time::Instant::now());
        }
    }

    pub fn change_view(&mut self, view: CurrentView) {
        if self.current_view != view {
            match &view {
                CurrentView::Modules => self.modules.clear(),
                CurrentView::Stacks => self.stacks.clear(),
                CurrentView::Deployments => self.deployments.clear(),
                _ => {}
            }
            self.current_view = view;
            self.selected_index = 0;
        }
    }
}

impl Default for ViewState {
    fn default() -> Self {
        Self::new()
    }
}
