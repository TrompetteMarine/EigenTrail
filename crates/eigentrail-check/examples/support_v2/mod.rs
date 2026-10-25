//! Registered finite-precision campaign sampler, separate from historical v1.
//! Known-answer conformance does not certify an exact continuous Gaussian law.
use eigentrail_core::{Error, Result};
use rand_chacha::{ChaCha12Rng, rand_core::RngCore};
use sha2::{Digest, Sha256};

pub struct Sampler<R = ChaCha12Rng>(pub R);

/// Every upper52-bit bin has an exactly representable midpoint strictly in(0,1).
pub fn open_uniform(raw: u64) -> f64 {
    ((raw >> 12) as f64 + 0.5) / 4503599627370496.
}

pub fn logical_seed(
    master_seed: u64,
    scenario: &str,
    outer: usize,
    date: Option<usize>,
    purpose: &str,
) -> Result<[u8; 32]> {
    if !matches!(scenario, "G1" | "G2" | "G3" | "G4" | "G5" | "G6")
        || outer >= 2000
        || date.is_some_and(|j| j > 100)
        || !matches!(purpose, "pilot-scatter" | "release-scatter")
    {
        return Err(Error::Invalid(
            "V2 campaign logical stream identifier".into(),
        ));
    }
    let token = date.map_or_else(|| "shared".to_owned(), |j| format!("date-{j}"));
    Ok(Sha256::digest(format!(
        "EigenTrail/campaign/v2/{master_seed}/{scenario}/{outer}/{token}/{purpose}"
    ))
    .into())
}

impl<R: RngCore> Sampler<R> {
    pub fn uniform(&mut self) -> f64 {
        open_uniform(self.0.next_u64())
    }

    /// One cosine Box–Muller draw, consuming two fresh uniforms without a cache.
    pub fn normal(&mut self) -> f64 {
        let u = self.uniform();
        let v = self.uniform();
        (-2. * u.ln()).sqrt() * (2. * std::f64::consts::PI * v).cos()
    }

    /// Marsaglia–Tsang for shape>=1; smaller shapes are explicitly unsupported.
    pub fn gamma(&mut self, shape: f64) -> Result<f64> {
        if !shape.is_finite() || shape < 1. {
            return Err(Error::Invalid("V2 gamma requires finite shape>=1".into()));
        }
        let d = shape - 1. / 3.;
        let c = (9. * d).sqrt().recip();
        if !c.is_finite() || c <= 0. {
            return Err(Error::Exhausted("V2 gamma arithmetic range".into()));
        }
        for _ in 0..100000 {
            let x = self.normal();
            let base = 1. + c * x;
            if base <= 0. {
                continue;
            }
            let v = base * base * base;
            let u = self.uniform();
            if u < 1. - 0.0331 * x.powi(4) || u.ln() < x * x / 2. + d * (1. - v + v.ln()) {
                let value = d * v;
                if !value.is_finite() || value <= 0. {
                    return Err(Error::Exhausted("V2 gamma result range".into()));
                }
                return Ok(value);
            }
        }
        Err(Error::Exhausted("V2 gamma sampler iteration cap".into()))
    }

    /// Normalized Bartlett scatter with fixed row/draw and ascending-k reduction order.
    pub fn wishart(&mut self, p: usize, nu: usize) -> Result<Vec<f64>> {
        if !(1..=256).contains(&p) || nu <= p || nu as u128 > 1u128 << 53 {
            return Err(Error::Invalid(
                "V2 Bartlett requires1<=p<=256 and p+1<=nu<=2^53".into(),
            ));
        }
        let size = p * p;
        let mut factor = zeros(size)?;
        for row in 0..p {
            for col in 0..row {
                factor[row + p * col] = self.normal();
            }
            factor[row + p * row] = (2. * self.gamma((nu - row) as f64 / 2.)?).sqrt();
        }
        let mut output = zeros(size)?;
        for col in 0..p {
            for row in col..p {
                let mut value = 0.;
                for k in 0..=col {
                    value += factor[row + p * k] * factor[col + p * k];
                }
                value /= nu as f64;
                if !value.is_finite() {
                    return Err(Error::Exhausted("V2 Bartlett result range".into()));
                }
                output[row + p * col] = value;
                output[col + p * row] = value;
            }
        }
        Ok(output)
    }
}

fn zeros(count: usize) -> Result<Vec<f64>> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| Error::Resource("V2 sampler workspace allocation refused".into()))?;
    values.resize(count, 0.);
    Ok(values)
}
