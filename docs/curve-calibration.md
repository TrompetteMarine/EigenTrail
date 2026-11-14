# Checked calibration, stability and SPD geometry

Version 0.2.0 adds four conditional calculations. Rust produces each result and
an independent Rust checker reconstructs its conditions and bounds. Python
returns decoded convenience reports and raw canonical JSON; R retains checked
canonical JSON. These checks do not infer statistical or path assumptions from data.

| Calculation | Useful output | Required premise or unresolved outcome |
| --- | --- | --- |
| `temporal_gaussian(...)` | A simultaneous operator-error radius for a prescribed window family | Known true means, jointly Gaussian residuals within each window, uniform second moments with divisor n, and an externally prescribed covariance/dependence envelope |
| `window_envelope(...)` | An instantaneous covariance-error radius | The declared window-target error, target mismatch and global operator-norm Lipschitz bound must be valid |
| `track_stability(...)` | Local and reference projector decisions, retaining the complete checked history | Continuous history scope; ambiguous identity or a threshold-overlapping enclosure remains unresolved |
| `spd_tube_geometry(...)` | Affine-invariant SPD distance and precision-matrix error bounds | An externally justified positive centre eigenvalue floor; radius at or above that floor is unresolved |

For temporal calibration, prescribe dimension, observation count, total windows
J, covariance bound V, correlation bound rho and family risk alpha before
inspection. After subtracting known true means, the joint covariance blocks must
satisfy `||Cov(X_i,X_k)||op <= V*rho**abs(i-k)`. Lag means observation-index lag.
Use the same J and alpha across the entire family. Overlap is allowed. Estimated
means, fitted envelopes and adaptively selected windows are outside this contract.
Perfect dependence gives no fictitious sample-size gain.

The window conversion uses the actual weighted absolute time offsets:
`total_radius = window_radius + target_mismatch_bound + lipschitz *
sum(w_i*abs(sample_time_i-target_time))/sum(w_i)`.
Zero weights contribute nothing; the remaining weights are normalized as real
numbers with outward numerical bounds. Symmetry does not cancel drift. Account
for estimator arithmetic error explicitly, and charge each included error,
mismatch and drift term once when passing the radius downstream.

Historical geometric stability declares stable when the checked upper bound is
at most the tolerance, which lies in [0,1]. The separate lineage-summary motion
rule uses a strict upper-bound comparison. Their domains and meanings differ.
Whole-space geometric stability is trivial and identifies no individual direction.
An affine witness can establish the recorded history feasibility without removing
its outer uncertainty. Existing `track_history(..., witness=...)` calls retain
their argument positions; stability has its own entry point.

For SPD geometry, the supplied floor must be a lower bound for the centre's
smallest eigenvalue; this scalar function does not receive or check the matrix.
Radius zero gives exact zero geometry bounds. At `radius >= centre_floor`,
unresolved means the supplied margin cannot certify positivity, not that the
target is singular. The metric is `tr(A^-1 U A^-1 V)`; the fixed-mean Gaussian
Fisher distance has an additional factor `1/sqrt(2)`.

Raw unresolved geometry quantities preserve the documented tagged `Undefined`
reason; Python convenience geometry uses `None`. Descriptive text is preserved.
Raw routes are `temporal_gaussian_json`, `window_envelope_json`,
`analyze_stability_json(request, tolerance)` and `spd_tube_geometry_json`.
The existing CLI history commands check the source history; these new calculations
are exposed through the Rust, Python and R interfaces.

Run the small [composition example](../examples/calibrated-window.py) with an
installed package and NumPy. Its deterministic supplied centres illustrate API
composition; no observations are generated and no Gaussian law is validated.
No additional formal coverage is claimed for these recovered extensions.
