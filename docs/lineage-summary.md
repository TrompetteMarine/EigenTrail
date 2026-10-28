# Read covariance histories as lineage summaries

The lineage summary route presents checked continuous or common-frame histories
as one ordered table. It retains the full checked history beside the table, so
the input assumptions, association alternatives and feasibility evidence remain
available.

Run the represented constant-path example after installing EigenTrail:

```sh
python examples/lineage-summary.py
```

The example uses exact supplied diagonal covariances and deterministic nodal
envelopes. It samples no observations and makes no coverage estimate.

For the CLI, create a new report and check it locally:

```sh
eigentrail-check analyze-lineage-summary examples/requests/lineage-summary-request.json report.json
eigentrail-check check-lineage-summary examples/requests/lineage-summary-request.json report.json
```

Python exposes `analyze_lineage_summary_json(request_json)` and
`check_lineage_summary_json(request_json, report_json)`. R exposes the same names:

```r
library(eigentrail)
request <- paste(readLines("examples/requests/lineage-summary-request.json"), collapse="\n")
report <- analyze_lineage_summary_json(request)
stopifnot(check_lineage_summary_json(request, report))
```

## Request and rows

The `eigentrail.lineage-summary.v1` request contains a complete
`eigentrail.histories.v2` request in `history` and a finite positive `delta`.
`delta` is the tolerated operator-norm change of a projector. Preserve fixed
feature coordinates, units, calibration and declared path regularity.

`rows` preserves configuration order and the original query order, including
duplicates. `configuration` and `query` are zero-based input indices. Each
bounded row contains:

| Field | Interpretation |
|---|---|
| `target` | An ordered spectral block `[start,end)`, or a common-frame anchor axis |
| `information_level` | `Line`, `ProperSubspace`, or `WholeSpace` |
| `strength`, `share`, `rank` | Checked spectral mass, variance share and possible ordinary ranks |
| `projector_error` | Checked uncertainty for the returned projector |
| `reference_displacement` | Motion relative to the first observation date |
| `motion` | `WithinTolerance`, `BeyondTolerance`, or `Unresolved` |
| `accuracy_target_attained` | A proper block meets the separate input projector-accuracy target |

An unresolved anchor or inconsistent outer bounds remains an explicit row state.
Undefined share or displacement quantities retain their tags. Requesting more
query times supplies no additional covariance observations.

Motion uses strict margins: an upper displacement bound below `delta` gives
`WithinTolerance`; a lower bound above it gives `BeyondTolerance`. Equality or
overlap remains `Unresolved`. A whole-space projector is always unresolved for
directional motion, even though the identity matrix has zero projector error.

A common-frame axis can have zero motion under the supplied global fixed-frame
premise while its orientation remains too uncertain for the requested accuracy.
Read `motion` and `accuracy_target_attained` separately. A simple anchor does
not establish the common-frame premise.

## Association and feasibility

For an `OrderedBlock`, `origins` names possible initial **spectral blocks**.
These indices are not generating-channel identities. Its `persistent` flag
describes the checked outer association; it does not produce a feasible complete
history by itself. Internal directions of a tied block remain unlabelled.

Inspect `history.configurations[c].feasibility` separately. An original witness,
a widened witness and unresolved feasibility retain different meanings. A
widened witness does not widen the original query enclosures. Arbitrary marginal
row choices need not form one jointly realizable path.

This route accepts continuous and common-frame configurations. Other model
classes retain their existing dedicated APIs. Statistical calibration and path
assumptions remain premises; successful numerical checking does not discover
the sampling law or hidden ancestry. Keep the original canonical request/report
strings for checking and decode copies only for display. See
[serialization](serialization.md) and the [API guide](api.md).
