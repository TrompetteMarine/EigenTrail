# API guide

## Covariance arrays

Python and R provide these convenience methods:

```text
track(covariances, times, radii, lipschitz)
track_history(covariances, times, radii, lipschitz, query_times,
              common_frame_weights=None)
```

R uses `NULL` for absent weights. Python arrays are `float64` with shape
`(dates, features, features)`; R arrays are double with shape
`(features, features, dates)`. Values must be finite and observation times strictly
increasing. Keep feature units and measurement coordinates fixed.

`track` returns eigensystems, separated spectral tubes, association graphs and
summaries. `track_history` adds query-time block strength/share/rank, displacement,
lineage alternatives and separate witness feasibility. The default is a
continuous path; supplying `common_frame_weights` declares a fixed eigenframe for
the entire path.

Python convenience methods return decoded dictionaries; R returns canonical JSON.
Python also exports `track_covariances` and `track_history_covariances` with the
same arguments, returning canonical JSON. See the [quickstart](quickstart.md).

## JSON interfaces

These functions are available in both Python and R. Analyze functions return
canonical JSON; check functions take the same inputs followed by the report and
return `True` in Python or `TRUE` in R on success. Invalid reports raise errors.

| Analyze | Check | Inputs before the report |
|---|---|---|
| `track_json` | `check_json` | request |
| `analyze_histories_json` | `check_histories_json` | request |
| `analyze_lineage_summary_json` | `check_lineage_summary_json` | request |
| `analyze_common_frame_json` | `check_common_frame_json` | request, model |
| `analyze_jumps_json` | `check_jumps_json` | request, model |
| `analyze_crossings_json` | `check_crossings_json` | request, window |
| `analyze_transport_json` | `check_transport_json` | request |

Use [example requests](../examples/requests/) and the
[canonical format](serialization.md) to construct input strings. Preserve the
original request/report pair for later checking. The [Gaussian guide](gaussian-calibration.md)
explains calibration from a declared observation model.

## Command line

After [installing the CLI](installation.md#rust-and-command-line), run these from
a source checkout, using a new output filename:

```sh
eigentrail-check track examples/requests/rotation-request.json report.json
eigentrail-check check examples/requests/rotation-request.json report.json
```

Analysis commands refuse existing output files. All checking runs locally.

| Analyze / check commands | Arguments |
|---|---|
| `track` / `check` | REQUEST REPORT |
| `analyze-histories` / `check-histories` | REQUEST REPORT |
| `analyze-lineage-summary` / `check-lineage-summary` | REQUEST REPORT |
| `analyze-common-frame` / `check-common-frame` | REQUEST MODEL REPORT |
| `analyze-jumps` / `check-jumps` | REQUEST MODEL REPORT |
| `analyze-crossings` / `check-crossings` | REQUEST WINDOW REPORT |
| `analyze-transport` / `check-transport` | REQUEST REPORT |

Use `eigentrail-check --help` for syntax and `--version` for the installed version.

## Interpret results under the declared model

Uncertainty bounds are conditional on calibration and path assumptions. Observed
slopes do not certify a global regularity bound. Near ties, a separated block may
remain informative while its individual directions remain unresolved. Undefined
shares and association alternatives are retained explicitly.

A common-frame model assumes one fixed eigenframe; its axes may change ordinary
PC rank. A crossing model needs the declared exact-collision and regularity
premises with straddling attachments; a small observed gap does not establish
them. Jump models retain candidate cells and half-open timing brackets, and do
not interpolate smoothly across a possible jump.

Initialized transport takes exact-polar geometry with initialization, speed and
numerical-error bounds. Transport inferred from covariance observations also
needs its covariance-history bridge, charging statistical and between-date
uncertainty. Internal refinement does not add observations.

Marginal intervals and association alternatives do not establish a jointly
realizable history. Original and widened witnesses retain separate scopes.
Conservative bounds can be uninformative; valid coverage and accurate recovery
of an individual direction are different properties. The
[lineage summary guide](lineage-summary.md) explains how to read motion, accuracy
and feasibility separately.

## Rust

`eigentrail_core::track(&TraceRequest)` produces a `TraceReport`;
`eigentrail_check::check(&TraceRequest, &TraceReport)` checks it independently.
Public modules expose the additional models listed above. Use
`eigentrail_core::serialization::{encode, decode}` for the wire format.

Invalid input, resource refusal, numerical exhaustion and cancellation use
`Error::{Invalid, Resource, Exhausted, Cancelled}`. Ordinary scientific ambiguity
is report data. See [installation](installation.md) for Git dependencies and the
[quickstart](quickstart.md#errors-and-resource-limits) for interface errors and
memory limits.
