"""Bind an ordinary app journal to the exact APK/NRO and launcher sessions."""
import argparse
import collections
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--receipt', type=Path, required=True)
parser.add_argument('--catalog', type=Path, required=True)
parser.add_argument('--package', required=True)
args = parser.parse_args()
receipt = json.loads(args.receipt.read_text(encoding='utf-8-sig'))
catalog = json.loads(args.catalog.read_bytes())
assert hashlib.sha256(Path(receipt['artifact']).read_bytes()).hexdigest() == receipt['nro_sha256']
run = args.receipt.parent
digest = hashlib.sha256(args.catalog.read_bytes()).hexdigest()
sd = Path(receipt['sd_result'])
journal_path = sd / 'userdata' / args.package / 'files/conformance/report.json'
journal = json.loads(journal_path.read_bytes())
assert journal['catalog_sha256'] == digest, 'journal belongs to another catalog'
assert len(journal['cases']) == len(catalog['cases'])
assert [r['id'] for r in journal['cases']] == [r['id'] for r in catalog['cases']]
assert len(receipt['inputs']) == 1
apk = run / receipt['inputs'][0]['staged_name']
# The exact staged APK lives in the saved SD when the harness did not copy it beside the receipt.
if not apk.exists():
    apk = sd / 'apks' / receipt['inputs'][0]['staged_name']
assert hashlib.sha256(apk.read_bytes()).hexdigest() == receipt['inputs'][0]['sha256']
launcher_path = run / 'emulator.json'
launcher = json.loads(launcher_path.read_bytes()) if launcher_path.exists() else None
sessions = []
for path in sorted((run / 'sessions').glob('*/session-report.json')):
    session = json.loads(path.read_bytes())
    assert session['session_id'] in {row['expected_session_id'] for row in receipt['launches']}
    assert session['package'] == args.package
    assert path.parent.name == session['session_id']
    pending = path.parent / 'userdata' / args.package / 'files/conformance' / digest[:12] / 'pending.txt'
    case = None
    if pending.exists():
        index = int(pending.read_text().strip())
        assert 0 <= index < len(catalog['cases'])
        case = catalog['cases'][index]['id']
    sessions.append(dict(session_id=session['session_id'], status=session['status'],
                         pending_case=case, error=session.get('error'), raw_report=str(path)))
counts = collections.Counter(row['status'] for row in journal['cases'])
allowed = {'PASS', 'FAIL', 'UNSUPPORTED', 'BLOCKED', 'NOT_RUN', 'INTERRUPTED', 'ERROR'}
assert set(counts) <= allowed
(run / 'conformance-report.json').write_bytes(journal_path.read_bytes())
summary = dict(schema_version=1, status='observed_catalog_not_compatibility_pass',
               backend='Horizon via diagnostic Ryujinx; not physical hardware or stock Ryujinx',
               catalog_sha256=digest, total=len(journal['cases']), counts=dict(counts),
               apk_sha256=receipt['inputs'][0]['sha256'], nro_sha256=receipt['nro_sha256'],
               external_status=receipt['status'], original_sd_restore=receipt.get('sd_original_restore'),
               launcher_completed=bool(launcher and launcher['status'] == 'passed_execution'),
               sessions=sessions, android_reference='not_run', physical_hardware='not_run',
               scope='An interrupted case remains INTERRUPTED; matching raw runtime errors explain it without inventing success.')
(run / 'conformance-summary.json').write_text(json.dumps(summary, indent=2) + '\n', encoding='utf-8')
print(json.dumps(dict(total=summary['total'], counts=dict(counts), sessions=len(sessions))))
