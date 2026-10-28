"""Local TRACE analysis. Covariances have shape (dates, features, features)."""
import json
import struct
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
                  common_frame_weights=None):
    """Track a continuous covariance path at observation and query times.

    Supply conditional nodal radii and a global Lipschitz bound. Returned
    block summaries and lineage candidates are outer information on one
    exact whole-path family; arbitrary marginal choices need not form a path.
    Checked affine witnesses retain their separate feasibility status.

    Supplying common_frame_weights selects the existing common-frame model
    and declares that one fixed eigenframe diagonalizes the entire path.
    Weights must be nonnegative with positive total. A simple anchor does not
    establish that premise; tied anchors retain unresolved labels.

    Input arrays must be float64 with shape (dates, features, features);
    strided arrays are supported. Returns a checked history report with
    query strength/share/rank bounds and separately checked path feasibility.
    """
    result = _unbits(json.loads(track_history_covariances(
        covariances, times, radii, lipschitz, query_times,
        common_frame_weights)))
    configuration = result['configurations'][0]
    dates = len(configuration['continuous_conditional']['eigensystems'])
    workflow = ('continuous_workflow' if common_frame_weights is None
                else 'common_frame_queries')
    queries = len(configuration[workflow][0]['queries'])
    result['caller_owned_payload_estimate_bytes'] = covariances.nbytes + 8*(
        (2 if common_frame_weights is None else 3)*dates+queries)
    return result

__all__ = ['track','track_history','track_json','check_json','track_covariances',
           'track_history_covariances',
           'analyze_jumps_json','check_jumps_json',
           'analyze_crossings_json','check_crossings_json',
           'analyze_transport_json','check_transport_json',
           'analyze_histories_json','check_histories_json',
           'analyze_lineage_summary_json','check_lineage_summary_json',
           'analyze_common_frame_json','check_common_frame_json']
