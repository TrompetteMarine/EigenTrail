"""Small synthetic rolling example. Run from an installed EigenTrail candidate."""
import argparse
from pathlib import Path

import numpy as np
from eigentrail import track_rolling


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path, help='new output directory')
    parser.add_argument('--plot', action='store_true', help='requires eigentrail[plot]')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    covariances = np.array([[[4., 0.], [0., 1.]], [[1., 0.], [0., 1.]],
                            [[1., 0.], [0., 4.]]], dtype=np.float64)
    result = track_rolling(covariances, [0., 1., 3.], tolerance=20., mode='point',
        lag_cap=2, founders=[{'date': 0, 'lens': 'covariance', 'ranks': [1]}],
        features=['height', 'width'], units=['synthetic-unit', 'synthetic-unit'],
        frame='fixed-synthetic-coordinates', projection='full')
    (args.output / 'request.json').write_text(result.request_json, encoding='utf-8')
    (args.output / 'report.json').write_text(result.report_json, encoding='utf-8')
    print(result['completion']['graph_counts'])
    if args.plot:
        from eigentrail import plot_window, plot_founder, plot_spectrum, export_html
        for name, (figure, _) in [('window', plot_window(result, 0, 1)),
                                   ('founder', plot_founder(result)),
                                   ('spectrum', plot_spectrum(result))]:
            figure.savefig(args.output / (name + '.svg'))
            figure.savefig(args.output / (name + '.pdf'))
            figure.savefig(args.output / (name + '.png'), dpi=300)
        export_html(result, args.output / 'report.html')


if __name__ == '__main__':
    main()
