"""Display checked bounds for a represented constant covariance history."""
import json
from pathlib import Path
import struct


def display(value):
    if isinstance(value, list):
        return [display(item) for item in value]
    if isinstance(value, dict):
        return {key: display(item) for key, item in value.items()}
    if isinstance(value, str) and value.startswith('str:'):
        return value[4:]
    if isinstance(value, str) and value.startswith('f64:'):
        return struct.unpack('>d', bytes.fromhex(value[4:]))[0]
    return value


def main():
    from eigentrail import analyze_lineage_summary_json, check_lineage_summary_json

    request_path = Path(__file__).with_name('requests') / 'lineage-summary-request.json'
    request_json = request_path.read_text()
    report_json = analyze_lineage_summary_json(request_json)
    assert check_lineage_summary_json(request_json, report_json) is True
    report = display(json.loads(report_json))
    assert report['completed'] is True
    for row in report['rows']:
        state = row['state']
        if state['kind'] != 'Bounds':
            print(row['time'], state['kind'])
            continue
        target = state['target']
        if target['kind'] == 'OrderedBlock' and target['start'] == 0:
            assert state['information_level'] == 'Line'
            assert state['motion'] == 'WithinTolerance'
            assert state['accuracy_target_attained'] is True
            print(f"t={row['time']:g}: {state['information_level']}, "
                  f"motion={state['motion']}, projector error<={state['projector_error']:.6g}, "
                  f"accuracy target attained={state['accuracy_target_attained']}")
    print('Feasibility:', report['history']['configurations'][0]['feasibility']['kind'])
    print('Supplied model and calibration remain assumptions; origins are outer block associations.')
    # Retain request_json and report_json unchanged for certificate replay.


if __name__ == '__main__':
    main()
