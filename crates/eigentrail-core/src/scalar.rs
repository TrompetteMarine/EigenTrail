//! Audited outward primitives shared with the independent checker.
use crate::{Error, Result};
pub use inari::Interval;

pub fn point(x: f64) -> Interval {
    Interval::try_from((x, x)).expect("finite scalar")
}
pub fn interval(a: f64, b: f64) -> Result<Interval> {
    if !a.is_finite() || !b.is_finite() || a > b {
        return Err(Error::Invalid("invalid finite interval".into()));
    }
    Interval::try_from((a, b)).map_err(|_| Error::Invalid("invalid interval".into()))
}
pub fn upper(x: Interval) -> Result<f64> {
    let v = x.sup();
    if v.is_finite() {
        Ok(v)
    } else {
        Err(Error::Exhausted("nonfinite numerical enclosure".into()))
    }
}
pub fn magnitude(x: Interval) -> f64 {
    x.inf().abs().max(x.sup().abs())
}
pub fn square(x: Interval) -> Interval {
    x.sqr()
}

/// Relative-error dot route; exceptional intermediates require interval fallback.
pub fn normal_product_sum(terms: impl IntoIterator<Item = (f64, f64, f64)>) -> Option<f64> {
    let mut sum = 0.;
    for (a, b, c) in terms {
        let first = a * b;
        if !(first.is_normal() || (first == 0. && (a == 0. || b == 0.))) {
            return None;
        }
        let term = first * c;
        if !(term.is_normal() || (term == 0. && (first == 0. || c == 0.))) {
            return None;
        }
        let next = sum + term;
        if !(next.is_normal() || (next == 0. && sum == -term)) {
            return None;
        }
        sum = next;
    }
    Some(sum)
}
pub fn nonnegative(x: f64, name: &str) -> Result<()> {
    if x.is_finite() && x >= 0. {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "{name} must be finite and nonnegative"
        )))
    }
}

fn finite_domain(x: Interval, lower: f64, upper: f64) -> Result<()> {
    if x.is_empty()
        || !x.inf().is_finite()
        || !x.sup().is_finite()
        || x.inf() < lower
        || x.sup() > upper
    {
        return Err(Error::Invalid(
            "finite elementary-function domain required".into(),
        ));
    }
    Ok(())
}

fn log_point(mut x: f64) -> Result<Interval> {
    let adjustment = if x < f64::MIN_POSITIVE {
        x *= 4503599627370496.;
        52
    } else {
        0
    };
    let bits = x.to_bits();
    let exponent = ((bits >> 52) & 2047) as i32 - 1023 - adjustment;
    let mantissa = f64::from_bits((bits & ((1u64 << 52) - 1)) | (1023u64 << 52));
    let z = (point(mantissa) - point(1.)) / (point(mantissa) + point(1.));
    let z2 = square(z);
    let mut power = z;
    let mut sum = point(0.);
    for n in 0..48 {
        sum += power / point((2 * n + 1) as f64);
        power *= z2;
    }
    let remainder = point(2.) * power / (point(97.) * (point(1.) - z2));
    Ok(point(exponent as f64) * Interval::LN_2 + point(2.) * sum + interval(0., remainder.sup())?)
}

/// Fixed 48-term outward series with exact binary range reduction; no heap workspace.
pub fn ln(x: Interval) -> Result<Interval> {
    finite_domain(x, f64::from_bits(1), f64::MAX)?;
    interval(log_point(x.inf())?.inf(), log_point(x.sup())?.sup())
}

fn asin_small(x: Interval) -> Result<Interval> {
    let x2 = square(x);
    let mut power = x;
    let mut coefficient = point(1.);
    let mut sum = point(0.);
    for n in 0..48 {
        sum += coefficient * power;
        coefficient *=
            point(((2 * n + 1) * (2 * n + 1)) as f64) / point((2 * (n + 1) * (2 * n + 3)) as f64);
        power *= x2;
    }
    let remainder = power / (point(1.) - x2);
    Ok(sum + interval(0., remainder.sup())?)
}

fn asin_point(x: f64) -> Result<Interval> {
    if x < 0. {
        return Ok(-asin_point(-x)?);
    }
    if x <= 0.5 {
        return asin_small(point(x));
    }
    Ok(Interval::FRAC_PI_2 - point(2.) * asin_small(((point(1.) - point(x)) / point(2.)).sqrt())?)
}

pub fn asin(x: Interval) -> Result<Interval> {
    finite_domain(x, -1., 1.)?;
    interval(asin_point(x.inf())?.inf(), asin_point(x.sup())?.sup())
}

fn acos_point(x: f64) -> Result<Interval> {
    if x < 0. {
        return Ok(Interval::PI - acos_point(-x)?);
    }
    if x >= 0.5 {
        return Ok(point(2.) * asin_small(((point(1.) - point(x)) / point(2.)).sqrt())?);
    }
    Ok(Interval::FRAC_PI_2 - asin_small(point(x))?)
}

pub fn acos(x: Interval) -> Result<Interval> {
    finite_domain(x, -1., 1.)?;
    interval(acos_point(x.sup())?.inf(), acos_point(x.inf())?.sup())
}
