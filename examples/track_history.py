"""Track persistent fixed-frame modes and query between observations.

Run with an installed EigenTrail: python examples/track_history.py
The fixed-frame and error/regularity premises here describe a synthetic example.
"""
import numpy as np
import eigentrail


def main():
    times = np.array([0., .5, 1.])
    covariances = np.array([np.diag([80.+t, 20.-t]) for t in times])
    # Supplying common-frame weights declares persistent fixed eigenaxes.
    report = eigentrail.track_history(
        covariances, times, radii=np.zeros(3), lipschitz=1.,
        query_times=[0., .25, .5, .75, 1.], common_frame_weights=[1., 0., 0.],
    )
    configuration = report['configurations'][0]
    workflow = configuration['common_frame_queries'][0]
    print('Persistent first-axis variance and share, including between-date queries')
    print(' time     variance interval       share interval     possible rank')
    for row in workflow['queries']:
        state = row['state']
        if state['kind'] != 'Bounds':
            print(f" {row['time']:.2f}     {state['kind']}")
            continue
        summary = state['summaries'][0]
        strength = summary['strength']
        share = summary['share']
        text = 'undefined' if share['kind'] == 'Undefined' else (
            f"[{share['value']['lower']:.2%}, {share['value']['upper']:.2%}]")
        print(f" {row['time']:.2f}     [{strength['lower']:.4f}, {strength['upper']:.4f}]"
              f"    {text:>18}     {summary['rank']}")
    print('Checked common-frame path witness:', configuration['feasibility']['kind'])
    print('Persistent labels refer to the declared fixed axes; ranks may change on other paths.')
    print('Coverage is conditional on the supplied error, fixed-frame and regularity premises.')


if __name__ == '__main__':
    main()
