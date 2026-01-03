use crate::io::IoRegistry;
use crate::messages::LoopMessage;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyTuple};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

#[pyclass(subclass, weakref, name = "BracioLoop")]
pub struct BracioEventLoop {
    pub(crate) runtime: Runtime,
    sender: UnboundedSender<LoopMessage>,
    receiver: Arc<Mutex<Option<UnboundedReceiver<LoopMessage>>>>,
    debug: AtomicBool,
    io_registry: Arc<Mutex<IoRegistry>>,
}

#[pymethods]
impl BracioEventLoop {
    #[new]
    pub fn new() -> PyResult<Self> {
        let runtime = Builder::new_multi_thread()
            .enable_all()
            .thread_name("bracio-worker")
            .build()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

        let (sender, receiver) = unbounded_channel::<LoopMessage>();
        let io_registry = IoRegistry::new(sender.clone(), runtime.handle().clone());

        Ok(BracioEventLoop {
            runtime,
            sender,
            receiver: Arc::new(Mutex::new(Some(receiver))),
            debug: AtomicBool::new(false),
            io_registry: Arc::new(Mutex::new(io_registry)),
        })
    }

    fn get_debug(&self) -> bool {
        self.debug.load(Ordering::Relaxed)
    }

    fn set_debug(&self, value: bool) {
        self.debug.store(value, Ordering::Relaxed);
    }

    // === Core Loop Methods ===

    fn run_forever<'py>(slf: Bound<'py, Self>, py: Python<'py>) -> PyResult<()> {
        let asyncio_events = py.import("asyncio.events")?;

        if asyncio_events.hasattr("_set_running_loop")? {
            asyncio_events.call_method1("_set_running_loop", (&slf,))?;
        }

        let (mut receiver, rt_handle) = {
            let rust_self = slf.borrow();
            let mut guard = rust_self.receiver.lock().unwrap();
            let rx = guard
                .take()
                .expect("run_forever called more than once or concurrently");
            let handle = rust_self.runtime.handle().clone();
            (rx, handle)
        };

        let receiver = py.detach(move || {
            rt_handle.block_on(async move {
                while let Some(msg) = receiver.recv().await {
                    match msg {
                        LoopMessage::RunHandle(handle) => {
                            Python::attach(|py| {
                                let h = handle.bind(py);
                                if let Err(e) = h.call_method0("_run") {
                                    e.print(py);
                                }
                            });
                        }
                        LoopMessage::Callback(cb, args) => {
                            Python::attach(|py| {
                                let callback = cb.bind(py);
                                let arguments = args.bind(py);
                                if let Err(e) = callback.call1(arguments) {
                                    e.print(py);
                                }
                            });
                        }
                        LoopMessage::Stop => break,
                    }
                }
                receiver
            })
        });

        let rust_self = slf.borrow();
        let mut guard = rust_self.receiver.lock().unwrap();
        *guard = Some(receiver);

        if asyncio_events.hasattr("_set_running_loop")? {
            asyncio_events.call_method1("_set_running_loop", (py.None(),))?;
        }

