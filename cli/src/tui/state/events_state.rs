use env_defs::{EventData, LogData};

#[derive(Debug, Clone, PartialEq)]
pub enum EventsLogView {
    Events,
    Logs,
    Changelog,
}

pub struct EventsState {
    pub showing_events: bool,
    pub events_deployment_id: String,
    pub events_data: Vec<EventData>,
    pub events_browser_index: usize,
    pub events_scroll: u16,
    pub events_focus_right: bool,
    pub events_logs: Vec<LogData>,
    pub events_current_job_id: String,
    pub events_log_view: EventsLogView,
}

impl EventsState {
    pub fn new() -> Self {
        Self {
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

    pub fn show_events(&mut self, deployment_id: String) {
        self.showing_events = true;
        self.events_deployment_id = deployment_id;
        self.events_browser_index = 0;
        self.events_scroll = 0;
    }

    pub fn close(&mut self) {
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

    pub fn browser_up(&mut self) {
        if self.events_browser_index > 0 {
            self.events_browser_index -= 1;
            self.events_scroll = 0;
            self.events_log_view = EventsLogView::Events;
        }
    }

    pub fn browser_down(&mut self, max_index: usize) {
        if max_index > 0 && self.events_browser_index < max_index.saturating_sub(1) {
            self.events_browser_index += 1;
            self.events_scroll = 0;
            self.events_log_view = EventsLogView::Events;
        }
    }

    pub fn scroll_up(&mut self) {
        self.events_scroll = self.events_scroll.saturating_sub(1);
    }

    pub fn scroll_down(&mut self, max_scroll: u16) {
        self.events_scroll = std::cmp::min(self.events_scroll.saturating_add(1), max_scroll);
    }

    pub fn scroll_page_up(&mut self) {
        const PAGE_SIZE: u16 = 10;
        self.events_scroll = self.events_scroll.saturating_sub(PAGE_SIZE);
    }

    pub fn scroll_page_down(&mut self, max_scroll: u16) {
        const PAGE_SIZE: u16 = 10;
        self.events_scroll =
            std::cmp::min(self.events_scroll.saturating_add(PAGE_SIZE), max_scroll);
    }

    pub fn toggle_focus(&mut self) {
        self.events_focus_right = !self.events_focus_right;
    }

    pub fn focus_left(&mut self) {
        self.events_focus_right = false;
    }

    pub fn focus_right(&mut self) {
        self.events_focus_right = true;
    }

    pub fn next_log_view(&mut self) {
        self.events_log_view = match self.events_log_view {
            EventsLogView::Events => EventsLogView::Logs,
            EventsLogView::Logs => EventsLogView::Changelog,
            EventsLogView::Changelog => EventsLogView::Events,
        };
        self.events_scroll = 0;
    }

    pub fn previous_log_view(&mut self) {
        self.events_log_view = match self.events_log_view {
            EventsLogView::Events => EventsLogView::Changelog,
            EventsLogView::Logs => EventsLogView::Events,
            EventsLogView::Changelog => EventsLogView::Logs,
        };
        self.events_scroll = 0;
    }

    pub fn get_grouped_events(&self) -> Vec<(String, Vec<&EventData>)> {
        use std::collections::HashMap;

        let mut jobs: HashMap<String, Vec<&EventData>> = HashMap::new();

        for event in &self.events_data {
            jobs.entry(event.job_id.clone())
                .or_insert_with(Vec::new)
                .push(event);
        }

        let mut job_list: Vec<(String, Vec<&EventData>)> = jobs.into_iter().collect();
        job_list.sort_by(|a, b| {
            let a_epoch = a.1.first().map(|e| e.epoch).unwrap_or(0);
            let b_epoch = b.1.first().map(|e| e.epoch).unwrap_or(0);
            b_epoch.cmp(&a_epoch)
        });

        job_list
    }
}

impl Default for EventsState {
    fn default() -> Self {
        Self::new()
    }
}
