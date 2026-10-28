# Gaussian calibration for covariance histories

EigenTrail can calculate simultaneous covariance-error radii from a declared
Gaussian observation model, then propagate those radii through a continuous
history. Use `analyze_histories_json` to produce the report and
`check_histories_json` to check the original request/report pair independently.
The checker verifies numerical conformance; it cannot observe or establish the
sampling model.

Run the [Python example](../examples/gaussian-history.py) after installing
EigenTrail:

```sh
python examples/gaussian-history.py
```

The example uses three exact represented covariance designs. It generates no
observations and makes no empirical coverage claim. The sample counts are
declared design parameters, not evidence that data were collected.

| Input at each of ten dates | Declared observations per date | Leading result |
|---|---:|---|
| `diag(16, 1, 0.25)` | 32,768 | A line meeting the requested projector accuracy |
| `diag(4, 4, 0.25)` | 32,768 | A proper rank-two subspace; the tied directions remain unlabelled |
| `diag(16, 1, 0.25)` | 64 | Whole space; no directional recovery |

All three use a `0.3` projector-error target and supplied global covariance
regularity `K=0`. This example-specific target does not change other examples or
acceptance targets. Repeated estimated matrices do not prove that the population
covariance is constant. Supply a justified global bound on the intended
population path; observed slopes alone do not establish it.

## Match calibration to the data

For a Gaussian interpretation, observations must be independent and identically
distributed **across replicas within each date**, with one covariance at that
date. Independent subjects followed over time can have dependent observations
across dates. The simultaneous calibration allows that cross-date dependence;
it does not require independent dates. A single dependent time series is not
automatically a set of independent Gaussian replicas.

The request records the normalization explicitly:

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

With a known mean, form the scatter around that mean and use `nu=n`. With a mean
estimated from these same independent Gaussian observations, set
`estimated_mean=True` and use `nu=n-1`. Set `divisor` to the divisor actually used
to produce the input matrix. EigenTrail normalizes it to scatter divided by
`nu`; do not silently substitute a differently normalized estimator. `observations`
is a positive integer count, not an effective sample size guessed from temporal
correlation.

`alpha` is the total simultaneous failure budget across the supplied dates.
`auxiliary_risk` reserves part of that budget for separately justified events and
must be smaller than `alpha`. Supply per-date `correction` bounds for charged
numerical or measurement transformations. They are zero in this example because
its matrices are supplied directly as exact binary64 designs. Covariance
estimation, missing-data handling, standardization, shrinkage or other
preprocessing needs its own stated estimand and justified errors; it is not
automatically covered by selecting Gaussian calibration.

## Read the result at its actual resolution

The example reads the leading query block, its rank span and projector bound.
A block with rank one describes a line; a block with rank between one and the
feature dimension describes a proper subspace. A tied leading pair can identify
its plane without identifying individual eigenvectors inside it.

A full-dimensional projector is the identity and can have error bound zero.
That is a valid whole-space enclosure, not precise tracking of the first
eigenvector. Count directional target attainment only for a proper block whose
checked tube is informative and whose query projector bound is below the
requested target. Report unresolved output rather than forcing a direction.

Strength, share, rank and lineage summaries are outer information on a history
family. Arbitrary pointwise choices from them need not form one realizable
history. Keep any checked witness's feasibility status separate.

The example encodes finite floats as canonical IEEE-754 bit strings and decodes
the returned JSON only for display. Preserve the original canonical request and
report strings for checking; ordinary JSON serialization of a decoded display
dictionary is not the certificate format. See [serialization](serialization.md)
and the [API guide](api.md).
