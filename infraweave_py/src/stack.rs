use env_common::{interface::{initialize_project_id, CloudHandler}, logic::handler};
use env_defs::ModuleResp;
use pyo3::{exceptions::PyException, prelude::*};
use tokio::runtime::Runtime;

#[pyclass]
#[derive(Clone)]
pub struct Stack {
    name: String,
    version: String,
    track: String,
    pub module: ModuleResp,
}

impl Stack {
    async fn async_initialize(name: &str, version: &str, track: &str) -> PyResult<Self> {
        initialize_project_id().await;
        let stack = match handler().get_stack_version(&name.to_lowercase(), &track, version).await {
            Ok(resp) => match resp {
                Some(stack) => stack,
                None => {
                    panic!("Version {} of stack {} not found", version, name);
                }
            }
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

#[pymethods]
impl Stack {
    #[new]
    fn new(name: &str, version: &str, track: &str) -> PyResult<Self> {
        let rt = Runtime::new().unwrap();
        let stack = rt.block_on(Stack::async_initialize(name, version, track))?;

        Ok(stack)
    }
}