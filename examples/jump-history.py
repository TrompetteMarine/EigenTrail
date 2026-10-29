"""Inspect one checked complete jump history and its retained query alternatives."""
from pathlib import Path
import json

import eigentrail


def main():
    root = Path(__file__).resolve().parents[1]
    request = (root / 'examples/requests/jump-history-request.json').read_text()
    output = eigentrail.analyze_histories_json(request)
    eigentrail.check_histories_json(request, output)
    for configuration in json.loads(output)['configurations']:
        print(configuration['id'], 'complete-history evidence:', configuration['feasibility']['kind'])
        for workflow in configuration.get('jump_workflow', []):
            print('smooth cell components:', [component['cells'] for component in workflow['components']])
            for index, query in enumerate(workflow['queries']):
                state = query['state']
                print('query', index, ':', state['kind'],
                      'retained sides:', len(state.get('sides', [])))
            print('checked step witnesses:', len(workflow['witnesses']))


if __name__ == '__main__':
    main()
