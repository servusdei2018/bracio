mod event_loop;
mod io;
mod messages;

use event_loop::BracioEventLoop;
use pyo3::prelude::*;

#[pymodule]
fn bracio(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<BracioEventLoop>()?;
    Ok(())
}
