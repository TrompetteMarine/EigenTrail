\name{temporal_gaussian}
\alias{temporal_gaussian}
\alias{spd_tube_geometry}
\title{Conditional temporal covariance calibration and SPD tube geometry}
\usage{
temporal_gaussian(dimension, observations, windows, covariance_bound,
                  correlation_bound, alpha, assumptions)
spd_tube_geometry(dimension, centre_floor, radius)
}
\arguments{
\item{dimension}{Positive exact integer feature count, supplied as a double,
no greater than 4294967295.}
\item{observations}{Positive exact integer observation count in this window,
supplied as a double, no greater than 4294967295.}
\item{windows}{Total number of windows in the prespecified simultaneous family,
as a positive exact integer double no greater than 4294967295.}
\item{covariance_bound}{Finite nonnegative prespecified covariance scale V.}
\item{correlation_bound}{Finite prespecified rho in [0, 1].}
\item{alpha}{Finite total failure probability strictly between zero and one.}
\item{assumptions}{Required nonempty description of the statistical premises,
at most 4096 UTF-8 bytes. It is retained as literal text.}
\item{centre_floor}{Supplied finite positive lower bound on the smallest
eigenvalue of an SPD centre matrix.}
\item{radius}{Finite nonnegative operator-norm error radius around that centre.}
}
\value{Canonical checked JSON retaining the complete request, conditional scope
and envelope. Temporal output uses schema eigentrail.temporal-gaussian.v1 and
fields log_tail, frobenius_bound, operator_bound and radius. Geometry output
uses eigentrail.spd-tube.v1 and fields resolved, affine_invariant_radius and
precision_radius. Unresolved radii retain objects with kind Undefined and value
"SPD margin unresolved", rather than a numerical bound. Invalid, exhausted,
cancelled or over-budget work raises a structured eigentrail_error.}
\description{Rust computes and independently checks numerical bounds from
explicit scalar premises. The functions do not inspect data to establish those
premises, identify spectral branches or verify an entire covariance curve.}
\details{
For temporal_gaussian, subtract each observation's known true mean. The full
window of residuals must be jointly Gaussian, with block covariance satisfying
\eqn{\|Cov(X_i,X_k)\|_{op} \le V\rho^{|i-k|}}. The exponent is observation-index
lag, not physical time. Uniform weights and divisor observations define the
second-moment estimator; its target is the arithmetic average of within-time
covariances. Sample centring, fitted envelope constants and adaptive window
selection require separate validity arguments.

Writing p = dimension, n = observations and J = windows, the routine encloses
\eqn{x=p\log(9)+\log(2)+\log(J)-\log(\alpha)},
\eqn{F=V\sqrt{\min(1,(1+\rho^2)/(n(1-\rho^2)))}} and
\eqn{b=V\min(1,(1+\rho)/(n(1-\rho)))}. At rho = 1, F and b equal V.
The radius is an outward upper bound for \eqn{4(F\sqrt{x}+bx)}.
Use the same J and alpha across the prespecified family for simultaneous
coverage of at least 1-alpha. Windows may overlap. Separate one-window calls
do not create that joint event. The assumptions string records the caller's
declaration; its presence does not verify the statistical model.

The temporal radius concerns a window average. To transfer it to instantaneous
covariance, use window_envelope with actual observation times, uniform weights
and a justified operator-norm Lipschitz bound. Account separately for numerical
estimation error and any justified target mismatch.

For spd_tube_geometry, let a = centre_floor and r = radius. If r < a, the
conditional target is SPD. The affine-invariant radius encloses
\eqn{\sqrt{p}\log(a/(a-r))}; the precision error encloses
\eqn{r/(a(a-r))}. The metric convention is
\eqn{g_A(U,V)=tr(A^{-1}UA^{-1}V)}; the fixed-mean Gaussian Fisher distance is
the corresponding affine-invariant distance divided by sqrt(2). If r >= a,
both bounds are unresolved, which does not establish singularity of an actual
target. For r = 0, both bounds are exactly zero. The centre and its eigenvalues
are not supplied to this scalar function, so the floor remains an external
premise. This deterministic conversion preserves an existing event and creates
no additional probability claim or path witness.
}
\examples{
source <- temporal_gaussian(2, 16, 1, 4, 0, 0.125,
    "Known true means, jointly Gaussian residuals, prespecified envelope")
geometry <- spd_tube_geometry(2, 2, 1)
stopifnot(is.character(source), is.character(geometry))
}
