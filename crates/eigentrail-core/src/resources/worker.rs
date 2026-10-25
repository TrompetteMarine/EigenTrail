//! Shared native worker ownership; storage remains charged until the thread joins.
use super::{
    Cancellation,
    memory::{Guard, HeapBytes, Managed, Operation},
};
use crate::{Error, Result};
use std::{
    sync::mpsc::{Receiver, RecvTimeoutError, sync_channel},
    thread::{Builder, Scope, ScopedJoinHandle},
    time::Duration,
};

pub const STACK_BYTES: usize = 2 * 1024 * 1024;
pub const CONTROL_BYTES: usize = 64 * 1024;

pub struct Worker<'scope, T> {
    receiver: Receiver<Result<Managed<T>>>,
    cancel: Cancellation,
    handle: Option<ScopedJoinHandle<'scope, ()>>,
    _storage: Guard,
}

impl<'scope, T: Send + HeapBytes + 'scope> Worker<'scope, T> {
    pub fn spawn<'env>(
        scope: &'scope Scope<'scope, 'env>,
        operation: &Operation,
        work: impl FnOnce(Cancellation) -> Result<T> + Send + 'scope,
    ) -> Result<Self> {
        let bytes = STACK_BYTES
            .checked_add(CONTROL_BYTES)
            .and_then(|n| n.checked_add(std::mem::size_of_val(&work)))
            .and_then(|n| n.checked_add(std::mem::size_of::<T>()))
            .ok_or_else(|| Error::Resource("worker storage overflow".into()))?;
        let storage = operation.external(bytes)?;
        let cancel = Cancellation::default();
        let worker_cancel = cancel.clone();
        let worker_operation = operation.clone();
        let (sender, receiver) = sync_channel(1);
        let handle = Builder::new()
            .stack_size(STACK_BYTES)
            .spawn_scoped(scope, move || {
                let _ = sender.send(worker_operation.own(|| work(worker_cancel)));
            })
            .map_err(|_| Error::Resource("worker creation refused".into()))?;
        Ok(Self {
            receiver,
            cancel,
            handle: Some(handle),
            _storage: storage,
        })
    }
}

impl<T> Worker<'_, T> {
    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<Result<Managed<T>>, RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }
}

impl<T> Drop for Worker<'_, T> {
    fn drop(&mut self) {
        self.cancel();
        if let Some(handle) = self.handle.take() {
            // A panicked worker is reported by the disconnected receiver as incomplete.
            let _ = handle.join();
        }
    }
}
