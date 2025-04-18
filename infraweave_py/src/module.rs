use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use env_defs::CloudProvider;
use env_defs::ModuleResp;
use pyo3::prelude::*;
use tokio::runtime::Runtime;

#[pyclass]
#[derive(Clone)]
#[allow(dead_code)]
pub struct Module {
    name: String,
    version: String,
    track: String,
    pub module: ModuleResp,
}

#[pymethods]
impl Module {
    #[new]
    fn new(name: &str, version: &str, track: &str) -> PyResult<Self> {
        let rt = Runtime::new().unwrap();
        let module = rt.block_on(Module::async_initialize(name, version, track))?;

        Ok(module)
    }

    pub fn get_name(&self) -> &str {
        println!("get_name called {}", &self.name);
        &self.name
    }
}

impl Module {
    async fn async_initialize(name: &str, version: &str, track: &str) -> PyResult<Self> {
        initialize_project_id_and_region().await;
        let handler = GenericCloudHandler::default().await;
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
