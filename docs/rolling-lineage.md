# Observed-date rolling lineage

Unreleased candidate **0.3.0** adds `eigentrail.rolling-lineage.v1`. It compares covariance
subspaces at observed dates in one declared feature chart. It does not infer a
continuous path between observations. The existing `lineage-summary.v1` API and
its strict projector-displacement cutoff retain their existing meaning.

All covariance matrices must be real, finite, exactly symmetric and independently
certifiable as positive definite. Python arrays have axes `(date, feature,
feature)`; R's array convention is `(feature, feature, date)`. Raw positive-lag
cross-covariances can be asymmetric and are outside this input contract. A missing
date must be an explicit missing slot in the canonical request. Invalid bytes
never stand for a missing observation. No matrix is silently repaired or imputed.

## Four dependent views

The same root covariance S supplies covariance S, precision S inverse,
correlation N(S), and partial correlation `2I - N(S inverse)`, where N normalises
the diagonal to one. Precision reverses covariance ranks. Correlation and partial
correlation coincide in two dimensions; their directions can differ in higher
dimensions. Partial correlation can be indefinite. Its signed spectral values
are reported without an explained-variance percentage. SPD geometry applies to
the declared positive-definite reference, not to an indefinite partial view.

These four views contain dependent evidence derived from one matrix. Repeated
agreement across them is not four independent replications. A matched spectral
direction is a statement about covariance geometry; interpreting it as a
behavioural factor requires a separate measurement model. Correlation loadings
use standardised coordinates. Converting them to scores in raw variables needs
the appropriate coordinate transform. Changes of units or an arbitrary rotation
can change cross-view conclusions.

## Query and output

Use `track_rolling` with explicit `tolerance`, `mode`, `lag_cap`, `founders`,
feature IDs, units and chart. Python requires NumPy float64; strided arrays are
supported. `track_rolling_panel` additionally requires the explicit axes tuple
`('subject','feature','feature','date')` for a `(K,N,N,T)` array and subject IDs.
It transposes representation and keeps subjects separate.

Founder dictionaries contain `subject` (default 0), `date` (zero-based original
slot index), `lens`, `ranks` (a consecutive list of human ranks 1 through N), and
optional `policies`. The wire uses zero-based half-open rank intervals; human
ranks `[1,2]` become wire `[0,2]`. Lens order is covariance, precision,
correlation, partial. Masks select ranks only when they cover a whole retained
group. A mask never selects an arbitrary basis vector inside a tie.

The result retains `request_json` and `report_json` unchanged, with a decoded
mapping for documented numerical fields and `records(kind)` for chunk records.
Descriptive text is retained verbatim. Raw `analyze_rolling_lineage_json` and
`check_rolling_lineage_json` remain available in Python and R. The CLI uses:

```sh
eigentrail-check analyze-rolling-lineage request.json report.json
eigentrail-check check-rolling-lineage request.json report.json
```

Analysis independently checks before writing a fresh destination. Existing files
are refused. Request hashes, ordered chunk chains and completion records bind
the report to the complete candidate scope. The checker reconstructs omitted
competitors; hashes alone do not establish scientific correctness.

| Record family | Meaning |
| --- | --- |
| `date_evidence` | Root and transformed numerical witnesses |
| `partitions`, `views` | Typed groups, spectrum, uncertainty and projector columns |
| `edges` | Selected/full graph-edge provenance and bounds |
| `candidates` | Packed decisions for every eligible competitor |
| `windows` | All-lens and per-lens-pair counts and outcomes |
| `founders` | Direct agreement and query-specific reach, separately |
| `paths` | Selected attaining paths, with original slot gaps |
| `norms` | Sparse independently checked block-angle witnesses |

Selected records use these fields. IDs are zero-based and stable within the
request; presentation tables retain their source IDs.

| Fields | Interpretation |
| --- | --- |
| `subject`, `date`, `lens`, `rank` | Typed group coordinates; `date` is the original slot, `rank` is a descending lens-spectrum half-open interval |
| `node`, `columns` | Group ID and its retained reference-eigensystem columns; these can differ from lens ranks under inversion |
| `point`, `geometry`, `method_bracket` | Numerically enclosed reference angle, region-widened geometry, and the declared conservative decision bracket |
| `source`, `target`, `ordinal`, `smaller_side` | Edge provenance, ordered candidate index and containment direction |
| `query`, `direct`, `families` | Founder query index, separate all-later direct result and named graph-family bounds |
| `lower`, `upper`, `lower_parent`, `upper_parent`, `reach` | Two bottlenecks, their attaining predecessors, and the query-cutoff result |
| `nodes`, `slot_gaps`, `cost` | One selected path, its original index gaps and its attained bottleneck in degrees |

