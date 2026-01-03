use pyo3::prelude::*;
use pyo3::types::{PyAny, PyTuple};

pub enum LoopMessage {
    RunHandle(Py<PyAny>),
    Callback(Py<PyAny>, Py<PyTuple>),
    Stop,
}
