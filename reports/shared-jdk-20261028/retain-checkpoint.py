"""Freeze measured numeric/String contracts, without granting API parity."""
import argparse
import hashlib
import importlib.util
import json
import subprocess
import tomllib
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEST = Path(__file__).resolve().parent
LOGS = ROOT / 'local-artifacts/upstream-numerics'
FIXTURE = ROOT / 'local-artifacts/conformance-strings-qualified'
GIT = 'C:/Program Files/Git/cmd/git.exe'
SPEC = importlib.util.spec_from_file_location('parity', ROOT / 'scripts/report-upstream-parity.py')
PARITY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PARITY)


def read(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def retain(runtime_commit, test_commit, run, visual_reviewed):
    evidence = []

    def keep(path, relative):
        raw = path.read_bytes()
        target = DEST / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(raw)
        evidence.append(dict(path=target.relative_to(ROOT).as_posix(), bytes=len(raw),
                             sha256=hashlib.sha256(raw).hexdigest()))

    def git(*args):
        return subprocess.check_output([GIT, '-c', 'core.fsmonitor=false', '-C', str(ROOT), *args])

    runtime_commit = git('rev-parse', runtime_commit).decode().strip()
    test_commit = git('rev-parse', test_commit).decode().strip()
    # The final product regression corrects a stale test, not the frozen runtime.
    delta = git('diff', '--name-only', runtime_commit, test_commit).decode().splitlines()
    assert delta == ['crates/as-switch-smoke/tests/portable_core.rs'], delta
    keep(LOGS / 'jdk-product-stale-intern-test.log', 'diagnostics/stale-product-test.log')
    for name in ('jdk-checks-before-evidence-policy.json', 'jdk-source-before-evidence-policy.json', 'jdk-source-before-evidence-policy.log'):
        keep(LOGS / name, 'diagnostics/' + name)
    checks = read(LOGS / 'jdk-checks.json')
    assert len(checks) == 11 and all(c['exit_code'] == c['expected_exit'] for c in checks)
    assert all(sha(Path(c['log'])) == c['sha256'] for c in checks)
    keep(LOGS / 'jdk-checks.json', 'checks/commands.json')
    keep(ROOT / 'scripts/report-upstream-parity.py', 'reporting/parser.py')
    keep(ROOT / 'scripts/tests/test_upstream_parity_report.py', 'reporting/parser-tests.py')
    keep(LOGS / 'jdk-report-parser-tests.log', 'reporting/execution.log')
    for c in checks:
        keep(Path(c['log']), 'checks/' + Path(c['log']).name)
    tests = {}
    commands = {
        'jdk-product': 'cargo test -p rd-vm -p as-framework -p as-switch-smoke --features as-framework/host,as-switch-smoke/host-cli,as-switch-smoke/portable-tests --locked --no-fail-fast -- --test-threads=1 --include-ignored',
        'jdk-reference': 'cargo test -p as-vm --features upstream-parity-tests --lib --tests --locked --no-fail-fast -- --test-threads=1',
    }
    for name in ('jdk-shipping', 'jdk-gc', 'jdk-fixture-tests'):
        c = next(c for c in checks if c['name'] == name)
        commands[name] = c['program'] + ' ' + ' '.join(c['arguments'])
    for name, command in commands.items():
        path = LOGS / (name + '.log')
        try:
            row = PARITY.log_evidence(path, command)
        except ValueError as error:
            raise ValueError(f'{name}: {error}') from error
        assert row['test_run_completed_successfully'] and row['totals']['passed'] > 0
        assert not any(row['totals'][k] for k in ('failed', 'ignored', 'filtered')), row['totals']
        tests[name] = row
        if name in ('jdk-product', 'jdk-reference'):
            keep(path, 'tests/' + path.name)
    manifest = tomllib.loads(git('show', test_commit + ':crates/as-vm/Cargo.toml').decode())
    assert not PARITY.verify_reference_routes(tests['jdk-reference']['suites'], manifest)
    source = read(LOGS / 'jdk-source.json')
    assert source['source_sync_pass'] and not source['device_behavior_parity_pass']
    assert not read(LOGS / 'jdk-device-gate.json')['device_behavior_parity_pass']
    for name in ('jdk-source.json', 'jdk-device-gate.json'):
        keep(LOGS / name, 'checks/' + name)
    build_dir = ROOT / 'local-artifacts/m9/shared-jdk-7b59cdb'
    build = read(build_dir / 'product-build.json')
    assert build['source_commit'] == runtime_commit and build['status'] == 'built'
    assert sha(build_dir / 'androswitch.nro') == build['nro_sha256']
    frozen_manifest = git('show', runtime_commit + ':data/rustydroid-upstream.json')
    assert (build_dir / 'source-inventory.json').read_bytes().replace(b'\r\n', b'\n') == frozen_manifest
    for name in ('product-build.json', 'product-source.json', 'product-std-platform.json', 'product-mesa-link.json', 'source-inventory.json'):
        keep(build_dir / name, 'horizon/' + name)
    keep(LOGS / 'jdk-nro-build.log', 'horizon/build.log')
    for name in ('catalog.json', 'build-receipt.json', 'jvm-oracle.tsv', 'signature-verification.log'):
        keep(FIXTURE / name, 'fixture/' + name)
    for name in ('StaticOracle.java', 'build.ps1', 'catalog.tsv', 'receipt.json'):
        keep(ROOT / 'fixtures/numeric-oracle' / name, 'numeric-oracle/' + name)
    oracle_receipt = read(ROOT / 'fixtures/numeric-oracle/receipt.json')
    assert sha(ROOT / 'fixtures/numeric-oracle/catalog.tsv') == oracle_receipt['catalog_sha256']
    assert sha(ROOT / 'fixtures/numeric-oracle/StaticOracle.java') == oracle_receipt['source_sha256']
    numeric = (LOGS / 'jdk-shipping.log').read_text(encoding='utf-8-sig')
    assert 'every_inherited_numeric_declaration_with_a_jvm_recipe_executes_the_same_contract' in numeric
    catalog = read(FIXTURE / 'catalog.json')
    ids = [r['id'] for r in catalog['cases']]
    assert len(ids) == len(set(ids)) == 299
    pure = [r for r in catalog['cases'] if r['family'] == 'jvm']
    expected = dict((int(s.split('\t', 1)[0]), s.split('\t', 1)[1])
                    for s in (FIXTURE / 'jvm-oracle.tsv').read_text(encoding='utf-8').splitlines())
    host = read(LOGS / 'jdk-sdk-report.json')
    actual = {r['id']: r for r in host['cases']}
    assert len(pure) == 173 and all(actual[r['id']]['status'] == 'PASS' and actual[r['id']]['detail'] == expected[r['index']] for r in pure)
    keep(LOGS / 'jdk-sdk-report.json', 'host/report.json')
    receipt = read(run / 'verification.json')
    summary = read(run / 'conformance-summary.json')
    assert receipt['nro_sha256'] == build['nro_sha256'] and receipt['sd_original_restore'] == 'restored'
    assert receipt['root_handle_pinned'] and not receipt.get('root_stop_error')
    assert summary['apk_sha256'] == sha(FIXTURE / 'AndroSwitchConformance.apk')
    assert summary['catalog_sha256'] == sha(FIXTURE / 'catalog.json')
    assert summary['launcher_completed'] and not summary['acceptance_passed']
    device = read(run / 'conformance-report.json')
    assert [r['id'] for r in device['cases']] == ids
    counts = Counter(r['status'] for r in device['cases'])
    assert counts == {'PASS': 296, 'NOT_RUN': 3}, counts
    assert {r['id'] for r in device['cases'] if r['status'] == 'NOT_RUN'} == {
        'host.cpu.clock_hz', 'host.gpu.clock_hz', 'host.memory.clock_hz'}
    # Retain the exact resume journal source, plus the host paging-file failure.
    for label, folder in (
        ('initial', ROOT / 'local-artifacts/m9/external-runs/04d0c935f65949acb57b846ec0f71222'),
        ('paging-file-failure', ROOT / 'local-artifacts/m9/external-runs/1b0d86101e0d41aea2786354ea727501'),
        ('completed-resume', run),
    ):
        prior = read(folder / 'verification.json')
        assert prior['sd_original_restore'] == 'restored' and prior['nro_sha256'] == build['nro_sha256']
        for name in ('verification.json', 'conformance-summary.json', 'conformance-report.json', 'emulator.json', 'launcher-test.json'):
            if (folder / name).is_file():
                keep(folder / name, 'emulator/' + label + '/' + name)
        for path in folder.rglob('*'):
            if path.is_file() and path.suffix in ('.log', '.png'):
                keep(path, 'emulator/' + label + '/' + path.relative_to(folder).as_posix())
    from PIL import Image
    for capture in receipt['cases'][0]['captures']:
        ppm = Path(capture['file'])
        png = ppm.with_suffix('.png')
        assert sha(ppm) == capture['sha256']
        with Image.open(ppm) as original, Image.open(png) as retained:
            assert original.size == retained.size and original.tobytes() == retained.tobytes()
    report = dict(
        schema=1, status='not_yet_1_to_1', all_upstream_apis_1_to_1_on_horizon=False,
        upstream_repository='https://github.com/nadabgfccvd/RustyDroid',
        upstream_revision=json.loads(frozen_manifest)['revision'],
        implementation_commit=runtime_commit, regression_test_commit=test_commit,
        test_only_delta=delta, tests=tests, source_audit=source, horizon_build=build,
        numeric_oracle=dict(measured_comparisons=233, distinct_inherited_declarations=218,
                            scope='Fixed JVM input samples, not every Unicode/float value'),
        host_sdk_counts=dict(Counter(r['status'] for r in host['cases'])),
        dex_jvm_rows_matched=173, emulator=dict(run=run.name, counts=dict(counts),
            resumed_same_apk_nro=True, available_baseline_completed=True, full_catalog_acceptance=False),
        visual_reviewed=visual_reviewed, physical_device='not_run_for_this_binary',
        games_first_frame='not_proved',
        diagnostics_scope='The stale product test and initial partial run are not passes; the failed resume is a host paging-file allocation failure before app startup.',
        open_boundaries_document='docs/UPSTREAM_PARITY_PLAN.md', evidence=evidence)
    output = DEST.with_suffix('.json')
    output.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8', newline='\n')
    print(json.dumps(dict(report=str(output), tests={k:v['totals'] for k,v in tests.items()}, emulator=report['emulator'])))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--runtime-commit', required=True)
    parser.add_argument('--test-commit', required=True)
    parser.add_argument('--run', type=Path, required=True)
    parser.add_argument('--visual-reviewed', action='store_true')
    args = parser.parse_args()
    retain(args.runtime_commit, args.test_commit, args.run, args.visual_reviewed)
