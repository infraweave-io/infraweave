use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use env_defs::CloudProvider;
use env_defs::ModuleResp;
use pyo3::prelude::*;
use pyo3::types::PyType;
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

    /// Gets the module name.
    #[getter]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Gets the module version.
    #[getter]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Gets the module track.
    #[getter]
    pub fn track(&self) -> &str {
        &self.track
    }

    /// Gets the latest version of this module for a given track.
    ///
    /// Fetches the most recent version of the module from the
    /// configured cloud provider. If no track is specified, defaults to "stable".
    ///
    /// # Arguments
    /// * `track` - Optional release track (e.g., "stable", "beta", "dev").
    ///             Defaults to "stable" if not provided.
    ///
    /// # Returns
    /// A new `Module` instance with the latest version for the specified track.
    ///
    /// # Errors
    /// Panics if the module is not found or if there's an error communicating
    /// with the cloud provider.
    ///
    /// # Example
    /// ```python
    /// from infraweave import S3Bucket
    ///
    /// # Get latest stable version
    /// latest_module = S3Bucket.get_latest_version()
    ///
    /// # Get latest dev version
    /// latest_dev = S3Bucket.get_latest_version(track="dev")
    /// ```
    #[classmethod]
    #[pyo3(signature = (track=None))]
    fn get_latest_version(_cls: &Bound<'_, PyType>, track: Option<&str>) -> PyResult<Self> {
        let class_name_str = _cls.name()?.to_string();

        // Prevent calling get_latest_version directly on the Module base class
        if class_name_str == "Module" {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "Cannot call get_latest_version() on Module base class. Use a specific module class like S3Bucket instead."
            ));
        }

        let track = track.unwrap_or("stable");
        let rt = Runtime::new().unwrap();
        let module = rt.block_on(Module::async_get_latest(&class_name_str, track))?;
        Ok(module)
    }

    /// Static method to get the latest version by module name.
    ///
    /// This is used internally by dynamic wrapper classes.
    #[staticmethod]
    #[pyo3(signature = (name, track=None))]
    fn get_latest_version_by_name(name: &str, track: Option<&str>) -> PyResult<Self> {
        let track = track.unwrap_or("stable");
        let rt = Runtime::new().unwrap();
        let module = rt.block_on(Module::async_get_latest(name, track))?;
        Ok(module)
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

    /// Internal async method to fetch the latest version of a module.
    ///
    /// - Initializes the project ID and region from environment.
    /// - Creates a `GenericCloudHandler` to query the latest module version.
    /// - Panics if the module is not found or on API errors.
    async fn async_get_latest(name: &str, track: &str) -> PyResult<Self> {
        // Ensure environment is set up for API calls
        initialize_project_id_and_region().await;
        let handler = GenericCloudHandler::default().await;

        // Fetch the latest module version from the cloud provider
        let module = match handler
            .get_latest_module_version(&name.to_lowercase(), track)
            .await
        {
            Ok(resp) => match resp {
                Some(module) => module,
                None => {
                    panic!("No version of module {} found in track {}", name, track);
                }
            },
            Err(e) => {
                panic!("Error trying to get latest module version: {}", e);
            }
        };

        Ok(Module {
            name: name.to_string(),
            version: module.version.clone(),
            track: track.to_string(),
            module,
        })
    }
}
