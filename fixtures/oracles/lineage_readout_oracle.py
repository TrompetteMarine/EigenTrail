"""Independent exact-arithmetic readout fixtures; imports no Rust implementation."""
from fractions import Fraction as F
from hashlib import sha256
from math import isqrt
from pathlib import Path
import json
import math
import struct


OUT = Path(__file__).resolve().parent
RADIUS = F(1, 2**40)


def bounds_trig(x, sine):
    assert 0 <= x <= F(1, 4)
    term = x if sine else F(1)
    total = term
    for k in range(1, 25):
        term *= -x*x / ((2*k)*(2*k+1) if sine else (2*k-1)*(2*k))
        total += term
    k = 25
    following = term * -x*x / ((2*k)*(2*k+1) if sine else (2*k-1)*(2*k))
    # Alternating decreasing terms enclose the tail between consecutive sums.
    return min(total, total + following), max(total, total + following)


def sqrt_up(x):
    assert x >= 0
    scale = 2**160
    n = -((-x.numerator * scale**2) // x.denominator)
    root = isqrt(n)
    if root*root < n:
        root += 1
    return F(root, scale)


def canonical(value):
    if isinstance(value, float):
        assert math.isfinite(value)
        return 'f64:' + struct.pack('>d', value).hex()
    if isinstance(value, str) and value.startswith(('f64:', 'str:')):
        return 'str:' + value
    if isinstance(value, list):
        return [canonical(item) for item in value]
    if isinstance(value, dict):
        return {key: canonical(item) for key, item in value.items()}
    return value


def matrix(rows):
    p = len(rows)
    return {'n': p, 'data': [float(rows[i][j]) for j in range(p) for i in range(p)]}


def diagonal(values):
    return matrix([[x if i == j else F(0) for j, x in enumerate(values)] for i in range(len(values))])


def history(nodes, times, k, queries, classes=None, radius=RADIUS, witnesses=None):
    p = nodes[0]['n']
    observations = {'schema': 'eigentrail.trace.v1', 'times': list(map(float, times)),
                    'covariances': nodes, 'features': [f'feature_{i}' for i in range(p)],
                    'lipschitz': float(k),
                    'calibration': {'kind': 'Conditional', 'radii': [float(radius)]*len(nodes),
                                    'assumptions': 'frozen deterministic nodal envelope and declared analytic global regularity'},
                    'correction': [0.0]*len(nodes), 'target': 0.002,
                    'numerical_allowance': 1e-5, 'memory_bytes': 512*1024**2}
    classes = classes or [('continuous', {'kind': 'Continuous'})]
    return {'schema': 'eigentrail.histories.v2', 'observations': observations,
            'configurations': [{'id': name, 'lipschitz': float(k), 'class': model,
                                'witnesses': witnesses or []} for name, model in classes],
            'query_times': list(map(float, queries))}


def frame(weights):
    return {'kind': 'CommonFrame', 'model': {'weights': list(map(float, weights)),
                                           'common_frame_assumed': True}}


def main():
    line = diagonal([F(4), F(1), F(1, 4)])
    plane = diagonal([F(4), F(4), F(1), F(1, 2), F(1, 4)])
    q3 = [F(0), F(1, 2), F(1)]
    cases = []

    def add(name, request, expected):
        cases.append({'name': name, 'history': canonical(request), 'expected': expected})

    add('constant_line3', history([line, line], [0, 1], 0, q3),
        {'rows': 9, 'leading_dimension': 1, 'leading_strength': '4', 'leading_share': '16/21',
         'reference_truth': '0', 'leading_motion': 'WithinTolerance'})
    add('constant_plane5', history([plane, plane], [0, 1], 0, q3),
        {'rows': 12, 'leading_dimension': 2, 'leading_strength': '8', 'leading_share': '32/39',
         'reference_truth': '0', 'leading_motion': 'WithinTolerance', 'internal_axes_identified': False})
    scalar = diagonal([F(4)])
    add('scalar_ambient1', history([scalar, scalar], [0, 1], 0, q3),
        {'rows': 3, 'information': 'WholeSpace', 'motion': 'Unresolved',
         'accuracy_target_attained': False})
    zero = diagonal([F(0), F(0), F(0)])
    add('zero_trace3', history([zero, zero], [0, 1], 0, q3, radius=F(0)),
        {'rows': 3, 'information': 'WholeSpace', 'motion': 'Unresolved',
         'share': 'Undefined', 'accuracy_target_attained': False})

    nodes, maximum_round = [], F(0)
    for j in range(129):
        x = F(j, 512)  # twice the rotation angle t/8, t=j/128.
        slo, shi = bounds_trig(x, True); clo, chi = bounds_trig(x, False)
        exact = [(F(5, 2)+F(3, 2)*clo, F(5, 2)+F(3, 2)*chi),
                 (F(3, 2)*slo, F(3, 2)*shi),
                 (F(5, 2)-F(3, 2)*chi, F(5, 2)-F(3, 2)*clo)]
        represented = [float((lo+hi)/2) for lo, hi in exact]
        errors = [max(abs(F(value)-lo), abs(F(value)-hi))
                  for value, (lo, hi) in zip(represented, exact)]
        norm_error = sqrt_up(errors[0]**2 + 2*errors[1]**2 + errors[2]**2)
        maximum_round = max(maximum_round, norm_error)
        a, b, d = represented
        nodes.append(matrix([[a, b, 0], [b, d, 0], [0, 0, F(1, 4)]]))
    assert maximum_round < RADIUS
    sin_lo, sin_hi = bounds_trig(F(1, 8), True)
    assert sin_lo > F(12, 100)
    eta = RADIUS + F(3, 8)*F(1, 128)/2
    gap = F(3) - 4*RADIUS - F(3, 8)*F(1, 128)
    ideal = 2*eta/gap
    composed = ideal + F(1, 100000)
    assert gap > 2*eta and composed < F(2, 1000)
    add('rotating_line3', history(nodes, [F(j, 128) for j in range(129)], F(3, 8), q3),
        {'rows': 9, 'leading_dimension': 1, 'endpoint_displacement_lower': str(sin_lo),
         'endpoint_displacement_upper': str(sin_hi), 'endpoint_motion': 'BeyondTolerance',
         'minimum_required_report_lower': '1/20'})

    hidden = diagonal([F(float(.6)), F(float(.3)), F(float(.1))])
    add('hidden_shared_observations3', history([hidden]*10, range(10), 2,
                                             [F(j, 4) for j in range(37)], radius=F(0)),
        {'rows': 37, 'leading_dimension': 3, 'all_information': 'WholeSpace',
         'all_motion': 'Unresolved', 'directional_accuracy': False,
         'oracle_labels_supplied_to_estimator': False})

    for name, eps in [('exact_crossing3', F(0)), ('avoided_crossing3', F(1, 50))]:
        pair = [matrix([[F(9, 20)+x, eps, 0], [eps, F(9, 20)-x, 0], [0, 0, F(1, 10)]])
                for x in [F(1, 40), -F(1, 40)]]
        add(name, history(pair, [4, 5], F(1, 20), [4, F(9, 2), 5]),
            {'rows': 6, 'leading_dimension': 2, 'midpoint_accuracy': False,
             'individual_internal_identity': 'unresolved', 'exact_collision_not_inferred': True})

    exchange = diagonal([F(1), F(4), F(1, 4)])
    add('common_frame_exchange3', history([line, exchange], [0, 1], 3, q3,
                                         [('frame', frame([1, 0]))]),
        {'rows': 9, 'target_kind': 'AnchorAxis', 'first_axis_endpoint_ranks': [[1, 1], [2, 2]],
         'reference_truth': '0', 'motion': 'WithinTolerance'})
    add('common_frame_unresolved_anchor5', history([plane, plane], [0, 1], 0, q3,
                                                  [('frame', frame([1, 1]))]),
        {'rows': 3, 'state': 'UnresolvedAnchor'})
    imprecise = diagonal([F(2), F(1)])
    add('common_frame_imprecise_axis2', history([imprecise, imprecise], [0, 1], 0, q3,
                                               [('frame', frame([1, 1]))], radius=F(1, 4)),
        {'rows': 6, 'target_kind': 'AnchorAxis', 'information': 'Line', 'motion': 'WithinTolerance',
         'accuracy_target_attained': False})

    different = diagonal([F(3), F(1), F(1, 4)])
    add('inconsistent_outer3', history([line, different], [0, 1], 0, q3),
        {'rows': 3, 'state': 'InconsistentOuterBounds', 'feasibility': 'Unresolved'})
    mid = diagonal([F(7, 2), F(1), F(1, 4)])
    add('widened_witness3', history([line, different], [0, 1], 0, q3,
                                   witnesses=[{'nodes': [mid, mid], 'widening': .5}]),
        {'rows': 3, 'state': 'InconsistentOuterBounds', 'feasibility': 'WidenedAffineWitness'})
    add('two_configs_duplicate_queries3', history([line, line], [0, 1], 0, [1, 0, 1],
                                                 [('continuous', {'kind': 'Continuous'}),
                                                  ('frame', frame([1, 1]))]),
        {'rows': 18, 'configurations': 2, 'query_times_in_request_order': [1, 0, 1]})

    result = {'schema': 'eigentrail.lineage-readout-independent-oracles.v1',
              'delta': '1/20', 'radius': str(RADIUS), 'projector_target': '1/500',
              'numerical_allowance': '1/100000', 'rotation_nodes': 129,
              'rotation_operator_rounding_upper': str(maximum_round),
              'rotation_ideal_projector_bound': str(ideal),
              'rotation_composed_budget_upper': str(composed),
              'rotation_endpoint_displacement': {'lower': str(sin_lo), 'upper': str(sin_hi)},
              'budget_scope': 'independent rational design budget, not executed native acceptance',
              'displacement_scope': 'existing report may use weaker Frobenius/sqrt(p) lower bound; require lower>delta, not attainment of operator triangle budget',
              'avoided_overlap': {'same_spectral_branch': '16/41', 'other_active_branch': '25/41'},
              'trig_derivation': '25-term alternating sin/cos Taylor sums and next-term remainder on[0,1/4]; rational Frobenius bound dominates operator rounding',
              'cases': cases}
    path = OUT/'lineage_readout_oracles.json'
    path.write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps({'cases': len(cases), 'sha256': sha256(path.read_bytes()).hexdigest(),
                      'rotation_operator_rounding_upper_float': float(maximum_round),
                      'rotation_composed_budget_upper_float': float(composed)}))


if __name__ == '__main__':
    main()