        Ok(())
    }

    // === Core asyncio Methods ===

    #[pyo3(signature = (coro, *, name=None, context=None))]
    fn create_task<'py>(
        slf: Bound<'py, Self>,
        py: Python<'py>,
        coro: Bound<'py, PyAny>,
        name: Option<Bound<'py, PyAny>>,
        context: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let asyncio = py.import("asyncio")?;
        let kwargs = PyDict::new(py);

        if let Some(n) = name {
            kwargs.set_item("name", n)?;
        }
        if let Some(c) = context {
            kwargs.set_item("context", c)?;
        }

        let running_loop = asyncio.call_method0("get_running_loop");
        let should_pass_loop = match running_loop {
            Ok(running) => !running.is(&slf),
            Err(_) => true,
        };

        if should_pass_loop {
            kwargs.set_item("loop", &slf)?;
        }

        asyncio.call_method("Task", (coro,), Some(&kwargs))
    }

    fn run_until_complete<'py>(
        slf: Bound<'py, Self>,
        py: Python<'py>,
        future: Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let asyncio = py.import("asyncio")?;

        let is_future = asyncio.call_method1("isfuture", (&future,))?.is_truthy()?;
        let fut = if is_future {
            future
        } else {
            Self::create_task(slf.clone(), py, future, None, None)?
        };

        let cb = slf.getattr("_stop_cb")?;
        fut.call_method1("add_done_callback", (cb,))?;

        Self::run_forever(slf.clone(), py)?;

        fut.call_method0("result")
    }

    fn _stop_cb(&self, _fut: &Bound<'_, PyAny>) -> PyResult<()> {
        self.stop()
    }

    fn stop(&self) -> PyResult<()> {
        self.sender
            .send(LoopMessage::Stop)
            .map_err(|_| PyRuntimeError::new_err("loop closed"))?;

        Ok(())
    }

    // TODO: Stub
    fn is_running(&self) -> PyResult<bool> {
        Ok(true)
    }

    // TODO: Stub
    fn is_closed(&self) -> PyResult<bool> {
        Ok(false)
    }

    // TODO: Stub
    fn close(&self) -> PyResult<()> {
        Ok(())
    }

    // === Scheduling ===

    fn time(&self) -> PyResult<f64> {
        use std::time::{SystemTime, UNIX_EPOCH};
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(dur) => Ok(dur.as_secs_f64()),
            Err(e) => Err(PyRuntimeError::new_err(e.to_string())),
        }
    }

    #[pyo3(signature = (callback, *args, context=None))]
    fn call_soon<'py>(
        slf: Bound<'py, Self>,
        py: Python<'py>,
        callback: Bound<'py, PyAny>,
        args: Vec<Bound<'py, PyAny>>,
        context: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let asyncio = py.import("asyncio")?;
        let kwargs = PyDict::new(py);
        kwargs.set_item("loop", &slf)?;
        if let Some(c) = context {
            kwargs.set_item("context", c)?;
        }
        let handle =
            asyncio.call_method("Handle", (callback, PyTuple::new(py, args)?), Some(&kwargs))?;

        let rust_self = slf.borrow();
        rust_self
            .sender
            .send(LoopMessage::RunHandle(handle.clone().unbind()))
            .map_err(|_| PyRuntimeError::new_err("loop closed"))?;

        Ok(handle)
    }

    #[pyo3(signature = (callback, *args, context=None))]
    fn call_soon_threadsafe<'py>(
        slf: Bound<'py, Self>,
        py: Python<'py>,
        callback: Bound<'py, PyAny>,
        args: Vec<Bound<'py, PyAny>>,
        context: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        Self::call_soon(slf, py, callback, args, context)
    }

    #[pyo3(signature = (delay, callback, *args, context=None))]
    fn call_later<'py>(
        slf: Bound<'py, Self>,
        py: Python<'py>,
        delay: f64,
        callback: Bound<'py, PyAny>,
        args: Vec<Bound<'py, PyAny>>,
        context: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let asyncio = py.import("asyncio")?;
        let rust_self = slf.borrow();
        let now = rust_self.time()?;
        let when = now + delay;

        let kwargs = PyDict::new(py);
        kwargs.set_item("loop", &slf)?;
        if let Some(c) = context {
            kwargs.set_item("context", c)?;
        }
        let handle = asyncio.call_method(
            "TimerHandle",
            (when, callback, PyTuple::new(py, args)?),
            Some(&kwargs),
        )?;
        let sender = rust_self.sender.clone();
        if sender.is_closed() {
            return Err(PyRuntimeError::new_err("loop closed"));
        }

        let handle_ref = handle.clone().unbind(); // Keep reference to pass to worker

        rust_self.runtime.handle().spawn(async move {
            if delay > 0.0 {
                tokio::time::sleep(tokio::time::Duration::from_secs_f64(delay)).await;
            }
            let _ = sender.send(LoopMessage::RunHandle(handle_ref));
        });

        Ok(handle)
    }

    #[pyo3(signature = (when, callback, *args, context=None))]
    fn call_at<'py>(
        slf: Bound<'py, Self>,
        py: Python<'py>,
        when: f64,
        callback: Bound<'py, PyAny>,
        args: Vec<Bound<'py, PyAny>>,
        context: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let rust_self = slf.borrow();
        let loop_time = rust_self.time()?;
        let delay = if when > loop_time {
            when - loop_time
        } else {
            0.0
        };

        drop(rust_self);
        Self::call_later(slf, py, delay, callback, args, context)
    }

    // === I/O ===

    fn add_reader(&self, fd: i32, cb: Py<PyAny>, args: Py<PyTuple>) {
        self.io_registry.lock().unwrap().add_reader(fd, cb, args);
    }
    fn remove_reader(&self, fd: i32) {
        self.io_registry.lock().unwrap().remove_reader(fd);
    }
    fn add_writer(&self, fd: i32, cb: Py<PyAny>, args: Py<PyTuple>) {
        self.io_registry.lock().unwrap().add_writer(fd, cb, args);
    }
    fn remove_writer(&self, fd: i32) {
        self.io_registry.lock().unwrap().remove_writer(fd);
    }
}
