#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Track rendered primary messages and related information from pinned evidence."""
import argparse
import json
import subprocess
from pathlib import Path
from record_diagnostic_positions import legacy_flags


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    fixtures = Path(__file__).resolve().parents[4] / 'backend/bluets/tests/fixtures'
    reference = json.loads((fixtures / 'diagnostics/reference.json').read_text())
    rows = []
    for case in reference['cases']:
        expected = case['first']
        if not expected:
            continue
        if 'config' in case:
            source_root = (fixtures / case['config']).parent
            command = ['--project', str(fixtures / case['config']), '--noEmit']
        else:
            entry = fixtures / 'typescript_oracle' / case['entry']
            source_root = entry.parent
            command = ['check', str(entry), *legacy_flags(case['flags'])]
        output = subprocess.run([str(args.binary), *command, '--diagnostics-json'],
                                text=True, capture_output=True, check=False)
        primary = json.loads(output.stderr.splitlines()[0])['typescript']
        if not primary:
            raise ValueError(f"Unmapped rejected program: {case['id']}")
        related = []
        for item in expected.get('related', []):
            filename = item['file']
            module = filename if filename.startswith('<typescript-lib>/') else str(
                (fixtures / filename).relative_to(source_root))
            related.append({'code': item['code'], 'message': item['message'],
                            'module': module, 'position': {key: item[key]
                            for key in ('line', 'column', 'length')}})
        actual_related = [{'code': item['code'], 'message': item['message'],
                           'module': item['span']['module'], 'position': item['position']}
                          for item in primary.get('relatedInformation', [])]
        differences = {}
        if primary['message'] != expected['message']:
            differences['message'] = {'expected': expected['message'], 'actual': primary['message']}
        if actual_related != related:
            differences['relatedInformation'] = {'expected': related, 'actual': actual_related}
        if differences:
            rows.append({'id': case['id'], 'code': expected['code'], **differences})
    record = {'version': reference['version'], 'comparedPrimaries': 800,
              'mismatches': rows}
    args.output.write_text(json.dumps(record, indent=2) + '\n')
    print(json.dumps({'mismatches': len(rows), 'messages': sum('message' in r for r in rows),
                      'relatedInformation': sum('relatedInformation' in r for r in rows)}))


if __name__ == '__main__':
    main()
