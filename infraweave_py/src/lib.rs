mod module;
mod stack;
mod deployment;

use deployment::Deployment;
use module::Module;
use pyo3::prelude::*;
use stack::Stack;

#[pymodule]
fn infraweave_py(py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<Module>()?;
    m.add_class::<Stack>()?;
    m.add_class::<Deployment>()?;
    Ok(())
}
