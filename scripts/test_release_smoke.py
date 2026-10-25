"""Independent guards for the release smoke's fixture and mathematical assertions."""
from copy import deepcopy
import importlib.util
from pathlib import Path
from fractions import Fraction
import math
import unittest

spec = importlib.util.spec_from_file_location('release_smoke', Path(__file__).with_name('release_smoke.py'))
smoke = importlib.util.module_from_spec(spec); spec.loader.exec_module(smoke)


def example():
    interval = lambda value: {'lower': value - 2.**-20, 'upper': value + 2.**-20}
    summaries = [{'strength': interval(.75), 'rank': [1, 1], 'share': {'kind': 'Interval', 'value': interval(.75)}},
                 {'strength': interval(.25), 'rank': [2, 2], 'share': {'kind': 'Interval', 'value': interval(.25)}}]
    return {'schema': 'eigentrail.trace.v1', 'completed': True,
        'eigensystems': [{'values': [.75, .25]}, {'values': [.75, .25]}],
        'segments': [{'association': {'kind': 'Unique', 'matching': [0, 1]},
            'tubes': [{'start': 0, 'end': 1, 'bound': .000004, 'informative': True},
                      {'start': 1, 'end': 2, 'bound': .000004, 'informative': True}]}],
        'summaries': [deepcopy(summaries), deepcopy(summaries)]}


class SmokeGuard(unittest.TestCase):
    def test_wire_preserves_dyadic_fixture_and_column_order(self):
        request = smoke.decoded(smoke.fixture())
        self.assertEqual(request['times'], [0., 1.])
        self.assertEqual(request['covariances'][0]['data'], [.75, 0., 0., .25])
        self.assertEqual(request['calibration']['radii'], [2.**-20] * 2)
        self.assertEqual(smoke.decoded(smoke.wire([.75, .25, -0.])), [.75, .25, -0.])

    def test_independent_valid_constant_intervals(self):
        report = example(); radius = Fraction(1, 2**20)
        bound = math.nextafter(float(2 * radius / (Fraction(1, 2) - 2 * radius)), math.inf)
        for tube in report['segments'][0]['tubes']: tube['bound'] = bound
        smoke.validate_report(report)

    def test_false_tighter_bound_is_rejected(self):
        report = example(); report['segments'][0]['tubes'][0]['bound'] = 0.
        with self.assertRaises(AssertionError): smoke.validate_report(report)

    def test_wrong_rank_share_or_association_is_rejected(self):
        for field in ['rank', 'share', 'association']:
            report = example()
            if field == 'rank': report['summaries'][0][0]['rank'] = [2, 2]
            elif field == 'share': report['summaries'][0][0]['share']['value'] = {'lower': .1, 'upper': .2}
            else: report['segments'][0]['association']['matching'] = [1, 0]
            with self.assertRaises(AssertionError): smoke.validate_report(report)

    def test_incomplete_report_is_rejected(self):
        report = example(); report['completed'] = False
        with self.assertRaises(AssertionError): smoke.validate_report(report)


if __name__ == '__main__': unittest.main()
