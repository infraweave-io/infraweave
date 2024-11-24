use prometheus::{register_int_gauge_vec, IntGaugeVec};

const PREFIX: &str = "infraweave";

#[derive(Clone)]
pub struct Metrics {
    pub event_counter: IntGaugeVec,
    // pub running_jobs: IntGaugeVec,
    // pub failing_jobs: IntCounterVec,
    // pub module_version_runtime: HistogramVec,
    // pub error_count: IntCounterVec,
    // pub run_count: IntCounterVec,
}

fn name(name: &str) -> String {
    format!("{}_{}", PREFIX, name)
}

impl Metrics {
    pub fn new() -> Self {
        Metrics {
            event_counter: register_int_gauge_vec!(
                name("event_status_total"),
                "Total number of events by status",
                &["module", "status"]
            )
            .unwrap(),
            // running_jobs: register_int_gauge_vec!(
            //     name("running_jobs"),
            //     "Current number of running jobs by module",
            //     &["module"]
            // ).unwrap(),
            // failing_jobs: register_int_counter_vec!(
            //     name("failing_jobs_total"),
            //     "Total number of failing jobs by module",
            //     &["module", "timestamp"]
            // ).unwrap(),
            // module_version_runtime: register_histogram_vec!(
            //     name("module_version_runtime_seconds"),
            //     "Histogram of module runtime by version",
            //     &["module", "version"]
            // ).unwrap(),
            // error_count: register_int_counter_vec!(
            //     name("module_error_count_total"),
            //     "Total errors per module",
            //     &["module"]
            // ).unwrap(),
            // run_count: register_int_counter_vec!(
            //     name("module_run_count_total"),
            //     "Total runs per module",
            //     &["module"]
            // ).unwrap(),
        }
    }

    // pub fn start_job(&self, module: &str) {
    //     self.running_jobs.with_label_values(&[module]).inc();
    // }

    // // Called when a job finishes (successfully or with failure)
    // pub fn finish_job(&self, module: &str) {
    //     self.running_jobs.with_label_values(&[module]).dec();
    // }

    // // fn observe_stage_runtime(&self, module: &str, stage: &str, duration_secs: f64) {
    // //     self.stage_runtime.with_label_values(&[module, stage]).observe(duration_secs);
    // // }

    // pub fn observe_version_runtime(&self, module: &str, version: &str, duration_secs: f64) {
    //     self.module_version_runtime.with_label_values(&[module, version]).observe(duration_secs);
    // }

    // pub fn record_success(&self, module: &str) {
    //     self.run_count.with_label_values(&[module]).inc();
    // }

    // pub fn fail_job(&self, module: &str) {
    //     self.run_count.with_label_values(&[module]).inc();
    //     self.error_count.with_label_values(&[module]).inc();
    // }

    // pub fn observe_event(&self, status: &str) {
    //     self.event_counter.with_label_values(&[status]).inc();
    // }
}
