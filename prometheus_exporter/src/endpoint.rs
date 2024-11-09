use std::{collections::HashSet, sync::{Arc, Mutex}};
use axum::{response::IntoResponse, http::StatusCode};
use env_common::{interface::CloudHandler, logic::central_handler};
use env_utils::get_epoch;
use prometheus::{Encoder, TextEncoder};

use crate::metrics::Metrics;

const FIVE_MINUTES_MILLIS: u128 = 5 * 60 * 1000;
const AVAILABLE_STATUSES: [&str; 4] = ["requested", "initiated", "successful", "failed"];

pub async fn metrics_handler(metrics: Metrics, available_modules: Arc<Mutex<HashSet<String>>>) -> impl IntoResponse {
    // Reset gauges and counters to keep exporter stateless
    // metrics.running_jobs.reset();
    // metrics.failing_jobs.reset();
    // metrics.error_count.reset();
    // metrics.run_count.reset();
    metrics.event_counter.reset();

    // Clone the list of available modules
    let modules = {
        let available_modules = available_modules.lock().unwrap();
        available_modules.clone()
    };

    // Initialize each module and status with zero values
    for module in &modules {
        for &status in &AVAILABLE_STATUSES {
            metrics.event_counter.with_label_values(&[module, status]).set(0);
        }
        // metrics.running_jobs.with_label_values(&[module]).set(0);
    }

    // Fetch recent events from the database
    let region = "eu-central-1";
    let events = central_handler(region).get_all_events_between(get_epoch() - FIVE_MINUTES_MILLIS, get_epoch()).await.unwrap();

    // Update metrics based on event data
    for event in events {
        // Dynamically add new modules if they aren't in the set
        {
            let mut available_modules = available_modules.lock().unwrap();
            if available_modules.insert(event.module.clone()) {
                // Initialize the new module's metrics with zero for each status
                for &status in &AVAILABLE_STATUSES {
                    metrics.event_counter.with_label_values(&[&event.module, status]).set(0);
                }
                // metrics.running_jobs.with_label_values(&[&event.module]).set(0);
            }            
        }

        // Set or increment counters based on the event's status
        // metrics.observe_event(event.status.as_str());
        metrics.event_counter.with_label_values(&[&event.module, event.status.as_str()]).inc();

        // Handle specific statuses with additional metrics
        // match event.status.as_str() {
        //     "requested" => {
        //         // Any custom logic for requested
        //     }
        //     "initiated" => {
        //         metrics.start_job(&event.module);
        //     }
        //     "successful" => {
        //         metrics.finish_job(&event.module);
        //         metrics.record_success(&event.module);
        //     }
        //     "failed" => {
        //         metrics.fail_job(&event.module);
        //     }
        //     _ => {}
        // }
    }

    // Encode metrics to Prometheus text format
    let mut buffer = vec![];
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    encoder.encode(&metric_families, &mut buffer).unwrap();

    (StatusCode::OK, String::from_utf8(buffer).unwrap())
}
