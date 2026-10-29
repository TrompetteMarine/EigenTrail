# Python and R quickstart

[Install EigenTrail](installation.md), then supply covariance matrices,
observation times, covariance-error radii, and a global regularity bound for the
population covariance path. All measurements must use fixed coordinates and units.

## Python

```python
import numpy as np
import eigentrail

covariances = np.array([[[0.75, 0.0], [0.0, 0.25]]] * 2, dtype=np.float64)
report = eigentrail.track(
    covariances, times=[0.0, 1.0],
    radii=[2.0**-20] * 2, lipschitz=0.0,
)
print("Leading eigenvalue:", report["eigensystems"][0]["values"][0])
print("Leading projector bound:", report["segments"][0]["tubes"][0]["bound"])
print("Completed:", report["completed"])
```

This supplied constant path has leading eigenvalue 0.75 and a leading-projector
bound below 0.00001. The error radii and `lipschitz=0.0` describe this example's
premises. Repeated estimated matrices alone would not establish constant
population covariance.

Python accepts NumPy `float64` arrays with shape `(dates, features, features)`.
Strided arrays are supported. Convenience methods return dictionaries with
ordinary decoded numerical values.

## R

```r
library(eigentrail)
covariances <- array(rep(c(0.75, 0.0, 0.0, 0.25), 2), dim = c(2, 2, 2))
report <- track(covariances, times = c(0.0, 1.0),
                radii = rep(2^-20, 2), lipschitz = 0.0)
cat(report, "\n")
```

R accepts double arrays with shape `(features, features, dates)` and returns
canonical JSON text. Finite numerical values are encoded as `f64:` bit strings;
see [serialization](serialization.md) before processing that JSON yourself.

## Query a continuous history

Python:

```python
history = eigentrail.track_history(
    covariances, times=[0.0, 1.0], radii=[2.0**-20] * 2,
    lipschitz=0.0, query_times=[0.0, 0.5, 1.0],
)
configuration = history["configurations"][0]
print(configuration["feasibility"]["kind"])
print(configuration["continuous_workflow"][0]["queries"])
```

R, using the R array above:

```r
history <- track_history(covariances, times = c(0.0, 1.0),
    radii = rep(2^-20, 2), lipschitz = 0.0, query_times = c(0.0, 0.5, 1.0))
```

The query results bound block strength, variance share, rank and displacement.
Adding query times does not add covariance observations. Returned intervals and
association alternatives are outer bounds; arbitrary choices from them need not
form one realizable complete path. Witness feasibility is reported separately.

Optional `common_frame_weights` selects a model with one fixed eigenframe over
the entire path. Supply it only when that premise is justified. See the
[API guide](api.md) and [lineage summary guide](lineage-summary.md).

## Errors and resource limits

Inputs must be finite; observation times must be strictly increasing. Missing
values are rejected. Python native validation/resource errors raise `ValueError`,
conversion errors may raise `TypeError`, and interruption raises
`KeyboardInterrupt`. R raises an `eigentrail_error` condition with a `kind` field.
Scientific ambiguity is returned in the report, rather than as an invalid-input
error.

The default requested managed-memory budget is 512 MiB per analysis, shared by
the producer, checker and retained output. Caller-owned arrays, interpreter
memory, allocator overhead and total process RSS are separate. Read the report's
`resource_qualification` for the scope of that budget and cancellation handling. The independent ceilings of 256 features and 10,000
dates do not promise that their largest combination fits the budget. Cancellation
and budget exhaustion do not produce completed certificates.
