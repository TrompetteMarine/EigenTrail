//! One-sided query sets and one separately checked complete step history.
use crate::{
    families::{Configuration, Feasibility},
    resources::{Cancellation, memory},
    scalar::*,
    *,
};
use serde::{Deserialize, Serialize};

pub const SCOPE: &str = "same complete cadlag history family on the common calibrated event; observed nodal balls, certified smooth-component bounds, or a union of two one-sided PSD balls; component-local origins/reference displacement do not cross retained jumps; one canonical step witness proves existence only, not truth, timing or Cartesian realizability";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    pub scope: String,
    pub components: Vec<Component>,
    pub queries: Vec<Query>,
    pub witnesses: Vec<StepWitness>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub cells: [usize; 2],
    pub lineage: Vec<continuous_queries::SegmentLineage>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub time: f64,
    pub state: State,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum State {
    Observed {
        date: usize,
        trace: Range,
        modes: Vec<Summary>,
    },
    Smooth {
        component: usize,
        query: continuous_queries::Query,
    },
    JumpPossible {
        cell: usize,
        mandatory: bool,
        sides: Vec<Side>,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Side {
    pub date: usize,
    pub radius: f64,
    pub trace: Range,
    pub modes: Vec<Summary>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepWitness {
    pub cells: Vec<usize>,
    pub values: Vec<Matrix>,
}

fn summary(e: &Eigensystem, radius: f64) -> Result<(Range, Vec<Summary>)> {
    memory::scope(|| {
        let err = point(radius) + point(e.backward);
        let sum = e.values.iter().fold(point(0.), |s, &v| s + point(v));
        let trace = Range::closed(
            (sum - point(e.values.len() as f64) * err).inf().max(0.),
            upper(sum + point(e.values.len() as f64) * err)?.max(0.),
        );
        Ok((trace, summaries::try_summaries(e, radius)?))
    })
}

fn components(
    report: &TraceReport,
    jump: &jumps::JumpReport,
    cancel: &Cancellation,
) -> Result<Vec<Component>> {
    memory::scope(|| {
        let mut out = Vec::new();
        let mut first = 0;
        while first < report.segments.len() {
            cancel.poll()?;
            if jump.family.mandatory[first] || jump.family.optional[first] {
                first += 1;
                continue;
            }
            let mut last = first + 1;
            while last < report.segments.len()
                && !jump.family.mandatory[last]
                && !jump.family.optional[last]
            {
                last += 1;
            }
            let lineage = continuous_queries::lineage_range(report, first, last, cancel)?;
            memory::push(
                &mut out,
                Component {
                    cells: [first, last],
                    lineage,
                },
            )?;
            first = last;
        }
        Ok(out)
    })
}

fn step_cells(
    nodes: &[Matrix],
    model: &jumps::JumpModel,
    jump: &jumps::JumpReport,
    cancel: &Cancellation,
) -> Result<Option<Vec<usize>>> {
    memory::scope(|| {
        let mut cells = Vec::new();
        for j in 0..nodes.len() - 1 {
            cancel.poll()?;
            if nodes[j].data == nodes[j + 1].data {
                continue;
            }
            if let Some(minimum) = model
                .minimum_jump
                .filter(|_| model.minimum_applies_to_all_candidates)
            {
                let mut round = point(0.);
                for (&a, &b) in nodes[j].data.iter().zip(&nodes[j + 1].data) {
                    let value = b - a;
                    if !value.is_finite() {
                        return Err(Error::Exhausted("step witness difference overflow".into()));
                    }
                    round += square(point(b) - point(a) - point(value));
                }
                let d = &jump.differences[j];
                let norm = d.values.iter().fold(0f64, |s, x| s.max(x.abs()));
                let lower = (point(norm) - point(d.backward) - round.sqrt())
                    .inf()
                    .max(0.);
                if lower < minimum {
                    return Ok(None);
                }
            }
            memory::push(&mut cells, j)?;
        }
        Ok(Some(cells))
    })
}

pub(crate) fn analyze(
    request: &TraceRequest,
    cfg: &Configuration,
    report: &TraceReport,
    jump: &jumps::JumpReport,
    times: &[f64],
    cancel: &Cancellation,
) -> Result<(Workflow, Feasibility)> {
    memory::scope(|| {
        let components = components(report, jump, cancel)?;
        let mut queries = memory::capacity(times.len())?;
        for &time in times {
            let query = memory::scope(|| {
                cancel.poll()?;
                let state = if let Ok(date) = request
                    .times
                    .binary_search_by(|t| t.partial_cmp(&time).unwrap())
                {
                    let (trace, modes) = summary(&report.eigensystems[date], report.radii[date])?;
                    State::Observed { date, trace, modes }
                } else {
                    let cell = request.times.partition_point(|&t| t < time) - 1;
                    if jump.family.mandatory[cell] || jump.family.optional[cell] {
                        let envelope = jumps::inside_cell(
                            time,
                            request.times[cell],
                            request.times[cell + 1],
                            report.radii[cell],
                            report.radii[cell + 1],
                            request.lipschitz,
                            true,
                        )?;
                        let mut sides = memory::capacity(2)?;
                        for (date, radius) in [
                            (cell, envelope.left_radius),
                            (cell + 1, envelope.right_radius),
                        ] {
                            let (trace, modes) = summary(&report.eigensystems[date], radius)?;
                            sides.push(Side {
                                date,
                                radius,
                                trace,
                                modes,
                            });
                        }
                        State::JumpPossible {
                            cell,
                            mandatory: jump.family.mandatory[cell],
                            sides,
                        }
                    } else {
                        let index = components
                            .iter()
                            .position(|c| c.cells[0] <= cell && cell < c.cells[1])
                            .ok_or_else(|| {
                                Error::Invalid("missing smooth jump component".into())
                            })?;
                        let component = &components[index];
                        let query = continuous_queries::query_at(
                            request,
                            report,
                            &component.lineage,
                            component.cells[0],
                            cell,
                            time,
                            cancel,
                        )?;
                        State::Smooth {
                            component: index,
                            query,
                        }
                    }
                };
                Ok(Query { time, state })
            })?;
            queries.push(query);
        }
        let mut witnesses = Vec::new();
        let feasibility = if !cfg.witnesses.is_empty() {
            Feasibility::UnsupportedModel
        } else {
            let families::PathClass::Jumps { model } = &cfg.class else {
                return Err(Error::Invalid("jump workflow requires jump class".into()));
            };
            let nodes = calibration::normalized(request)?;
            match step_cells(&nodes, model, jump, cancel)? {
                None => Feasibility::Unresolved,
                Some(cells) => {
                    let mut values = memory::capacity(times.len())?;
                    for &time in times {
                        cancel.poll()?;
                        let date = request
                            .times
                            .partition_point(|&t| t <= time)
                            .saturating_sub(1);
                        values.push(Matrix {
                            n: nodes[date].n,
                            data: memory::copy(&nodes[date].data)?,
                        });
                    }
                    memory::push(&mut witnesses, StepWitness { cells, values })?;
                    Feasibility::OriginalStepWitness
                }
            }
        };
        Ok((
            Workflow {
                scope: memory::text(SCOPE)?,
                components,
                queries,
                witnesses,
            },
            feasibility,
        ))
    })
}
