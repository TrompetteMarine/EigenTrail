use crate::resources::DEFAULT_BYTES;
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "eigentrail.trace.v1";
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Matrix {
    pub n: usize,
    pub data: Vec<f64>,
}
impl Matrix {
    pub fn at(&self, i: usize, j: usize) -> f64 {
        self.data[i + j * self.n]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Calibration {
    Conditional {
        radii: Vec<f64>,
        assumptions: String,
    },
    Gaussian {
        observations: u64,
        estimated_mean: bool,
        divisor: f64,
        alpha: f64,
        auxiliary_risk: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TraceRequest {
    pub schema: String,
    pub times: Vec<f64>,
    pub covariances: Vec<Matrix>,
    pub features: Vec<String>,
    pub lipschitz: f64,
    pub calibration: Calibration,
    pub correction: Vec<f64>,
    pub target: f64,
    pub numerical_allowance: f64,
    pub memory_bytes: usize,
}
impl TraceRequest {
    pub fn try_conditional(
        times: Vec<f64>,
        covariances: Vec<Matrix>,
        radii: Vec<f64>,
        lipschitz: f64,
    ) -> crate::Result<Self> {
        use crate::resources::memory;
        let p = covariances.first().map_or(0, |a| a.n);
        let m = times.len();
        let mut features = memory::capacity(p)?;
        for i in 0..p {
            features.push(memory::format(format_args!("feature_{i}"))?);
        }
        Ok(Self {
            schema: memory::text(SCHEMA)?,
            times,
            covariances,
            features,
            lipschitz,
            calibration: Calibration::Conditional {
                radii,
                assumptions: memory::text(
                    "supplied nodal covariance envelope; declared global Lipschitz regularity",
                )?,
            },
            correction: crate::resources::zeros(m)?,
            target: 0.002,
            numerical_allowance: 1e-5,
            memory_bytes: DEFAULT_BYTES,
        })
    }
    pub fn try_clone(&self) -> crate::Result<Self> {
        use crate::resources::memory;
        memory::scope(|| {
            let mut covariances = memory::capacity(self.covariances.len())?;
            for a in &self.covariances {
                covariances.push(Matrix {
                    n: a.n,
                    data: memory::copy(&a.data)?,
                });
            }
            let mut features = memory::capacity(self.features.len())?;
            for name in &self.features {
                features.push(memory::text(name)?);
            }
            let calibration = match &self.calibration {
                Calibration::Conditional { radii, assumptions } => Calibration::Conditional {
                    radii: memory::copy(radii)?,
                    assumptions: memory::text(assumptions)?,
                },
                Calibration::Gaussian {
                    observations,
                    estimated_mean,
                    divisor,
                    alpha,
                    auxiliary_risk,
                } => Calibration::Gaussian {
                    observations: *observations,
                    estimated_mean: *estimated_mean,
                    divisor: *divisor,
                    alpha: *alpha,
                    auxiliary_risk: *auxiliary_risk,
                },
            };
            Ok(Self {
                schema: memory::text(&self.schema)?,
                times: memory::copy(&self.times)?,
                covariances,
                features,
                lipschitz: self.lipschitz,
                calibration,
                correction: memory::copy(&self.correction)?,
                target: self.target,
                numerical_allowance: self.numerical_allowance,
                memory_bytes: self.memory_bytes,
            })
        })
    }
    pub fn conditional(
        times: Vec<f64>,
        covariances: Vec<Matrix>,
        radii: Vec<f64>,
        lipschitz: f64,
    ) -> Self {
        let p = covariances.first().map_or(0, |a| a.n);
        let m = times.len();
        Self {
            schema: SCHEMA.into(),
            times,
            covariances,
            features: (0..p).map(|i| format!("feature_{i}")).collect(),
            lipschitz,
            calibration: Calibration::Conditional {
                radii,
                assumptions:
                    "supplied nodal covariance envelope; declared global Lipschitz regularity"
                        .into(),
            },
            correction: vec![0.; m],
            target: 0.002,
            numerical_allowance: 1e-5,
            memory_bytes: DEFAULT_BYTES,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Eigensystem {
    pub values: Vec<f64>,
    pub vectors: Matrix,
    pub backward: f64,
    pub polar: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Range {
    pub lower: f64,
    pub upper: f64,
    pub lower_closed: bool,
    pub upper_closed: bool,
}
impl Range {
    pub fn closed(lower: f64, upper: f64) -> Self {
        Self {
            lower,
            upper,
            lower_closed: true,
            upper_closed: true,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
pub enum Quantity {
    Interval(Range),
    Undefined(String),
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub strength: Range,
    pub share: Quantity,
    pub rank: [usize; 2],
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Tube {
    pub start: usize,
    pub end: usize,
    pub eta: f64,
    pub gap: f64,
    pub bound: f64,
    pub informative: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "matching", deny_unknown_fields)]
pub enum Association {
    Unique(Vec<usize>),
    Alternatives,
    NoPerfectMatching,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    pub tubes: Vec<Tube>,
    pub edges: Vec<Vec<usize>>,
    pub association: Association,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TraceReport {
    pub schema: String,
    pub request_hash: String,
    pub eigensystems: Vec<Eigensystem>,
    pub radii: Vec<f64>,
    pub segments: Vec<Segment>,
    pub summaries: Vec<Vec<Summary>>,
    pub calibration_scope: String,
    pub witness: String,
    pub completed: bool,
    pub managed_reservation: usize,
    pub resource_qualification: String,
}
