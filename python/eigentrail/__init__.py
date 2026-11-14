"""Local TRACE analysis. Covariances have shape (dates, features, features)."""
import json
import platform
import struct
import sys

if sys.platform.startswith('linux'):
    if platform.machine().lower() != 'x86_64':
        raise ImportError('EigenTrail supports Linux x86_64 with Haswell CPU instructions and kernel AVX support.')
    # Rust 1.99 target-cpu=haswell; Linux calls SSE3 pni, CMPXCHG16B cx16 and LZCNT abm.
    _haswell_flags = set('avx avx2 bmi1 bmi2 cx16 f16c fma fxsr abm movbe pclmulqdq popcnt rdrand sse sse2 pni sse4_1 sse4_2 ssse3 xsave xsaveopt'.split())
    try:
        with open('/proc/cpuinfo', encoding='ascii') as _cpuinfo:
            _cpu_flags = [set(line.partition(':')[2].split()) for line in _cpuinfo
                          if line.partition(':')[0].strip() == 'flags']
    except (OSError, UnicodeError) as _error:
        raise ImportError('EigenTrail requires readable /proc/cpuinfo to verify Linux CPU and kernel support before loading native code.') from _error
    # shortcut: absent kernel flags also refuse capable hosts; use baseline-safe dispatch to broaden support.
    if not _cpu_flags or any(not _haswell_flags <= flags for flags in _cpu_flags):
        raise ImportError('EigenTrail requires Haswell CPU instructions and kernel AVX support on every Linux CPU; /proc/cpuinfo does not confirm the required features.')

from ._eigentrail import (track_json, check_json, track_covariances,
                         track_history_covariances, track_stability_covariances,
                         analyze_stability_json, window_envelope_json,
                         temporal_gaussian_json, spd_tube_geometry_json,
                         analyze_jumps_json, check_jumps_json,
                         analyze_crossings_json, check_crossings_json,
                         analyze_transport_json, check_transport_json,
                         analyze_histories_json, check_histories_json,
                         analyze_lineage_summary_json, check_lineage_summary_json,
                         analyze_common_frame_json, check_common_frame_json)

def _unbits(value):
    if isinstance(value,dict):return {k:_unbits(v) for k,v in value.items()}
    if isinstance(value,list):return [_unbits(v) for v in value]
    if isinstance(value,str) and value.startswith('str:'):
        return value[4:]
    if isinstance(value,str) and value.startswith('f64:'):
        return struct.unpack('>d',bytes.fromhex(value[4:]))[0]
    return value

def track(covariances, times, radii, lipschitz):
    """Track under supplied nodal radii and a declared global regularity bound.

    Statistical coverage is conditional on those premises. Inputs must be
    float64 arrays; strided arrays are supported. Returns checked report data.
    """
    result = _unbits(json.loads(track_covariances(covariances,times,radii,lipschitz)))
    result['caller_owned_payload_estimate_bytes'] = covariances.nbytes + 16*len(result['eigensystems'])
    return result

def track_history(covariances, times, radii, lipschitz, query_times,
                  common_frame_weights=None, *, witness=None):
    """Track a continuous covariance path at observation and query times.

    Supply conditional nodal radii and a global Lipschitz bound. Returned
    block summaries and lineage candidates are outer information on one
    exact whole-path family; arbitrary marginal choices need not form a path.
    Checked affine witnesses retain their separate feasibility status.
    Optional witness supplies one candidate covariance per observation date,
    checked against the original radii and regularity without widening them.
    It establishes existence and leaves the outer query bounds unchanged.

    Supplying common_frame_weights selects the existing common-frame model
    and declares that one fixed eigenframe diagonalizes the entire path.
    Weights must be nonnegative with positive total. A simple anchor does not
    establish that premise; tied anchors retain unresolved labels.
    Explicit common-frame witnesses must be diagonal in the input coordinates.

    Input arrays must be float64 with shape (dates, features, features);
    strided arrays are supported. Returns a checked history report with
    query strength/share/rank bounds and separately checked path feasibility.
    """
    result = _unbits(json.loads(track_history_covariances(
        covariances, times, radii, lipschitz, query_times,
        common_frame_weights, witness=witness)))
    configuration = result['configurations'][0]
    dates = len(configuration['continuous_conditional']['eigensystems'])
    workflow = ('continuous_workflow' if common_frame_weights is None
                else 'common_frame_queries')
    queries = len(configuration[workflow][0]['queries'])
    result['caller_owned_payload_estimate_bytes'] = covariances.nbytes + 8*(
        (2 if common_frame_weights is None else 3)*dates+queries) + (
        0 if witness is None else witness.nbytes)
    return result

