\name{track}
\alias{track}
\alias{track_history}
\alias{track_json}
\alias{check_json}
\alias{analyze_jumps_json}
\alias{check_jumps_json}
\alias{analyze_crossings_json}
\alias{check_crossings_json}
\alias{analyze_transport_json}
\alias{check_transport_json}
\alias{analyze_histories_json}
\alias{check_histories_json}
\alias{analyze_lineage_summary_json}
\alias{check_lineage_summary_json}
\alias{analyze_common_frame_json}
\alias{check_common_frame_json}
\title{Local covariance trajectories under declared TRACE assumptions}
\usage{
track(covariances, times, radii, lipschitz)
track_history(covariances, times, radii, lipschitz, query_times,
              common_frame_weights = NULL)
track_json(request)
check_json(request, report)
analyze_jumps_json(request, model)
check_jumps_json(request, model, report)
analyze_crossings_json(request, window)
check_crossings_json(request, window, assessment)
analyze_transport_json(request)
check_transport_json(request, assessment)
analyze_histories_json(request)
check_histories_json(request, report)
analyze_lineage_summary_json(request)
check_lineage_summary_json(request, report)
analyze_common_frame_json(request, model)
check_common_frame_json(request, model, report)
}
\arguments{
\item{covariances}{Double array with dimensions features, features, dates.}
\item{times}{Strictly increasing finite double vector.}
\item{radii}{One conditional nodal error bound per date.}
\item{lipschitz}{Declared global covariance regularity bound.}
\item{query_times}{Finite double vector inside the observation domain. Query order
and duplicates are preserved.}
\item{common_frame_weights}{Optional nonnegative double anchor weights, one per
date, with positive total. NULL selects the continuous model. Supplied weights
declare one fixed eigenframe diagonalizing the entire covariance path.}
\item{request}{Canonical eigentrail.trace.v1 request JSON; history functions accept
eigentrail.histories.v1 and eigentrail.histories.v2. The initialized transport
functions accept eigentrail.transport.v1 geometry requests. The lineage summary
functions accept eigentrail.lineage-summary.v1 with history (a v2 request)
and a finite positive delta (projector-motion tolerance).}
\item{report}{Canonical report JSON to check.}
\item{window}{Canonical crossing window JSON with block, centre, secant,
attachments, bracket, curvature and exact_collision_assumed. Indices are zero-based;
the block is half-open. Attachments strictly straddle the collision bracket, which
must lie inside the secant.}
\item{assessment}{Canonical eigentrail.crossings.v1 or eigentrail.transport.v1 assessment JSON to check.}
\item{model}{Canonical jump model JSON: at_most_one_per_cell,
minimum_applies_to_all_candidates and optional minimum_jump. Common-frame models
use weights and common_frame_assumed.}
}
\value{Canonical JSON for tracking, or TRUE after successful checking. Invalid,
cancelled, exhausted and over-budget work raises a structured Rust error at the
boundary. The caller-owned payload estimate is an attribute on track and
track_history output.}
\description{Rust owns the scientific algorithms. Statistical coverage remains
conditional on supplied sampling and regularity assumptions. No missing values,
implicit repair, changing feature units or bootstrap calibration are accepted.}
\details{The separate jump route interprets the request Lipschitz bound between
jumps. It retains complete-path occupancy constraints and uncertain timing;
it does not interpolate continuously through jumps or establish feasibility
of every retained candidate. All path and sampling premises remain conditional.

History v2 Jumps configurations return jump_workflow through the existing JSON
history functions. Observation queries retain nodal bounds. Inside a retained
jump cell, both one-sided PSD-ball alternatives remain present. Smooth components
carry checked queries with lineage and reference displacement restarted after
retained jump cells. OriginalStepWitness verifies one complete right-continuous
step path if the candidate-wide jump minimum permits it. This witness does not
narrow the outer query bounds or identify jump times. Components retain the
original global Gaussian calibration and event budget.

The crossing route returns a checked assessment conditional on an exact
finite-multiplicity collision, a C2 path, declared derivative bounds and a collision
bracket. Small observed gaps do not establish these premises. Missing structural
premises or unresolved strict gates retain an unresolved assessment. Numerical
exhaustion, cancellation and resource refusal remain errors. History v2 can retain
TransverseCrossings configurations containing 1 to 16 windows; crossing success
does not establish whole-history feasibility or restore bare-continuous identities.

Initialized transport accepts full square frame proposals, a selected half-open
block, an initial frame and an orthogonal gauge. The exact polar factors define
reference and reconstructed short-geodesic paths; the checked assessment retains
speed, endpoint, initialization and accumulated numerical errors. Its standalone
scope covers known geometry. A history v2 InitializedTransport configuration adds
an independently checked population-covariance bridge using observation spacing,
calibrated uncertainty and global regularity. Geometry can resolve while that
population bridge remains unresolved. A fine geometry mesh does not create extra
covariance observations or prove whole-history feasibility.

The track_history convenience route accepts an ordinary double array and constructs
one continuous configuration using eigentrail.histories.v2. It returns checked
block strength, share, ordered-rank and displacement bounds, conservative lineage
candidates and separately checked query values from one affine witness when verified.
Marginal bounds and lineage reachability are outer information; arbitrary choices
within them need not form one realizable path. Merge/split ambiguity does not create
an individual identity. An unverified witness is unresolved, not proof of infeasibility.

Supplying common_frame_weights selects the existing eigentrail.histories.v1
common-frame configuration. The supplied radii, regularity and common-frame model
remain premises; the call does not infer them from the data. A simple anchor does
not establish the common-frame model. Tied anchors retain unresolved individual labels.

The JSON history route retains all declared continuous, jump and common-frame configurations,
query variables on one joint path, mask alternatives and block merge/split
constraints. A smooth cell is reported only when every retained mask is smooth
there. Common-frame members impose one frame on the entire
path and query states; tied anchors remain UnresolvedAnchor without individual
labels. Marginal query intervals do not assert that arbitrary choices within them
are jointly attainable. The automatic exact-polar affine witness checks unlabelled
existence of one PSD path in the common-frame family. Explicit coordinate-diagonal
affine witnesses are checked as exact supplied matrices, with original and widened
feasibility reported separately. Non-diagonal explicit common-frame witnesses remain
UnsupportedModel. A widened witness does not enlarge the original query enclosures.

The lineage summary route presents continuous or common-frame history bounds as
flat rows. It retains the checked nested history, including separate feasibility.
Ordered-block origins are outer block indices, not latent labels; common-frame
axes rely on the supplied global premise. Whole-space output is unresolved for
directions. Motion WithinTolerance requires an upper displacement bound strictly
below delta; BeyondTolerance requires a lower bound strictly above it. Motion
and requested projector accuracy are separate fields. A fixed common-frame axis
can have zero motion while its orientation accuracy remains insufficient.

The standalone common-frame route conditions on a supplied common eigenframe
and verifies a simple weighted anchor before returning persistent axis labels,
strengths and possible ranks. Tied anchors and absent premises return explicit
exhaustion without individual labels. A simple anchor does not establish the
common-frame model.}
\examples{
covariances <- array(0, dim = c(2, 2, 3))
for (j in seq_len(3)) covariances[, , j] <- diag(c(2, 1))
times <- c(0, 0.5, 1)

# Zero radii and regularity are declared for this constant example.
# Output is canonical JSON; no JSON package is needed to call the API.
report <- track(covariances, times, radii = rep(0, 3), lipschitz = 0)
stopifnot(is.character(report), length(report) == 1L)
history <- track_history(covariances, times, rep(0, 3), 0,
                         query_times = c(0.25, 0.75))
stopifnot(is.character(history), length(history) == 1L)
}
