"""Small loading changes can accumulate into a large reference displacement."""
import numpy as np
from eigentrail import track_history

rotation = np.array([[399., -40.], [40., 399.]]) / 401.
direction = np.array([1., 0.])
covariances = []
for _ in range(17):
    covariances.append(np.eye(2) + np.outer(direction, direction))
    direction = rotation @ direction

# Supplied radii and regularity are premises, rather than estimates from slopes.
report = track_history(
    np.array(covariances), np.arange(17, dtype=np.float64),
    np.full(17, 1e-10), 0.125, list(map(float, range(17))),
)
configuration = report['configurations'][0]
queries = configuration['continuous_workflow'][0]['queries']
last = queries[-1]['state']['blocks'][0]
local = [q['state']['blocks'][0]['local_displacement']['value'] for q in queries[1:]]
reference = last['reference_displacement']['value']
assert max(r['upper'] for r in local) < 0.2
assert reference['lower'] > 0.99
print('Checked continuous history:', configuration['feasibility']['kind'])
print('Largest local displacement upper bound:', max(r['upper'] for r in local))
print('Final displacement from initial reference:', reference['lower'], reference['upper'])
print('These are conditional outer bounds on one history family; arbitrary selections are not jointly feasible.')
