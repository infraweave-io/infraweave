use crate::deployment::Deployment;
pub use crate::module::Module;
pub use crate::stack::Stack;
use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use env_defs::CloudProvider;
use env_utils::setup_logging;
use pyo3::prelude::*;
use pyo3::types::{IntoPyDict, PyDict};
use std::collections::HashSet;
use std::ffi::CString;
use tokio::runtime::Runtime;

// This is a helper function to create a dynamic wrapper class for each module,
// since it's not possible to infer the class name from the module name otherwise
#[allow(dead_code)]
fn create_dynamic_wrapper(
    py: Python<'_>,
    class_name: &str,
    wrapped_class: &str,
) -> PyResult<PyObject> {
    let class_dict = PyDict::new(py);

    let globals = {
        let d = PyDict::new(py);
        if wrapped_class == "Module" {
            d.set_item(wrapped_class, py.get_type::<Module>())?; // `set_item` takes Bound
        } else {
            d.set_item(wrapped_class, py.get_type::<Stack>())?;
        }
        Some(d)
    };

    // Define `__init__` as a lambda function to initialize `module` with `name`, `version`, and `track`
    let init_func = py.eval(
        CString::new(format!(
            "lambda self, version, track: setattr(self, 'module', {}('{}', version, track))",
            wrapped_class, class_name
        ))?
        .as_c_str(),
        globals.as_ref(),
        None,
    )?;
    class_dict.set_item("__init__", init_func)?;

    // Define `get_name` to call `self.module.get_name`, this is necessary for all functions to add to the class
    let get_name_func = py.eval(
        CString::new("lambda self: self.module.get_name()")?.as_c_str(),
        None,
        None,
    )?;
    class_dict.set_item("get_name", get_name_func)?;

    let globals_dict = [("dict", class_dict)].into_py_dict(py)?;

    // Create the dynamic class with `type(class_name, (object,), class_dict)`
    let dynamic_class = py.eval(
        CString::new(format!("type('{}', (object,), dict)", class_name))?.as_c_str(),
        Some(&globals_dict),
        None,
    )?;

    Ok(dynamic_class.into())
}

// async fn _get_available_modules() -> Vec<ModuleResp> {
//     handler().get_all_latest_module("").await.unwrap_or(vec![])
// }

// async fn _get_available_stacks() -> Vec<ModuleResp> {
//     handler().get_all_latest_stack("").await.unwrap_or(vec![])
// }

#[allow(dead_code)]
async fn get_available_modules_stacks() -> (Vec<String>, Vec<String>) {
    initialize_project_id_and_region().await;
    let handler = GenericCloudHandler::default().await;
    let (modules, stacks) = tokio::join!(
        handler.get_all_latest_module(""),
        handler.get_all_latest_stack("")
    );

    let unique_module_names: HashSet<_> = modules
        .unwrap_or(vec![])
        .into_iter()
        .map(|module| module.module_name)
        .collect();
    let unique_stack_names: HashSet<_> = stacks
        .unwrap_or(vec![])
        .into_iter()
        .map(|stack| stack.module_name)
        .collect();

    (
        unique_module_names.into_iter().collect(),
        unique_stack_names.into_iter().collect(),
    )
}

#[pymodule]
fn infraweave(py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    setup_logging().unwrap();

    let rt = Runtime::new().unwrap();
    let (available_modules, available_stacks) = rt.block_on(get_available_modules_stacks());

    for module_name in available_modules {
        // Dynamically create each wrapper class and add it to the module
        let dynamic_class = create_dynamic_wrapper(py, &module_name, "Module")?;
        m.add(&*module_name, dynamic_class)?;
    }
    for stack_name in available_stacks {
        // Dynamically create each wrapper class and add it to the stack
        let dynamic_class = create_dynamic_wrapper(py, &stack_name, "Stack")?;
        m.add(&*stack_name, dynamic_class)?;
    }

    m.add_class::<Module>()?;
    m.add_class::<Stack>()?;
    m.add_class::<Deployment>()?;
    Ok(())
}
