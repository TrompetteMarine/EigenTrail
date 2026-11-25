"""Array-to-wire adapters for observed-date queries; all numerical inference is native."""
from collections.abc import Mapping
from itertools import islice
import json
import math
import struct

_LENSES = ('covariance', 'precision', 'correlation', 'partial')
_NUMBERS = frozenset(('time', 'tolerance_deg', 'rtol', 'rho', 'backward', 'polar',
    'root_numerical_rho', 'matrix_error', 'numerical_rho', 'operator_error',
    'angular_radius_deg', 'cost', 'degrees', 'total_rho', 'mapped_rho', 'entry_error',
    'data', 'values', 'range', 'intervals', 'centre_correlation',
    'population_correlation', 'cutoff'))


def _wire(value):
    if isinstance(value, float):
        if not math.isfinite(value):
            raise ValueError('finite binary64 values required')
        return 'f64:' + struct.pack('>d', value).hex()
    if isinstance(value, str):
        return 'str:' + value if value.startswith(('f64:', 'str:')) else value
    if isinstance(value, dict):
        return {key: _wire(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [_wire(item) for item in value]
    return value


def _decoded(value, numeric=False):
    if isinstance(value, dict):
        return {key: _decoded(item, key in _NUMBERS) for key, item in value.items()}
    if isinstance(value, list):
        return [_decoded(item, numeric) for item in value]
    if isinstance(value, str):
        if value.startswith('str:'):
            return value[4:]
        if numeric and value.startswith('f64:'):
            return struct.unpack('>d', bytes.fromhex(value[4:]))[0]
    return value


class RollingResult(Mapping):
    """Decoded checked records and their original canonical request/report bytes."""
    def __init__(self, request_json, report_json):
        from ._eigentrail import check_rolling_lineage_json
        if check_rolling_lineage_json(request_json, report_json) is not True:
            raise ValueError('rolling report failed independent checking')
        self.request_json = request_json
        self.report_json = report_json
        self.request = _decoded(json.loads(request_json))
        self.report = _decoded(json.loads(report_json))

    def __getitem__(self, key):
        return self.report[key]

    def __iter__(self):
        return iter(self.report)

    def __len__(self):
        return len(self.report)

    def records(self, kind):
        """Iterate one documented record family without discarding the canonical report."""
        for chunk in self.report['chunks']:
            if chunk['records']['kind'] == kind:
                yield from chunk['records']['records']


def _index(value, label, *, minimum=0):
    if isinstance(value, bool) or not isinstance(value, int) or value < minimum:
        raise ValueError(label + ' must be an integer >= ' + str(minimum))
    return value


def _sequence(value, length, label):
    if isinstance(value, (str, bytes)):
        raise ValueError(label + ' must be a sequence, not a string')
    output = list(islice(iter(value), length + 1))
    if len(output) != length:
        raise ValueError(label + ' length differs from the declared axes')
    return output


def _request(covariances, dates, *, tolerance, mode, lag_cap, founders,
             features, units, frame, lenses, lens_pairs, rank_masks, rho,
             joint_event, radius_provider, grouping_rtol, projection,
             max_candidates, max_edge_records, max_bytes, chunk_records,
             subject_ids):
    import numpy as np
    arrays = list(covariances)
    if not arrays:
        raise ValueError('at least one subject is required')
    if any(not isinstance(a, np.ndarray) or a.dtype != np.dtype('float64') or
           a.ndim != 3 or a.shape[1] != a.shape[2] for a in arrays):
        raise ValueError('real float64 covariance arrays of shape (dates,features,features) required')
    t, n, _ = arrays[0].shape
    if not 1 <= t <= 10000 or not 1 <= n <= 256 or len(arrays) > 1024 or any(a.shape != arrays[0].shape for a in arrays):
        raise ValueError('same axes required: 1..10000 dates, 1..256 features, 1..1024 subjects')
    if isinstance(tolerance, (bool, np.bool_)):
        raise ValueError('tolerance must be a real angle, not a boolean')
    if sum(a.size for a in arrays) * 24 + t * len(arrays) * 256 > max_bytes:
        raise ValueError('rolling JSON input estimate exceeds max_bytes before copying')
    times = _sequence(dates, t, 'dates')
    feature_ids = _sequence(features, n, 'features')
    feature_units = _sequence(units, n, 'units')
    ids = _sequence(subject_ids, len(arrays), 'subject_ids')
    if any(not isinstance(x, str) for x in feature_ids + feature_units + ids):
        raise ValueError('feature IDs, units and subject IDs must be strings')
    dated = []
    for i, value in enumerate(times):
        if isinstance(value, dict) and set(value) == {'id', 'time'}:
            dated.append({'id': value['id'], 'time': float(value['time'])})
        else:
            dated.append({'id': str(i), 'time': float(value)})
    if mode == 'point':
        if any(x is not None for x in (rho, joint_event, radius_provider)):
            raise ValueError('point mode has no conditional region fields')
        uncertainty = {'mode': 'point'}
    elif mode == 'conditional':
        if rho is None or not isinstance(joint_event, str) or not isinstance(radius_provider, str):
            raise ValueError('conditional mode requires rho, joint_event and radius_provider')
        uncertainty = {'mode': mode, 'rho': float(rho), 'joint_event': joint_event,
                       'provider': radius_provider}
    else:
        raise ValueError('mode must be point or conditional')
    selected = list(lenses)
    if not selected or selected != [lens for lens in _LENSES if lens in selected]:
        raise ValueError('unique lenses in covariance,precision,correlation,partial order required')
    travel = [[a, b] for a in selected for b in selected] if lens_pairs is None else [list(x) for x in lens_pairs]
    masks = ({lens: list(range(1, n + 1)) for lens in selected} if rank_masks is None else rank_masks)
    if not isinstance(masks, Mapping) or set(masks) - set(selected):
        raise ValueError('rank_masks keys must be selected lenses')
    mask_rows = []
    for lens in selected:
        ranks = list(masks.get(lens, []))
        if ranks != sorted(set(ranks)):
            raise ValueError('rank_masks must contain unique ascending human ranks')
        mask_rows.append({'lens': lens, 'ranks': [[_index(r, 'rank', minimum=1)-1, r] for r in ranks]})
    queries = []
    for founder in founders:
        if not isinstance(founder, dict) or not {'date', 'lens', 'ranks'} <= set(founder) or set(founder) - {'subject', 'date', 'lens', 'ranks', 'policies'}:
            raise ValueError('founder requires date index, lens and human ranks; optional subject and policies')
        ranks = list(founder['ranks'])
        if not ranks or any(isinstance(r, bool) or not isinstance(r, int) for r in ranks) or ranks != list(range(_index(ranks[0], 'founder rank', minimum=1), ranks[-1] + 1)):
            raise ValueError('founder ranks must be one nonempty consecutive human-rank interval')
        queries.append({'subject': _index(founder.get('subject', 0), 'subject'),
            'date': _index(founder['date'], 'date index'), 'lens': founder['lens'],
            'rank': [ranks[0]-1, ranks[-1]],
            'policies': list(founder.get('policies', ('resolved_line', 'typed_block_graph', 'outer_relaxed')))})
    if isinstance(projection, str):
        if projection not in ('counts', 'full'):
            raise ValueError('projection must be counts, full or a selected_edges wire object')
        projection = {'kind': projection}
    subjects = []
    for subject, values in zip(ids, arrays):
        slots = [{'tag': 'observed', 'covariance': {'n': n,
                  'data': [float(x) for x in values[i].ravel(order='F')]}} for i in range(t)]
        subjects.append({'id': subject, 'slots': slots})
    return {'schema': 'eigentrail.rolling-lineage.v1',
        'chart': {'frame': frame, 'coordinates': 'frame_coefficients'},
        'features': [{'id': a, 'unit': b} for a, b in zip(feature_ids, feature_units)],
        'dates': dated, 'subjects': subjects, 'lenses': selected,
        'representation': {'symmetry': 'exact', 'correction': {'kind': 'none'}},
        'grouping': {'rtol': float(grouping_rtol)}, 'tolerance_deg': float(tolerance),
        'uncertainty': uncertainty,
        'scope': {'lens_travel': travel, 'rank_masks': mask_rows,
                  'max_lag': _index(lag_cap, 'lag_cap'), 'missing': 'skip_recorded',
                  'policy': 'draft_f_conservative', 'certificate': 'observed_dated_subspaces',
                  'direct_scope': 'all_later_slots'}, 'founders': queries,
        'projection': projection,
        'limits': {'max_candidates': _index(max_candidates, 'max_candidates'),
                   'max_edge_records': _index(max_edge_records, 'max_edge_records'),
                   'max_bytes': _index(max_bytes, 'max_bytes'),
                   'chunk_records': _index(chunk_records, 'chunk_records', minimum=1)},
        'precision': {'format': 'binary64', 'angle_unit': 'degrees'}}


def track_rolling(covariances, dates, *, tolerance, mode, lag_cap, founders,
                  features, units, frame, lenses=_LENSES, lens_pairs=None,
                  rank_masks=None, rho=None, joint_event=None, radius_provider=None,
                  grouping_rtol=1e-12, projection='full', max_candidates=1_000_000,
                  max_edge_records=1_000_000, max_bytes=512*1024*1024, chunk_records=256,
                  subject_id='subject'):
    """Check observed-date covariance lineage in declared feature coordinates.

    Input is a real float64 (T,N,N) array; strides are supported. Dates are
    finite times or {id,time} records. Founder dates use zero-based slot indices;
    their ranks use human ranks 1..N (a consecutive list denotes a block).
    Conditional mode requires a joint event, radius provider and Thompson rho.
    Direct founder comparisons use all later slots; graph travel uses lag_cap.
    No missing date is inferred. Use the raw request route for explicit missing slots.
    """
    from ._eigentrail import analyze_rolling_lineage_json
    request = _request([covariances], dates, tolerance=tolerance, mode=mode,
        lag_cap=lag_cap, founders=founders, features=features, units=units, frame=frame,
        lenses=lenses, lens_pairs=lens_pairs, rank_masks=rank_masks, rho=rho,
        joint_event=joint_event, radius_provider=radius_provider, grouping_rtol=grouping_rtol,
        projection=projection, max_candidates=max_candidates, max_edge_records=max_edge_records,
        max_bytes=max_bytes, chunk_records=chunk_records, subject_ids=[subject_id])
    text = json.dumps(_wire(request), sort_keys=True, separators=(',', ':'), ensure_ascii=False, allow_nan=False)
    return RollingResult(text, analyze_rolling_lineage_json(text))


def track_rolling_panel(covariances, dates, *, axes, subject_ids,
                        tolerance, mode, lag_cap, founders, features, units, frame,
                        lenses=_LENSES, lens_pairs=None, rank_masks=None, rho=None,
                        joint_event=None, radius_provider=None, grouping_rtol=1e-12,
                        projection='full', max_candidates=1_000_000,
                        max_edge_records=1_000_000, max_bytes=512*1024*1024,
                        chunk_records=256):
    """Explicit (subject,feature,feature,date) adapter; subjects remain separate."""
    import numpy as np
    from ._eigentrail import analyze_rolling_lineage_json
    if tuple(axes) != ('subject', 'feature', 'feature', 'date'):
        raise ValueError('declare axes=(subject,feature,feature,date) explicitly')
    if not isinstance(covariances, np.ndarray) or covariances.dtype != np.dtype('float64') or covariances.ndim != 4:
        raise ValueError('real float64 (K,N,N,T) panel required')
    # This is a representation transpose, never a covariance transformation or pooling.
    arrays = covariances.transpose(0, 3, 1, 2)
    request = _request(arrays, dates, tolerance=tolerance, mode=mode,
        lag_cap=lag_cap, founders=founders, features=features, units=units, frame=frame,
        lenses=lenses, lens_pairs=lens_pairs, rank_masks=rank_masks, rho=rho,
        joint_event=joint_event, radius_provider=radius_provider, grouping_rtol=grouping_rtol,
        projection=projection, max_candidates=max_candidates, max_edge_records=max_edge_records,
        max_bytes=max_bytes, chunk_records=chunk_records, subject_ids=subject_ids)
    text = json.dumps(_wire(request), sort_keys=True, separators=(',', ':'), ensure_ascii=False, allow_nan=False)
    return RollingResult(text, analyze_rolling_lineage_json(text))
