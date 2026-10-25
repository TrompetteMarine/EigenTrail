use crate::{Error, Result};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
pub mod memory;
pub mod worker;
pub const DEFAULT_BYTES: usize = 512 * 1024 * 1024;

#[derive(Clone, Default)]
pub struct Cancellation(pub Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn poll(&self) -> Result<()> {
        if self.0.load(Ordering::Relaxed) {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

pub fn preflight(p: usize, dates: usize, limit: usize) -> Result<usize> {
    if !(1..=256).contains(&p) || !(2..=10000).contains(&dates) || limit > DEFAULT_BYTES {
        return Err(Error::Invalid(
            "features 1..256, dates 2..10000, budget <=512 MiB required".into(),
        ));
    }
    let matrix = p
        .checked_mul(p)
        .ok_or_else(|| Error::Resource("matrix size overflow".into()))?;
    let raw = matrix
        .checked_mul(dates)
        .and_then(|v| v.checked_mul(8))
        .ok_or_else(|| Error::Resource("input size overflow".into()))?;
    let scratch = matrix
        .checked_mul(8 * 64)
        .ok_or_else(|| Error::Resource("scratch size overflow".into()))?;
    // Many scalar dates have small payloads but substantial report/JSON structure.
    let records = dates
        .checked_mul(p)
        .and_then(|v| v.checked_mul(8192))
        .ok_or_else(|| Error::Resource("record size overflow".into()))?;
    // Conservative refusal only: the complete shared allocation cap is still pending.
    let reserved = raw
        .checked_mul(20)
        .and_then(|v| v.checked_add(scratch))
        .and_then(|v| v.checked_add(records))
        .and_then(|v| v.checked_add(2 * 1024 * 1024))
        .ok_or_else(|| Error::Resource("workspace size overflow".into()))?;
    if reserved > limit {
        return Err(Error::Resource(format!(
            "conservative reservation {reserved} exceeds managed limit {limit}"
        )));
    }
    Ok(reserved)
}

pub fn zeros(n: usize) -> Result<Vec<f64>> {
    let mut data = memory::capacity(n)?;
    data.resize(n, 0.);
    Ok(data)
}
