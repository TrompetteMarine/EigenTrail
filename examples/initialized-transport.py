"""Run one checked initialized path and its separately charged covariance history."""
from pathlib import Path
import json
import struct

import eigentrail


def number(value):
    return struct.unpack('>d', bytes.fromhex(value[4:]))[0]


def wire(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'))


def main():
    root = Path(__file__).resolve().parents[1]
    data = json.loads((root / 'fixtures/oracles/initialized_transport.json').read_text())
    geometry = data['cases'][0]
    request = wire(geometry['request'])
    output = eigentrail.analyze_transport_json(request)
    eigentrail.check_transport_json(request, output)
    report = json.loads(output)
    print('geometry:', report['outcome']['kind'],
          'frame error <=', number(report['budget'][0]['comparison_error']))
    for case in data['history_cases']:
        request = wire(case['request'])
        output = eigentrail.analyze_histories_json(request)
        eigentrail.check_histories_json(request, output)
        transport = json.loads(output)['configurations'][0]['transports'][0]
        covariance = transport['covariance'][0]
        print(case['id'], ':', covariance['outcome'],
              'observation mesh =', number(covariance['budget'][0]['mesh']),
              'total error <=', number(covariance['budget'][0]['total_error']))
        assert covariance['outcome']['kind'] == case['expectation']['covariance']


if __name__ == '__main__':
    main()