Founder policies are `resolved_line`, `outer_resolved`, `typed_block_graph` and
`outer_relaxed`. The first two share the `line_only` bound family; the last uses
`relaxed_internal_lines`. Python's `lens_pairs` declares ordered allowed travel
and `rank_masks` maps each lens to an ascending list of selected human ranks. Empty masks
produce no eligible candidates. Counts projection retains coverage and group
partitions but omits detailed view and edge rows.

## Bounds and failure states

Lines use `acos(abs(u transpose v))`. Equal-dimensional blocks use their largest
principal angle. Unequal groups use containment of the smaller group in the
larger, recording the smaller side. Full-space containment is trivial and gives
no identity to an internal line. Exact multiplicities, numerical near ties and
regional clusters have different labels. A group's full projector and spectrum
are retained; its internal basis is arbitrary.

For an enclosing angle `[L,U]` in degrees and epsilon, agreement requires
`U <= epsilon`, exclusion requires `L > epsilon`, and the remaining case is
open. Numerical uncertainty is retained even in point mode. A mathematical
threshold equality that finite arithmetic cannot resolve remains open.
The conservative paper policy retains a mixed line/block regional method
bracket `[0,90]` separately from the geometric containment bound.

Conditional mode requires a joint-event identity, radius-provider description
and nonnegative Thompson-region radius rho. A supplied rho of 0.1 has no implied
sampling level. Numerical alignment is added once. The sharp planar correlation
interval allows spectral changes even when its axes remain fixed. Unsupported
normalisation transfers retain conservative whole-space information and an
explicit unsupported reason, rather than becoming point-data certainty.

A window is an agreement if any eligible edge agrees. Without agreement, an
open candidate gives open. A complete break requires a nonempty complete set of
excluded candidates. Empty masks, an empty graph scope, missing data and
unsupported evidence are unavailable; no omitted candidate establishes a break.
Budget exhaustion, cancellation and unresolved numerical certificates refuse the
operation without a successful partial certificate. Increase an explicit budget
or narrow projection/scope and retry in a fresh destination.

## Direct comparisons, travel and bottlenecks

Direct founder agreement compares the founder with all later eligible slots.
Graph reach follows dated edges up to the declared maximum index lag and needs
the same typed middle node at each composition. Same-date aliases do not create
an implicit cross-lens step. Skips retain the original date gap.

`line_only`, `typed_block_graph` and `relaxed_internal_lines` retain distinct
meanings. Relaxed travel through an internal block is outer information and
cannot select consistent internal lines on successive edges. Outer paths can
combine incompatible per-edge witnesses. A lower bottleneck uses lower edge
costs; an upper bottleneck uses upper costs. The latter is the minimum tolerance
certifying that named graph reach, not a calibrated universal scientific cutoff.
No route is tagged explicitly. Equal-cost paths select the smallest typed
immediate attaining predecessor deterministically; they need not be the
lexicographically smallest full path.

The report declares its retrospective data horizon. No future streaming or
prefix-only calibration is implied. Sampling precision additionally depends on
within-date sample size, dependence, joint calibration, separation and threshold
margin. A descriptive match graph does not establish temporal continuity.

## Plotting and installation status

Install the optional `plot` extra from a qualified candidate artifact to use
`plot_window`, `plot_founder`, `plot_spectrum` and `export_html`. Plot imports are
lazy; no TeX is needed. Figure objects support SVG/PDF and PNG with `dpi=300`.
Status text accompanies colour, and offline HTML includes a searchable edge
table and canonical records. Counts-only reports cannot display omitted edges;
request selected edge blocks or a full projection when needed.

The runnable public examples use original synthetic inputs distributed under the
repository's MIT licence. `scripts/test_rolling_public.py` exercises small
diagonal and tied inputs with explicit `--cli` and/or `--python` installed
runtimes and a fresh `--out` directory outside the checkout. Its receipt is a
small interface smoke, separate from scientific and platform qualification.

This is implementation documentation, not a released platform support claim.
Artifact versions, supported OS/architecture/ABI/CPU tuples and installation
commands must be frozen with exact native qualification receipts before release.
The existing specialised Linux CPU restriction remains in force until a
baseline-safe backend and exact wheel have passed their own checks. Generic
Linux publication and Windows publication remain held until qualified.
