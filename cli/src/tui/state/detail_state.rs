use crate::tui::utils::NavItem;
use env_defs::{DeploymentResp, ModuleResp};

pub struct DetailState {
    pub showing_detail: bool,
    pub detail_content: String,
    pub detail_module: Option<ModuleResp>,
    pub detail_stack: Option<ModuleResp>,
    pub detail_deployment: Option<DeploymentResp>,
    pub detail_nav_items: Vec<NavItem>,
    pub detail_browser_index: usize,
    pub detail_focus_right: bool,
    pub detail_scroll: u16,
    pub detail_visible_lines: u16,
    pub detail_total_lines: u16,
    pub detail_wrap_text: bool,
}

impl DetailState {
    pub fn new() -> Self {
        Self {
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
        }
    }

    pub fn close(&mut self) {
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

    pub fn show_module(&mut self, module: ModuleResp, nav_items: Vec<NavItem>, content: String) {
        self.showing_detail = true;
        self.detail_module = Some(module);
        self.detail_stack = None;
        self.detail_deployment = None;
        self.detail_nav_items = nav_items;
        self.detail_content = content;
        self.detail_scroll = 0;
        self.detail_browser_index = 0;
        self.detail_focus_right = false;
    }

    pub fn show_stack(&mut self, stack: ModuleResp, nav_items: Vec<NavItem>, content: String) {
        self.showing_detail = true;
        self.detail_stack = Some(stack);
        self.detail_module = None;
        self.detail_deployment = None;
        self.detail_nav_items = nav_items;
        self.detail_content = content;
        self.detail_scroll = 0;
        self.detail_browser_index = 0;
        self.detail_focus_right = false;
    }

    pub fn show_deployment(
        &mut self,
        deployment: DeploymentResp,
        nav_items: Vec<NavItem>,
        content: String,
    ) {
        self.showing_detail = true;
        self.detail_deployment = Some(deployment);
        self.detail_module = None;
        self.detail_stack = None;
        self.detail_nav_items = nav_items;
        self.detail_content = content;
        self.detail_scroll = 0;
        self.detail_browser_index = 0;
        self.detail_focus_right = false;
    }

    pub fn show_message(&mut self, message: String) {
        self.showing_detail = true;
        self.detail_content = message;
        self.detail_module = None;
        self.detail_stack = None;
        self.detail_deployment = None;
        self.detail_nav_items.clear();
        self.detail_scroll = 0;
        self.detail_browser_index = 0;
    }

    pub fn scroll_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        let max_scroll = self.get_max_scroll();
        self.detail_scroll = std::cmp::min(self.detail_scroll.saturating_add(1), max_scroll);
    }

    pub fn scroll_page_up(&mut self) {
        const PAGE_SIZE: u16 = 10;
        self.detail_scroll = self.detail_scroll.saturating_sub(PAGE_SIZE);
    }

    pub fn scroll_page_down(&mut self) {
        const PAGE_SIZE: u16 = 10;
        let max_scroll = self.get_max_scroll();
        self.detail_scroll =
            std::cmp::min(self.detail_scroll.saturating_add(PAGE_SIZE), max_scroll);
    }

    pub fn get_max_scroll(&self) -> u16 {
        if self.detail_total_lines <= self.detail_visible_lines {
            0
        } else {
            let base_max = self
                .detail_total_lines
                .saturating_sub(self.detail_visible_lines);
            let buffer = self.detail_total_lines / 5;
            base_max.saturating_add(buffer)
        }
    }

    pub fn browser_up(&mut self) {
        if self.detail_browser_index > 0 {
            self.detail_browser_index -= 1;
            self.detail_scroll = 0;
        }
    }

    pub fn browser_down(&mut self) {
        let max_index = self.detail_nav_items.len().saturating_sub(1);
        if self.detail_browser_index < max_index {
            self.detail_browser_index += 1;
            self.detail_scroll = 0;
        }
    }

    pub fn toggle_wrap(&mut self) {
        self.detail_wrap_text = !self.detail_wrap_text;
    }

    pub fn focus_left(&mut self) {
        self.detail_focus_right = false;
    }

    pub fn focus_right(&mut self) {
        self.detail_focus_right = true;
    }

    pub fn calculate_logs_section_index(&self) -> usize {
        if let Some(deployment) = &self.detail_deployment {
            let mut idx = 1;

            if !deployment.variables.is_null() && deployment.variables.is_object() {
                if let Some(obj) = deployment.variables.as_object() {
                    if !obj.is_empty() {
                        idx += 1;
                    }
                }
            }

            if !deployment.output.is_null() && deployment.output.is_object() {
                if let Some(obj) = deployment.output.as_object() {
                    if !obj.is_empty() {
                        idx += 1;
                    }
                }
            }

            if !deployment.dependencies.is_empty() {
                idx += 1;
            }

            if !deployment.policy_results.is_empty() {
                idx += 1;
            }

            idx
        } else {
            0
        }
    }
}

impl Default for DetailState {
    fn default() -> Self {
        Self::new()
    }
}
