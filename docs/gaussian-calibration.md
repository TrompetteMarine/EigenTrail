# Gaussian calibration for covariance histories

EigenTrail can calculate simultaneous covariance-error radii from a declared
Gaussian observation model and propagate them through a continuous history.
`analyze_histories_json` produces the report; `check_histories_json` checks the
same request/report pair. Numerical checking cannot establish the sampling model.

After [installation](installation.md), run from a source checkout:

```sh
python examples/gaussian-history.py
```

This example supplies three represented covariance designs. It generates no
observations, and its sample counts are declared model parameters.

| Covariance at each of ten dates | Observations per date | Leading result |
|---|---:|---|
| `diag(16, 1, 0.25)` | 32,768 | A line meeting the requested projector accuracy |
| `diag(4, 4, 0.25)` | 32,768 | A rank-two subspace; internal directions remain unlabelled |
| `diag(16, 1, 0.25)` | 64 | Whole space; no directional recovery |

All three request projector error below `0.3` and assume global population
regularity `K=0`. Repeated input estimates do not establish constant population
covariance. Supply a justified global bound; observed slopes alone do not provide
one.

## Match calibration to your observations

Observations must be independent and identically distributed across replicas
within each date, with one Gaussian covariance at that date. The same subjects
may be followed across dates: the simultaneous calibration allows cross-date
dependence. A single dependent time series does not automatically supply
independent Gaussian replicas.

Before canonical encoding, the calibration fields are:

```python
"calibration": {
    "kind": "Gaussian",
    "observations": n,
    "estimated_mean": False,
    "divisor": float(n),
    "alpha": 0.05,
    "auxiliary_risk": 0.0,
}
```

For a known mean, form scatter around that mean and use degrees of freedom
`nu=n`. If the mean is estimated from the same independent Gaussian observations,
set `estimated_mean=True`; then `nu=n-1`. Set `divisor` to the divisor actually
used in the supplied matrix. EigenTrail normalizes to scatter divided by `nu`.
`observations` is an integer count, not a guessed effective sample size.

`alpha` is the total simultaneous failure budget over the supplied dates.
`auxiliary_risk` reserves part for separately justified events and must be smaller
than `alpha`. Per-date `correction` bounds charge numerical or measurement
transformations. The example uses zero because its input matrices are supplied
directly as exact binary64 designs. Missingness, shrinkage, standardization and
other preprocessing need an explicit estimand and justified errors; choosing
Gaussian calibration does not automatically account for them.

## Read results at their supported resolution

A rank-one block describes a line. A larger proper block describes a subspace;
a tied leading pair may identify a plane without identifying either internal
eigenvector. A full-dimensional projector is the identity and may have zero error
bound while providing no directional recovery.

Directional target attainment requires a proper block, an informative tube and a
query projector bound below the requested target. Preserve unresolved results
instead of selecting an unsupported direction. Strength, share, rank and
association alternatives are outer information; arbitrary pointwise choices need
not form one complete history. Witness feasibility remains separate.

The example encodes finite floats as canonical bit strings and decodes copies
for display. Preserve the original request/report strings for checking. See the
[JSON format](serialization.md), [API guide](api.md) and
[lineage summaries](lineage-summary.md).
