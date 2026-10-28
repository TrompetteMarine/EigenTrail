# API guide

## Arrays

Python exports `track(covariances, times, radii, lipschitz)` and
`track_history(covariances, times, radii, lipschitz, query_times,
common_frame_weights=None)`. R uses the same arguments, with `NULL` for optional
weights. Arrays must be finite, times strictly increasing, and features expressed
in fixed measurement coordinates. Input shapes and return types are in the
[quickstart](quickstart.md).

`track` returns eigensystems, separated spectral tubes, checked association graphs
and summaries. `track_history` additionally returns intermediate-time block
strength/share/rank, local/reference displacement, lineage alternatives and
separately checked witnesses. Default histories use no common-frame premise;
providing weights explicitly selects that additional model.

## Canonical JSON

The same checked JSON routes are exported in Python and R. Each analysis method
returns canonical JSON; each check method validates the supplied input/report.
Python additionally exposes `track_covariances` and `track_history_covariances`,
the native array routes returning canonical JSON rather than decoded dictionaries.

| Analyze | Check | Input arguments before the report |
|---|---|---|
| `track_json` | `check_json` | request |
| `analyze_histories_json` | `check_histories_json` | request |
| `analyze_lineage_summary_json` | `check_lineage_summary_json` | request |
| `analyze_common_frame_json` | `check_common_frame_json` | request, model |
| `analyze_jumps_json` | `check_jumps_json` | request, model |
| `analyze_crossings_json` | `check_crossings_json` | request, window |
| `analyze_transport_json` | `check_transport_json` | request |

The request interface supports conditional error envelopes or the Gaussian
scatter calibration with declared degrees of freedom, mean/divisor conventions,
corrections and event budgets. A covariance stack does not establish a sampling
law. Inspect [request examples](../examples/requests/) or the Rust public types
with `cargo doc --no-deps --open` for the full field definitions.

## Command line

```sh
eigentrail-check track examples/requests/rotation-request.json report.json
eigentrail-check check examples/requests/rotation-request.json report.json
```

Analysis commands refuse an existing output file. Checking runs locally without
network access. Other commands pair `analyze-histories`/`check-histories`,
`analyze-common-frame`/`check-common-frame`, `analyze-jumps`/`check-jumps`,
`analyze-crossings`/`check-crossings`, `analyze-transport`/`check-transport`, and
`analyze-lineage-summary`/`check-lineage-summary`.
Common-frame/jump commands take REQUEST MODEL REPORT; crossings take
REQUEST WINDOW REPORT; the remaining commands take REQUEST REPORT.
Use `--help` and `--version` for discovery.

The [lineage summary guide](lineage-summary.md) explains the checked row table,
strict displacement tolerance, accuracy flags and separate history feasibility.

## Scientific scope

Every uncertainty statement is conditional on its declared calibration and
regularity/model assumptions. Observed slopes do not certify global regularity.
Near ties, separated blocks can remain informative while individual identities
remain unresolved. Undefined shares and matching alternatives are retained.

Common-frame tracking assumes a fixed eigenframe; its persistent axes can change
instantaneous PC rank. Crossings require an explicit exact-collision premise,
curvature/regularity bounds and straddling attachments. Small observed gaps do not
establish those premises. Jumps retain possible cells and half-open timing
brackets; smooth interpolation does not cross a possible jump.

Initialized transport checks known exact-polar geometry, initialization, speeds
and numerical errors. A covariance-history transport bridge additionally charges
statistical and between-observation uncertainty. Finer internal geometry spacing
does not create more covariance observations.

Marginal summaries, local matches and symbolic alternatives are not evidence of
a realizable complete history. Original and widened witnesses retain separate
scope. Conservative bounds can be uninformative at practical sample sizes;
coverage and useful individual-mode accuracy are distinct properties.

## Rust

`eigentrail_core::track(&TraceRequest)` produces a report and
`eigentrail_check::check(&TraceRequest, &TraceReport)` independently reconstructs
and verifies its decisions. The remaining public modules provide the JSON-route
models above. Invalid input, cancellation and resource refusal are structured
errors; scientific ambiguity remains structured report data.
