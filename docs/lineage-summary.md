# Read covariance histories as lineage summaries

Lineage summaries present checked continuous or common-frame histories as an
ordered table. The report retains the full checked history, including assumptions,
association alternatives and feasibility evidence.

## Run an example

After [installation](installation.md), run from a source checkout:

```sh
python examples/lineage-summary.py
```

This supplied constant-path example uses exact represented diagonal covariances
and deterministic error envelopes. It generates no observations.

For the CLI, choose a new report filename:

```sh
eigentrail-check analyze-lineage-summary examples/requests/lineage-summary-request.json report.json
eigentrail-check check-lineage-summary examples/requests/lineage-summary-request.json report.json
```

Python:

```python
from pathlib import Path
from eigentrail import analyze_lineage_summary_json, check_lineage_summary_json

request = Path("examples/requests/lineage-summary-request.json").read_text()
report = analyze_lineage_summary_json(request)
check_lineage_summary_json(request, report)
```

R:

```r
library(eigentrail)
request <- paste(readLines("examples/requests/lineage-summary-request.json"), collapse = "\n")
report <- analyze_lineage_summary_json(request)
check_lineage_summary_json(request, report)
```

## Request and rows

An `eigentrail.lineage-summary.v1` request contains a complete
`eigentrail.histories.v2` request in `history` and a finite positive `delta`.
`delta` is the tolerated operator-norm projector displacement. Preserve fixed
feature coordinates, units, calibration and global path regularity.

Rows preserve configuration order and query order, including duplicate queries.
`configuration` and `query` are zero-based input indices. Each bounded row has:

| Field | Meaning |
|---|---|
| `target` | Ordered spectral block `[start,end)`, or common-frame anchor axis |
| `information_level` | `Line`, `ProperSubspace`, or `WholeSpace` |
| `strength`, `share`, `rank` | Spectral mass, variance share and possible ordinary ranks |
| `projector_error` | Checked projector uncertainty |
| `reference_displacement` | Motion relative to the first observation date |
| `motion` | `WithinTolerance`, `BeyondTolerance`, or `Unresolved` |
| `accuracy_target_attained` | A proper block meets the input projector-accuracy target |

Unresolved anchors and inconsistent outer bounds retain explicit row states.
Undefined quantities retain their tags. More query times do not add observations.

Motion uses strict bounds: upper displacement below `delta` gives
`WithinTolerance`; lower displacement above it gives `BeyondTolerance`. Equality
or overlap gives `Unresolved`. Whole-space projectors are always unresolved for
directional motion, even though the identity matrix has zero projector error.

A common-frame axis may have zero motion under the supplied fixed-frame premise
while its orientation remains too uncertain for the requested accuracy. Read
`motion` and `accuracy_target_attained` separately. Anchor weights alone do not
establish the common-frame premise.

## Association and feasibility

For `OrderedBlock`, `origins` lists possible initial spectral blocks, not hidden
generating-channel identities. `persistent` describes the outer association; it
does not establish a feasible complete history. Tied internal directions remain
unlabelled.

Read `history.configurations[c].feasibility` separately. Original witnesses,
widened witnesses and unresolved feasibility have different scopes. A widened
witness does not widen the original query enclosures, and arbitrary row choices
need not form one realizable path.

This route accepts continuous and common-frame configurations. Use the dedicated
APIs for other models. A successful numerical check does not establish the
sampling law or the path assumptions. Preserve canonical request/report strings
for checking; see [serialization](serialization.md) and the [API guide](api.md).
