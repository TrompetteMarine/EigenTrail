# Python and R quickstart

Install from the repository as described in the [README](../README.md). The
ordinary analysis interface needs covariance matrices, observation times,
conditional covariance-error radii, and a global covariance regularity bound.

## Python

```python
import numpy as np
import eigentrail

covariances = np.array([[[0.75, 0.0], [0.0, 0.25]]] * 2, dtype=np.float64)
times = [0.0, 1.0]
report = eigentrail.track(covariances, times, radii=[2.0**-20] * 2, lipschitz=0.0)
assert report["completed"]
print("Leading eigenvalue:", report["eigensystems"][0]["values"][0])
print("Leading projector bound:", report["segments"][0]["tubes"][0]["bound"])
```

The leading eigenvalue is 0.75 and the bound is below 0.00001 for this synthetic
constant path. This example supplies its error and regularity premises.
Python requires float64 arrays; strided arrays are supported. Convenience methods
return dictionaries with decoded numerical values.

## R

```r
library(eigentrail)
covariances <- array(rep(c(0.75, 0.0, 0.0, 0.25), 2), dim = c(2, 2, 2))
times <- c(0.0, 1.0)
report <- track(covariances, times, radii = rep(2^-20, 2), lipschitz = 0.0)
stopifnot(grepl('"completed":true', report, fixed = TRUE))
cat("Checked TRACE JSON:", nchar(report), "characters\n")
```

R expects double arrays and returns canonical JSON text. Use the JSON methods in
either language to preserve exact certificates for later independent checking.

## Continuous histories

Call `track_history(covariances, times, radii, lipschitz, query_times)` for
between-date block summaries and local/reference displacement. Optional
`common_frame_weights` declares one common eigenframe over the entire path.
A simple weighted anchor does not itself justify that premise.

Returned bounds and lineage alternatives are outer information. Arbitrary
choices from marginal intervals need not form a complete history. A checked
witness has its own feasibility status. See the [API guide](api.md).

## Errors and resources

Rust validation and numerical/resource refusals raise Python `ValueError`;
argument/type conversion can raise `TypeError`. Native interruption raises
`KeyboardInterrupt`. R raises `eigentrail_error` with a `kind` field.

The default managed allocation policy is 512 MiB per analysis, shared across
producer, checker, working buffers and retained output. Caller-owned arrays,
interpreter memory, allocator overhead and total process RSS are separate.
The 256-feature and 10,000-date ceilings are independent: their largest joint
stack does not fit the default budget and is rejected before copying.
Cancellation or exhaustion never yields a completed certificate.
