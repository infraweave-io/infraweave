use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use env_defs::CloudProvider;
use env_defs::ModuleResp;
use pyo3::prelude::*;
use pyo3::types::PyType;
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

    /// Gets the stack name.
    #[getter]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Gets the stack version.
    #[getter]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Gets the stack track.
    #[getter]
    pub fn track(&self) -> &str {
        &self.track
    }

    /// Gets the latest version of this stack for a given track.
    ///
    /// Fetches the most recent version of the stack from the
    /// configured cloud provider. If no track is specified, defaults to "stable".
    ///
    /// # Arguments
    /// * `track` - Optional release track (e.g., "stable", "beta", "dev").
    ///             Defaults to "stable" if not provided.
    ///
    /// # Returns
    /// A new `Stack` instance with the latest version for the specified track.
    ///
    /// # Errors
    /// Panics if the stack is not found or if there's an error communicating
    /// with the cloud provider.
    ///
    /// # Example
    /// ```python
    /// from infraweave import WebsiteRunner
    ///
    /// # Get latest stable version
    /// latest_stack = WebsiteRunner.get_latest_version()
    ///
    /// # Get latest dev version
    /// latest_dev = WebsiteRunner.get_latest_version(track="dev")
    /// ```
    #[classmethod]
    #[pyo3(signature = (track=None))]
    fn get_latest_version(_cls: &Bound<'_, PyType>, track: Option<&str>) -> PyResult<Self> {
        let class_name_str = _cls.name()?.to_string();

        // Prevent calling get_latest_version directly on the Stack base class
        if class_name_str == "Stack" {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "Cannot call get_latest_version() on Stack base class. Use a specific stack class like WebsiteRunner instead."
            ));
        }

        let track = track.unwrap_or("stable");
        let rt = Runtime::new().unwrap();
        let stack = rt.block_on(Stack::async_get_latest(&class_name_str, track))?;
        Ok(stack)
    }

    /// Static method to get the latest version by stack name.
    ///
    /// This is used internally by dynamic wrapper classes.
    #[staticmethod]
    #[pyo3(signature = (name, track=None))]
    fn get_latest_version_by_name(name: &str, track: Option<&str>) -> PyResult<Self> {
        let track = track.unwrap_or("stable");
        let rt = Runtime::new().unwrap();
        let stack = rt.block_on(Stack::async_get_latest(name, track))?;
        Ok(stack)
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

    /// Internal async method to fetch the latest version of a stack.
    ///
    /// - Initializes the project ID and region from environment.
    /// - Creates a `GenericCloudHandler` to query the latest stack version.
    /// - Panics if the stack is not found or on API errors.
    async fn async_get_latest(name: &str, track: &str) -> PyResult<Self> {
        // Ensure environment is set up for API calls
        initialize_project_id_and_region().await;
        let handler = GenericCloudHandler::default().await;

        // Fetch the latest stack version from the cloud provider
        let stack = match handler
            .get_latest_stack_version(&name.to_lowercase(), track)
            .await
        {
            Ok(resp) => match resp {
                Some(stack) => stack,
                None => {
                    panic!("No version of stack {} found in track {}", name, track);
                }
            },
            Err(e) => {
                panic!("Error trying to get latest stack version: {}", e);
            }
        };

        Ok(Stack {
            name: name.to_string(),
            version: stack.version.clone(),
            track: track.to_string(),
            module: stack,
        })
    }
}
