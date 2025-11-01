use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use env_defs::CloudProvider;
use env_defs::ModuleResp;
use pyo3::prelude::*;
use tokio::runtime::Runtime;

/// A Python-exposed wrapper for a module version.
///
/// This class is just the base class of all published modules in your platform and is not intended to be used directly. This allows interaction with a specific version of a
/// module from the configured cloud provider via the Python SDK. The name of the module is the logical name of the module (e.g., "S3Bucket").
///
/// ## Example
///
/// ```python
/// from infraweave import S3Bucket
///
/// bucket_module = S3Bucket(
///     version='0.0.11-dev',
///     track="dev"
/// )
/// ```
///
#[pyclass(module = "infraweave")]
#[derive(Clone)]
#[allow(dead_code)]
pub struct Module {
    /// The logical name of the module (e.g., "S3Bucket").
    name: String,
    /// The version string of the module (e.g., "1.2.3").
    version: String,
    /// The release track or channel (e.g., "stable" or "beta").
    track: String,
    /// The underlying ModuleResp data returned by the cloud handler.
    pub module: ModuleResp,
}

#[pymethods]
impl Module {
    /// Constructs a new Module instance by fetching metadata from the cloud.
    ///
    /// Performs an asynchronous lookup of the specified module name, version,
    /// and track, then returns a fully initialized `Module` object.
    ///
    /// # Arguments
    /// * `name` - The name of the module to fetch.
    /// * `version` - The specific version identifier.
    /// * `track` - The release track (e.g., "stable", "beta").
    ///
    /// # Errors
    /// Panics if the project ID/region initialization fails or if the
    /// module version cannot be found.
    ///
    #[new]
    fn new(name: &str, version: &str, track: &str) -> PyResult<Self> {
        let rt = Runtime::new().unwrap();
        let module = rt.block_on(Module::async_initialize(name, version, track))?;

        Ok(module)
    }

    /// Retrieves the logical name of this module.
    ///
    /// This method prints a debug log to stdout and returns the stored name.
    pub fn get_name(&self) -> &str {
        println!("get_name called {}", &self.name);
        &self.name
    }
}

impl Module {
    /// Internal async initializer that does the actual cloud lookup.
    ///
    /// - Initializes the project ID and region from environment.
    /// - Creates a `GenericCloudHandler` to query the module version.
    /// - Panics if the requested version is not found or on API errors.
    async fn async_initialize(name: &str, version: &str, track: &str) -> PyResult<Self> {
        // Ensure environment is set up for API calls
        initialize_project_id_and_region().await;
        let handler = GenericCloudHandler::default().await;
        // Fetch the module version from the cloud provider
        let module = match handler
            .get_module_version(&name.to_lowercase(), track, version)
            .await
        {
            Ok(resp) => match resp {
                Some(module) => module,
                None => {
                    panic!("Version {} of module {} not found", version, name);
                }
            },
            Err(e) => {
                panic!("Error trying to get module {}", e);
            }
        };

        Ok(Module {
            name: name.to_string(),
            version: version.to_string(),
            track: track.to_string(),
            module,
        })
    }
}
