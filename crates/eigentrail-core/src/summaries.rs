use crate::{Result, resources::memory, scalar::*, types::*};
pub fn summaries(e: &Eigensystem, radius: f64) -> Vec<Summary> {
    try_summaries(e, radius).expect("fallible summaries require try_summaries")
}
pub fn try_summaries(e: &Eigensystem, radius: f64) -> Result<Vec<Summary>> {
    crate::resources::memory::scope(|| {
        let n = e.values.len();
        let err = point(radius) + point(e.backward);
        let mut strengths = memory::capacity(n)?;
        for &v in &e.values {
            strengths.push(Range::closed(
                (point(v) - err).inf().max(0.),
                (point(v) + err).sup().max(0.),
            ));
        }
        let trace = e.values.iter().fold(point(0.), |s, &v| s + point(v));
        let trlo = (trace - point(n as f64) * err).inf();
        let trhi = (trace + point(n as f64) * err).sup();
        let mut out = memory::capacity(n)?;
        for v in &strengths {
            let share = if trlo > 0. {
                Quantity::Interval(Range::closed(
                    (point(v.lower) / point(trhi)).inf().max(0.),
                    (point(v.upper) / point(trlo)).sup().min(1.),
                ))
            } else {
                Quantity::Undefined(memory::text("trace may be zero")?)
            };
            let rank = [
                1 + strengths.iter().filter(|s| s.lower > v.upper).count(),
                n - strengths.iter().filter(|s| s.upper < v.lower).count(),
            ];
            out.push(Summary {
                strength: v.clone(),
                share,
                rank,
            });
        }
        Ok(out)
    })
}
