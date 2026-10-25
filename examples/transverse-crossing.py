"""Conditional reconnection through an explicitly assumed exact collision."""
import json
import struct
from eigentrail import analyze_crossings_json, check_crossings_json


def wire(value):
    if isinstance(value, float):
        return 'f64:' + struct.pack('>d', value).hex()
    if isinstance(value, list):
        return [wire(v) for v in value]
    if isinstance(value, dict):
        return {k: wire(v) for k, v in value.items()}
    return value


times = [-1. / 64., 0., 1. / 64.]
request = dict(
    schema='eigentrail.trace.v1', times=times,
    covariances=[dict(n=2, data=[2. - t, 0., 0., 2. + t]) for t in times],
    features=['x', 'y'], lipschitz=1., correction=[0.] * 3,
    calibration=dict(kind='Conditional', radii=[2. ** -40] * 3,
                     assumptions='declared uniform covariance error and C2 model'),
    target=.002, numerical_allowance=1e-5, memory_bytes=512 * 1024 * 1024,
)
window = dict(block=[0, 2], centre=1, secant=[0, 2], attachments=[0, 2],
              bracket=[-1. / 4096., 1. / 4096.], curvature=0.,
              exact_collision_assumed=True)
request_json, window_json = [json.dumps(wire(v), sort_keys=True, separators=(',', ':'))
                             for v in (request, window)]
report_json = analyze_crossings_json(request_json, window_json)
assert check_crossings_json(request_json, window_json, report_json)
report = json.loads(report_json)
assert report['outcome']['kind'] == 'Resolved'
proof = report['proof'][0]
assert proof['left'] == [0, 1] and proof['right'] == [1, 0]
print('Derivative-labelled branches reconnect across PC rank reversal:',
      proof['left'], '->', proof['right'])
print('Both complete attachment graphs and timing evidence checked independently.')
print('The exact collision and C2 bounds remain declared premises.')
