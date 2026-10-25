//! Fallible pre-allocation accounting; scopes retain only their escaping owned buffers.
use crate::{Error, Result};
use std::{
    cell::RefCell,
    fmt,
    ops::Deref,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

mod owned;
pub trait HeapBytes {
    fn heap_bytes(&self) -> Result<usize>;
}
const CONTROL: usize = 8192;
const DEPTH: usize = 128;
fn refused() -> Error {
    Error::Resource("managed allocation budget exhausted or untracked allocation".into())
}
struct State {
    limit: usize,
    live: AtomicUsize,
    peak: AtomicUsize,
    failed: AtomicBool,
}
impl State {
    fn charge(&self, bytes: usize) -> Result<()> {
        if self.failed.load(Ordering::Relaxed) {
            return Err(refused());
        }
        let mut live = self.live.load(Ordering::Relaxed);
        loop {
            let next = live
                .checked_add(bytes)
                .filter(|n| *n <= self.limit)
                .ok_or_else(refused)?;
            match self
                .live
                .compare_exchange_weak(live, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => {
                    self.peak.fetch_max(next, Ordering::Relaxed);
                    return Ok(());
                }
                Err(now) => live = now,
            }
        }
    }
    fn release(&self, bytes: usize) {
        if self
            .live
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                n.checked_sub(bytes)
            })
            .is_err()
        {
            self.failed.store(true, Ordering::Relaxed);
        }
    }
}
struct Active {
    operation: Option<Arc<State>>,
    depth: usize,
    charged: [usize; DEPTH],
}
thread_local! { static ACTIVE:RefCell<Active> = const {RefCell::new(Active{operation:None,depth:0,charged:[0;DEPTH]})}; }
#[derive(Clone)]
pub struct Operation {
    state: Arc<State>,
}
#[derive(Debug, Clone, Copy)]
pub struct Stats {
    pub live: usize,
    pub peak: usize,
    pub limit: usize,
}
pub struct Guard {
    state: Option<Arc<State>>,
    bytes: usize,
}
impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(state) = &self.state {
            state.release(self.bytes);
        }
    }
}
pub struct Managed<T> {
    value: Option<T>,
    guard: Guard,
}
impl<T> Deref for Managed<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value.as_ref().expect("live managed value")
    }
}
impl<T> Managed<T> {
    pub fn into_inner(mut self) -> T {
        let value = self.value.take().expect("live managed value");
        if let Some(state) = self.guard.state.take() {
            ACTIVE.with(|cell| {
                let mut active = cell.borrow_mut();
                if active
                    .operation
                    .as_ref()
                    .is_some_and(|s| Arc::ptr_eq(s, &state))
                    && active.depth > 0
                {
                    let index = active.depth - 1;
                    active.charged[index] += self.guard.bytes;
                }
            });
            // Outside a scope, retain the conservative charge until the operation ends.
        }
        value
    }
}
impl<T> HeapBytes for Managed<T> {
    fn heap_bytes(&self) -> Result<usize> {
        Ok(0)
    }
}
struct Frame {
    state: Arc<State>,
    root: bool,
    finished: bool,
}
impl Frame {
    fn enter(state: Arc<State>) -> Result<Self> {
        if state.failed.load(Ordering::Relaxed) {
            return Err(refused());
        }
        let root = ACTIVE.with(|cell| -> Result<bool> {
            let mut a = cell.borrow_mut();
            if a.depth == DEPTH
                || a.operation
                    .as_ref()
                    .is_some_and(|s| !Arc::ptr_eq(s, &state))
            {
                return Err(refused());
            }
            let root = a.operation.is_none();
            if root {
                a.operation = Some(state.clone());
            }
            let depth = a.depth;
            a.charged[depth] = 0;
            a.depth += 1;
            Ok(root)
        })?;
        Ok(Self {
            state,
            root,
            finished: false,
        })
    }
    fn finish(&mut self, retained: usize, managed: bool) -> Result<()> {
        let valid = ACTIVE.with(|cell| {
            let mut a = cell.borrow_mut();
            let index = a.depth - 1;
            let charged = a.charged[index];
            if retained > charged {
                self.state.failed.store(true, Ordering::Relaxed);
                return false;
            }
            a.depth -= 1;
            if a.depth > 0 && !managed {
                let parent = a.depth - 1;
                a.charged[parent] += retained;
            }
            self.state.release(charged - retained);
            if self.root {
                a.operation = None;
            }
            true
        });
        if !valid {
            return Err(refused());
        }
        self.finished = true;
        Ok(())
    }
}
impl Drop for Frame {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish(0, false);
        }
    }
}
impl Operation {
    pub fn new(limit: usize) -> Result<Self> {
        if !(CONTROL..=super::DEFAULT_BYTES).contains(&limit) {
            return Err(Error::Resource(String::new()));
        }
        Ok(Self {
            state: Arc::new(State {
                limit,
                live: AtomicUsize::new(CONTROL),
                peak: AtomicUsize::new(CONTROL),
                failed: AtomicBool::new(false),
            }),
        })
    }
    pub fn stats(&self) -> Stats {
        Stats {
            live: self.state.live.load(Ordering::Relaxed),
            peak: self.state.peak.load(Ordering::Relaxed),
            limit: self.state.limit,
        }
    }
    pub fn external(&self, bytes: usize) -> Result<Guard> {
        self.state.charge(bytes)?;
        Ok(Guard {
            state: Some(self.state.clone()),
            bytes,
        })
    }
    pub fn run<T: HeapBytes>(&self, work: impl FnOnce() -> Result<T>) -> Result<T> {
        let mut frame = Frame::enter(self.state.clone())?;
        let result = work();
        if self.state.failed.load(Ordering::Relaxed) {
            drop(result);
            return Err(refused());
        }
        let retained = match &result {
            Ok(v) => v.heap_bytes()?,
            Err(_) => 0,
        };
        if frame.finish(retained, false).is_err() {
            drop(result);
            return Err(refused());
        }
        result
    }
    pub fn own<T: HeapBytes>(&self, work: impl FnOnce() -> Result<T>) -> Result<Managed<T>> {
        let mut frame = Frame::enter(self.state.clone())?;
        let value = work()?;
        if self.state.failed.load(Ordering::Relaxed) {
            drop(value);
            return Err(refused());
        }
        let retained = value.heap_bytes()?;
        if frame.finish(retained, true).is_err() {
            drop(value);
            return Err(refused());
        }
        Ok(Managed {
            value: Some(value),
            guard: Guard {
                state: Some(self.state.clone()),
                bytes: retained,
            },
        })
    }
}
pub fn scope<T: HeapBytes>(work: impl FnOnce() -> Result<T>) -> Result<T> {
    let current = ACTIVE.with(|cell| cell.borrow().operation.clone());
    match current {
        Some(state) => Operation { state }.run(work),
        None => work(),
    }
}
pub fn active_limit() -> Option<usize> {
    ACTIVE.with(|cell| cell.borrow().operation.as_ref().map(|state| state.limit))
}
pub fn ensure_operation<T: HeapBytes>(
    limit: usize,
    external_retained_bytes: usize,
    work: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let active = ACTIVE.with(|cell| cell.borrow().operation.clone());
    match active {
        Some(state) => {
            if state.limit != limit {
                return Err(refused());
            }
            let operation = Operation { state };
            // Borrowed records may come from outside this operation; conservative double-counting is safe.
            let _retained = operation.external(external_retained_bytes)?;
            operation.run(work)
        }
        None => {
            let operation = Operation::new(limit)?;
            let _retained = operation.external(external_retained_bytes)?;
            operation.run(work)
        }
    }
}
pub fn external(bytes: usize) -> Result<Guard> {
    let current = ACTIVE.with(|cell| cell.borrow().operation.clone());
    match current {
        Some(state) => Operation { state }.external(bytes),
        None => Ok(Guard {
            state: None,
            bytes: 0,
        }),
    }
}
fn charge(bytes: usize) -> Result<()> {
    ACTIVE.with(|cell| {
        let mut a = cell.borrow_mut();
        if let Some(state) = &a.operation {
            state.charge(bytes)?;
            let index = a.depth - 1;
            a.charged[index] = a.charged[index].checked_add(bytes).ok_or_else(refused)?;
        }
        Ok(())
    })
}
fn refund(bytes: usize) {
    ACTIVE.with(|cell| {
        let mut a = cell.borrow_mut();
        if let Some(state) = a.operation.clone() {
            let index = a.depth - 1;
            if let Some(n) = a.charged[index].checked_sub(bytes) {
                a.charged[index] = n;
                state.release(bytes)
            } else {
                state.failed.store(true, Ordering::Relaxed);
            }
        }
    });
}
pub fn capacity<T>(n: usize) -> Result<Vec<T>> {
    let bytes = n
        .checked_mul(std::mem::size_of::<T>())
        .ok_or_else(refused)?;
    charge(bytes)?;
    let mut result = Vec::new();
    if result.try_reserve_exact(n).is_err() {
        refund(bytes);
        return Err(refused());
    }
    if result.capacity().checked_mul(std::mem::size_of::<T>()) != Some(bytes) {
        drop(result);
        refund(bytes);
        return Err(refused());
    }
    Ok(result)
}
fn reserve<T>(values: &mut Vec<T>, additional: usize) -> Result<()> {
    // A growing buffer belongs to the current scope; child scopes return owned values.
    let required = values.len().checked_add(additional).ok_or_else(refused)?;
    if required <= values.capacity() {
        return Ok(());
    }
    let next = required
        .max(values.capacity().checked_mul(2).ok_or_else(refused)?)
        .max(4);
    let old = values
        .capacity()
        .checked_mul(std::mem::size_of::<T>())
        .ok_or_else(refused)?;
    let bytes = next
        .checked_mul(std::mem::size_of::<T>())
        .ok_or_else(refused)?;
    charge(bytes)?;
    if values.try_reserve_exact(next - values.len()).is_err() {
        refund(bytes);
        return Err(refused());
    }
    if values.capacity() != next {
        ACTIVE.with(|cell| {
            if let Some(state) = &cell.borrow().operation {
                state.failed.store(true, Ordering::Relaxed);
            }
        });
        return Err(refused());
    }
    refund(old);
    Ok(())
}
pub fn push<T>(values: &mut Vec<T>, value: T) -> Result<()> {
    reserve(values, 1)?;
    values.push(value);
    Ok(())
}
pub fn extend<T: Copy>(values: &mut Vec<T>, extra: &[T]) -> Result<()> {
    reserve(values, extra.len())?;
    values.extend_from_slice(extra);
    Ok(())
}
pub fn copy<T: Copy>(source: &[T]) -> Result<Vec<T>> {
    let mut result = capacity(source.len())?;
    result.extend_from_slice(source);
    Ok(result)
}
pub fn text(value: &str) -> Result<String> {
    Ok(String::from_utf8({
        let mut bytes = capacity(value.len())?;
        bytes.extend_from_slice(value.as_bytes());
        bytes
    })
    .expect("input UTF-8"))
}
pub fn format(arguments: fmt::Arguments<'_>) -> Result<String> {
    struct Writer {
        bytes: Vec<u8>,
        error: Option<Error>,
    }
    impl fmt::Write for Writer {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            match extend(&mut self.bytes, s.as_bytes()) {
                Ok(()) => Ok(()),
                Err(e) => {
                    self.error = Some(e);
                    Err(fmt::Error)
                }
            }
        }
    }
    let mut writer = Writer {
        bytes: Vec::new(),
        error: None,
    };
    if fmt::write(&mut writer, arguments).is_err() {
        return Err(writer.error.unwrap_or_else(refused));
    }
    Ok(String::from_utf8(writer.bytes).expect("formatter UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escaping_buffers_native_guards_and_temporary_scopes_refund() {
        let op = Operation::new(32768).unwrap();
        let baseline = op.stats().live;
        let output = op.own(|| copy(&[7u8; 1024])).unwrap();
        assert_eq!(op.stats().live, baseline + 1024);
        let native = op.external(4096).unwrap();
        op.run(|| {
            for _ in 0..1000 {
                scope(|| {
                    let mut scratch = capacity::<u8>(8192)?;
                    scratch.resize(8192, 1);
                    Ok(())
                })?;
            }
            Ok(())
        })
        .unwrap();
        assert!(op.stats().peak <= 32768);
        assert_eq!(op.stats().live, baseline + 1024 + 4096);
        drop(native);
        drop(output);
        assert_eq!(op.stats().live, baseline);
    }
    #[test]
    fn growth_authorizes_both_old_and_new_buffers_and_keeps_failure_intact() {
        let op = Operation::new(CONTROL + 5).unwrap();
        op.run(|| {
            let mut bytes = capacity(4)?;
            bytes.resize(4, 3u8);
            assert!(matches!(push(&mut bytes, 4), Err(Error::Resource(_))));
            assert_eq!(bytes, [3; 4]);
            Ok(())
        })
        .unwrap();
        assert_eq!(op.stats().live, CONTROL);
    }
    #[test]
    fn uncharged_escape_poison_is_fail_closed() {
        let op = Operation::new(32768).unwrap();
        assert!(matches!(
            op.run(|| Ok(String::from("untracked"))),
            Err(Error::Resource(_))
        ));
        assert!(matches!(op.run(|| copy(&[1u8])), Err(Error::Resource(_))));
        assert!(matches!(op.run(|| Ok(())), Err(Error::Resource(_))));
    }

    #[test]
    fn borrowed_admission_is_identical_inside_and_outside_an_operation() {
        let limit = CONTROL + 1024;
        let operation = Operation::new(limit).unwrap();
        assert!(matches!(
            ensure_operation(limit, 1025, || Ok(())),
            Err(Error::Resource(_))
        ));
        operation
            .run(|| {
                assert!(matches!(
                    ensure_operation(limit, 1025, || Ok(())),
                    Err(Error::Resource(_))
                ));
                ensure_operation(limit, 1024, || {
                    assert_eq!(operation.stats().live, limit);
                    Ok(())
                })?;
                assert_eq!(operation.stats().live, CONTROL);
                Ok(())
            })
            .unwrap();
        assert_eq!(operation.stats().live, CONTROL);
    }
}
