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
                         track_history_covariances,
                         analyze_jumps_json, check_jumps_json,
                         analyze_crossings_json, check_crossings_json,
                         analyze_transport_json, check_transport_json,
                         analyze_histories_json, check_histories_json,
                         analyze_lineage_summary_json, check_lineage_summary_json,
                         analyze_common_frame_json, check_common_frame_json)

def _unbits(value):
    if isinstance(value,dict):return {k:_unbits(v) for k,v in value.items()}
    if isinstance(value,list):return [_unbits(v) for v in value]
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

__all__ = ['track','track_history','track_json','check_json','track_covariances',
           'track_history_covariances',
           'analyze_jumps_json','check_jumps_json',
           'analyze_crossings_json','check_crossings_json',
           'analyze_transport_json','check_transport_json',
           'analyze_histories_json','check_histories_json',
           'analyze_lineage_summary_json','check_lineage_summary_json',
           'analyze_common_frame_json','check_common_frame_json']