def track_stability(covariances, times, radii, lipschitz, query_times, tolerance, *, witness=None):
    """Classify checked projector movement against a prespecified tolerance.

    ``tolerance`` is finite and lies in [0, 1], in the operator norm distance
    between orthogonal projectors in the fixed feature coordinates. A checked
    distance enclosure wholly below or at the tolerance is stable; one wholly
    above it is changed; overlapping or undefined enclosures are unresolved.

    Returns an envelope with the complete checked ``history`` and an
    ``assessment``. Configuration, workflow, query and block order agree with
    the source history. Local and reference decisions use that history's
    existing displacement bounds, including their identity qualifications.
    Calibration, feasibility and undefined reasons remain in ``history``.

    Optional witness has the same shape and zero-widening semantics as track_history.
    This route supports the continuous history model. Self-comparisons and
    full-space projectors are geometrically trivial; ``full_space`` marks the
    latter. Projector stability does not establish covariance equality or
    unchanged temporal dynamics. No new statistical coverage is calibrated.
    """
    if tolerance is None:
        raise ValueError('stability tolerance required')
    result = _unbits(json.loads(track_stability_covariances(
        covariances, times, radii, lipschitz, query_times, tolerance, witness=witness)))
    configuration = result['history']['configurations'][0]
    dates = len(configuration['continuous_conditional']['eigensystems'])
    queries = len(configuration['continuous_workflow'][0]['queries'])
    result['caller_owned_payload_estimate_bytes'] = covariances.nbytes + 8*(
        2*dates+queries+1) + (0 if witness is None else witness.nbytes)
    return result

def window_envelope(sample_times, weights, target_time, window_radius, lipschitz,
                    target_mismatch_bound, target_description, assumptions):
    """Transfer a declared window-target error radius to an instantaneous target.

    Nonnegative raw weights define exact real normalized weights. The checked
    radius is window_radius + target_mismatch_bound + lipschitz times the
    weighted mean absolute time offset. Target and assumptions are required
    descriptions of the estimator target, centring/divisor/filtering and the
    supplied source-error event. They do not establish those premises.

    The global operator-norm regularity bound must cover the connecting time
    intervals in fixed coordinates. This deterministic transfer creates no
    statistical calibration or simultaneous event. Pooling means and serial
    dependence can change the estimator target even for constant covariance.

    Returns a checked envelope with the complete request. Its total_radius can
    be passed as a conditional TRACE radius; do not charge included mismatch
    effects again through correction. Bounded sequences/iterables are supported;
    duplicate and unordered sample times are allowed.
    """
    result = json.loads(window_envelope_json(
        sample_times, weights, target_time, window_radius, lipschitz,
        target_mismatch_bound, target_description, assumptions))
    for field in ('sample_times', 'weights', 'target_time', 'window_radius',
                  'lipschitz', 'target_mismatch_bound'):
        result['request'][field] = _unbits(result['request'][field])
    for field in ('target_description', 'assumptions'):
        result['request'][field] = result['request'][field].removeprefix('str:')
    for field in ('mean_absolute_offset', 'drift_bound', 'total_radius'):
        result['envelope'][field] = _unbits(result['envelope'][field])
    return result


