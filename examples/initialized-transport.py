"""Run one checked initialized path and its separately charged covariance history."""
from pathlib import Path
import json
import struct

import eigentrail


def number(value):
    return struct.unpack('>d', bytes.fromhex(value[4:]))[0]


def main():
    root = Path(__file__).resolve().parents[1]
    inputs = root / 'examples/requests'
    request = (inputs / 'transport-geometry-request.json').read_text()
    output = eigentrail.analyze_transport_json(request)
    eigentrail.check_transport_json(request, output)
    report = json.loads(output)
    print('geometry:', report['outcome']['kind'],
          'frame error <=', number(report['budget'][0]['comparison_error']))
    for label, filename, expected in [
        ('covariance-observation-bridge', 'transport-history-request.json', 'Resolved'),
        ('coarse-observations-remain-unresolved', 'transport-coarse-history-request.json', 'Unresolved'),
    ]:
        request = (inputs / filename).read_text()
        output = eigentrail.analyze_histories_json(request)
        eigentrail.check_histories_json(request, output)
        transport = json.loads(output)['configurations'][0]['transports'][0]
        covariance = transport['covariance'][0]
        print(label, ':', covariance['outcome'],
              'observation mesh =', number(covariance['budget'][0]['mesh']),
              'total error <=', number(covariance['budget'][0]['total_error']))
        assert covariance['outcome']['kind'] == expected


if __name__ == '__main__':
    main()
