use crate::messages::LoopMessage;

use pyo3::prelude::*;
use pyo3::types::{PyAny, PyTuple};
use std::collections::HashMap;
use std::os::fd::{AsRawFd, RawFd};
use tokio::io::unix::AsyncFd;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::AbortHandle;

#[derive(Debug)]
struct FdWrapper(RawFd);

impl AsRawFd for FdWrapper {
    fn as_raw_fd(&self) -> RawFd {
        self.0
    }
}

pub struct IoRegistry {
    readers: HashMap<i32, AbortHandle>,
    writers: HashMap<i32, AbortHandle>,
    sender: UnboundedSender<LoopMessage>,
    runtime_handle: tokio::runtime::Handle,
}

impl IoRegistry {
    pub fn new(
        sender: UnboundedSender<LoopMessage>,
        runtime_handle: tokio::runtime::Handle,
    ) -> Self {
        Self {
            readers: HashMap::new(),
            writers: HashMap::new(),
            sender,
            runtime_handle,
        }
    }

    pub fn add_reader(&mut self, fd: i32, cb: Py<PyAny>, args: Py<PyTuple>) {
        self.remove_reader(fd);
        let sender = self.sender.clone();

        let handle = self.runtime_handle.spawn(async move {
            match AsyncFd::new(FdWrapper(fd)) {
                Ok(async_fd) => loop {
                    match async_fd.readable().await {
                        Ok(mut guard) => {
                            let (cb_clone, args_clone) =
                                Python::attach(|py| (cb.clone_ref(py), args.clone_ref(py)));

                            if sender
                                .send(LoopMessage::Callback(cb_clone, args_clone))
                                .is_err()
                            {
                                break;
                            }
                            guard.clear_ready();
                        }
                        Err(_) => break,
                    }
                },
                Err(e) => eprintln!("Failed to register FD {}: {}", fd, e),
            }
        });

        self.readers.insert(fd, handle.abort_handle());
    }

    pub fn remove_reader(&mut self, fd: i32) {
        if let Some(handle) = self.readers.remove(&fd) {
            handle.abort();
        }
    }

    pub fn add_writer(&mut self, fd: i32, cb: Py<PyAny>, args: Py<PyTuple>) {
        self.remove_writer(fd);
        let sender = self.sender.clone();

        let handle = self.runtime_handle.spawn(async move {
            match AsyncFd::new(FdWrapper(fd)) {
                Ok(async_fd) => loop {
                    match async_fd.writable().await {
                        Ok(mut guard) => {
                            let (cb_clone, args_clone) =
                                Python::attach(|py| (cb.clone_ref(py), args.clone_ref(py)));

                            if sender
                                .send(LoopMessage::Callback(cb_clone, args_clone))
                                .is_err()
                            {
                                break;
                            }
                            guard.clear_ready();
                        }
                        Err(_) => break,
                    }
                },
                Err(e) => eprintln!("Failed to register FD {}: {}", fd, e),
            }
        });

        self.writers.insert(fd, handle.abort_handle());
    }

    pub fn remove_writer(&mut self, fd: i32) {
        if let Some(handle) = self.writers.remove(&fd) {
            handle.abort();
        }
    }
}