def temporal_gaussian(dimension, observations, windows, covariance_bound,
                      correlation_bound, alpha, assumptions):
    """Bound uniform window second moments under explicit temporal premises.

    After subtracting each observation's known true mean, the complete window
    must be jointly Gaussian. The caller supplies the prespecified envelope
    ||Cov(X_i, X_k)||op <= covariance_bound * correlation_bound**abs(i-k).
    The exponent uses observation-index lag, not elapsed physical time.
    This routine does not infer or verify the law or envelope from data.

    The estimator is the uniform second moment with divisor observations; its
    target is the arithmetic average of within-time covariances. Sample mean
    subtraction, fitted bounds and adaptive windows require another argument.
    Use the same total windows and alpha for every member of a prespecified
    family to obtain the stated simultaneous 1-alpha source event. Overlapping
    windows are allowed; separate windows=1 calls do not create that event.

    Counts are positive integers no greater than 2**32-1. The covariance bound
    is finite and nonnegative, correlation_bound is in [0,1], and alpha is in
    (0,1). Nonempty assumptions text (at most 4096 UTF-8 bytes) is mandatory.
    Returns a checked envelope retaining the request and conditional scope.
    Its radius can be transferred to an instantaneous covariance using
    window_envelope with the actual times, uniform weights and a justified
    Lipschitz bound; account separately for estimator numerical error.
    """
    result = json.loads(temporal_gaussian_json(
        dimension, observations, windows, covariance_bound, correlation_bound,
        alpha, assumptions))
    for field in ('covariance_bound', 'correlation_bound', 'alpha'):
        result['request'][field] = _unbits(result['request'][field])
    result['request']['assumptions'] = result['request']['assumptions'].removeprefix('str:')
    for field in ('log_tail', 'frobenius_bound', 'operator_bound', 'radius'):
        result['envelope'][field] = _unbits(result['envelope'][field])
    return result


def spd_tube_geometry(dimension, centre_floor, radius):
    """Convert a conditional covariance error tube to SPD and inverse bounds.

    The caller supplies a positive lower bound centre_floor on the smallest
    eigenvalue of the SPD centre and a nonnegative operator-norm error radius.
    This scalar routine does not receive or verify that centre. In fixed
    coordinates, radius < centre_floor implies an SPD target, with checked
    affine-invariant distance and precision-matrix operator error bounds.
    The affine-invariant metric is tr(A^-1 U A^-1 V); the fixed-mean Gaussian
    Fisher distance is this distance divided by sqrt(2).

    If radius >= centre_floor, resolved is False and both bounds are None.
    This does not prove a target singular. A zero radius returns exact zeros.
    Dimension is a positive integer no greater than 2**32-1; both scalars must
    be finite. No statistical event, matrix repair, branch identity or whole
    curve witness is created. Returns the full checked request and envelope.
    """
    result = json.loads(spd_tube_geometry_json(dimension, centre_floor, radius))
    for field in ('centre_floor', 'radius'):
        result['request'][field] = _unbits(result['request'][field])
    undefined = {'kind': 'Undefined', 'value': 'SPD margin unresolved'}
    for field in ('affine_invariant_radius', 'precision_radius'):
        value = result['envelope'][field]
        result['envelope'][field] = None if value == undefined else _unbits(value)
    return result

__all__ = ['track','track_history','track_stability','track_stability_covariances',
           'analyze_stability_json','window_envelope','window_envelope_json',
           'temporal_gaussian','temporal_gaussian_json','spd_tube_geometry','spd_tube_geometry_json','track_json','check_json','track_covariances',
           'track_history_covariances',
           'analyze_jumps_json','check_jumps_json',
           'analyze_crossings_json','check_crossings_json',
           'analyze_transport_json','check_transport_json',
           'analyze_histories_json','check_histories_json',
           'analyze_lineage_summary_json','check_lineage_summary_json',
           'analyze_common_frame_json','check_common_frame_json']
