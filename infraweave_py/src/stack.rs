use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use env_defs::CloudProvider;
use env_defs::ModuleResp;
use pyo3::prelude::*;
use tokio::runtime::Runtime;

/// A Python-exposed wrapper for a stack version.
///
/// This class is just the base class of all published stacks in your platform and is not intended to be used directly. This allows interaction with a specific version of a
/// stack from the configured cloud provider via the Python SDK. The name of the stack is the logical name of the stack (e.g., "WebsiteRunner").
///
/// ## Example
///
/// ```python
/// from infraweave import WebsiteRunner
///
/// website_stack = WebsiteRunner(
///     version='0.1.6',
///     track="stable"
/// )
/// ```
///
#[pyclass(module = "infraweave")]
#[derive(Clone)]
#[allow(dead_code)]
pub struct Stack {
    /// The logical name of the stack (e.g., "WebsiteRunner").
    name: String,
    /// The version string of the stack (e.g., "1.2.3").
    version: String,
    /// The release track or channel (e.g., "stable" or "beta").
    track: String,
    /// The underlying ModuleResp data returned by the cloud handler for the stack.
    pub module: ModuleResp,
}

#[pymethods]
impl Stack {
    /// Constructs a new Stack instance by fetching metadata from the cloud.
    ///
    /// Performs an asynchronous lookup of the specified stack name, version,
    /// and track, then returns a fully initialized `Stack` object.
    ///
    /// # Arguments
    /// * `name` - The name of the stack to fetch.
    /// * `version` - The specific version identifier.
    /// * `track` - The release track (e.g., "stable", "beta").
    ///
    /// # Errors
    /// Panics if the project ID/region initialization fails or if the
    /// stack version cannot be found.
    #[new]
    fn new(name: &str, version: &str, track: &str) -> PyResult<Self> {
        let rt = Runtime::new().unwrap();
        let stack = rt.block_on(Stack::async_initialize(name, version, track))?;

        Ok(stack)
    }

    /// Retrieves the logical name of this stack.
    ///
    /// This method prints a debug log to stdout and returns the stored name.
    pub fn get_name(&self) -> &str {
        println!("get_name called {}", &self.name);
        &self.name
    }
}

impl Stack {
    /// Internal async initializer that does the actual cloud lookup.
    ///
    /// - Initializes the project ID and region from environment.
    /// - Creates a `GenericCloudHandler` to query the stack version.
    /// - Panics if the requested version is not found or on API errors.
    async fn async_initialize(name: &str, version: &str, track: &str) -> PyResult<Self> {
        // Ensure environment is set up for API calls
        initialize_project_id_and_region().await;
        let handler = GenericCloudHandler::default().await;
        // Fetch the stack version from the cloud provider
        let stack = match handler
            .get_stack_version(&name.to_lowercase(), track, version)
            .await
        {
            Ok(resp) => match resp {
                Some(stack) => stack,
                None => {
                    panic!("Version {} of stack {} not found", version, name);
                }
            },
            Err(e) => {
                panic!("Error trying to get stack {}", e);
            }
        };

        Ok(Stack {
            name: name.to_string(),
            version: version.to_string(),
            track: track.to_string(),
            module: stack,
        })
    }
}
