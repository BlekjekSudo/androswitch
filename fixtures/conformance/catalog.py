"""Freeze the actual SDK catalog and independent JVM oracle into one APK."""
import argparse
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
root = Path(__file__).resolve().parent
groups = [json.loads((root / f'{name}-cases.json').read_text()) for name in ['pure', 'android', 'native']]
oracle = dict(line.split('\t', 1) for line in (args.output / 'jvm-oracle.tsv').read_text(encoding='utf-8-sig').splitlines())
assert len(oracle) == len(groups[0])
rows = []
for group_index, group in enumerate(groups):
    for local_index, row in enumerate(group):
        rows.append(dict(row, index=len(rows), local_index=local_index,
                         family=['jvm', 'android', 'native'][group_index],
                         kind=row.get('kind', 'contract'),
                         expected=oracle[str(local_index)] if group_index == 0 else None))
assert len({row['id'] for row in rows}) == len(rows)
assert not any('sensor' in row['group'].lower() for row in rows)
catalog = dict(schema_version=1, reference_api=26, sensor_tests=False,
               coverage_scope='Declared cases only, not all Android behavior or CTS certification', cases=rows)
encoded = (json.dumps(catalog, indent=2, ensure_ascii=True) + '\n').encode()
(args.output / 'catalog.json').write_bytes(encoded)
digest = hashlib.sha256(encoded).hexdigest()
def array(name, values):
    return 'public static final String[] ' + name + '={' + ','.join(json.dumps(v, ensure_ascii=True) for v in values) + '};\n'
source = 'package org.androswitch.conformance;\npublic final class Catalog {\n'
source += f'public static final int PURE_COUNT={len(groups[0])},ANDROID_COUNT={len(groups[1])};\n'
source += f'public static final String DIGEST="{digest}";\n'
for name,key in [('IDS','id'),('GROUPS','group'),('KINDS','kind'),('EXPECTED','expected')]:
    source += array(name, [row[key] for row in rows])
(args.output / 'generated/Catalog.java').write_text(source + '}\n')
print(f'Catalog: {len(rows)} cases; {len(oracle)} actual JVM rows; SHA256 {digest}')
