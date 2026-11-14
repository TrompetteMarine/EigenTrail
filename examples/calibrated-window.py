"""Conditional API composition with exact synthetic centres; no data-law validation."""
import eigentrail
import numpy as np

source = eigentrail.temporal_gaussian(
    2, 200000, 2, 1., 0.6, 0.05,
    'Illustrated premises: known zero means, joint Gaussian residuals, prescribed '
    'V=1 and rho=0.6; two prespecified windows share alpha=0.05. No data generated.')
window = eigentrail.window_envelope(
    [-0.005, 0.005], [1., 1.], 0., source['envelope']['radius'], 0., 0.,
    'Uniform second moment in two equally sized time groups; divisor n.',
    'Illustrated constant-covariance target; no mismatch or arithmetic error '
    'for the exact supplied synthetic centres. Statistical premises remain external.')
radius = window['envelope']['total_radius']
centres = np.repeat(np.diag([0.75, 0.25])[None, :, :], 2, axis=0)
history = eigentrail.track_history(centres, [0., 1.], [radius]*2, 0., [0., 0.5, 1.])
stability = eigentrail.track_stability(
    centres, [0., 1.], [radius]*2, 0., [0., 0.5, 1.], 0.3)
geometry = eigentrail.spd_tube_geometry(2, 0.25, radius)
assert stability['history'] == {
    key: value for key, value in history.items()
    if key != 'caller_owned_payload_estimate_bytes'}
print('Conditional instantaneous radius:', radius)
print('Stability assessment:', stability['assessment'])
print('Conditional SPD geometry:', geometry['envelope'])
