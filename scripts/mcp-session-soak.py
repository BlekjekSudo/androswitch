#!/usr/bin/env python3
"""Bounded, real-stdio/native/UI functional endurance; no performance claims.

Requires APKs built by golden/native-languages/build.py and a native/JIT rd.
Each of the 18 language/ABI/JIT combinations gets a persistent session. The
summary retains counters, independent result oracles and a rolling transcript
hash rather than accumulating unbounded screenshot/base64 transcripts.
"""
import argparse
import base64
from collections import Counter, deque
import hashlib
import json
import os
from pathlib import Path
import selectors
import shutil
import tempfile
import signal
import struct
import subprocess
import time


def digest(tag, left, right):
    value = 2166136261 ^ tag
    for n in (left, right):
        for shift in (0, 8, 16, 24):
            value = ((value ^ ((n >> shift) & 255)) * 16777619) & 0xffffffff
    return value & 0x7fffffff


class Client:
    def __init__(self, rd, apks, out, ident, abi, jit):
        self.ident, self.abi, self.jit = ident, abi, jit
        self.out = out
        out.mkdir(parents=True, exist_ok=True)
        self.seq, self.pending, self.session_id = 0, b'', None
        self.calls, self.history = Counter(), deque(maxlen=24)
        self.transcript = hashlib.sha256()
        self.cycles, self.oracles, self.image_pairs, self.peak_rss_kb = 0, 0, 0, 0
        env = dict(os.environ, RD_NATIVE_ABI=abi, RD_JIT=jit, RD_GC='on',
                   RD_MCP_SANDBOX=os.pathsep.join((str(apks), str(out))))
        self.stderr = (out / 'stderr.log').open('wb')
        self.process = subprocess.Popen([str(rd), 'mcp'], stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=self.stderr,
                                        env=env, bufsize=0, start_new_session=True)
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ)
        try:
            self.rpc('initialize', {'protocolVersion': '2024-11-05', 'capabilities': {},
                                   'clientInfo': {'name': 'functional-endurance', 'version': '1'}})
            self.notify('notifications/initialized')
            self.names = {t['name'] for t in self.rpc('tools/list')['result']['tools']}
            self.call('install', {'apk': str(apks / ('native-' + ident + '.apk'))})
            self.call('permissions_grant', {'permission': 'android.permission.POST_NOTIFICATIONS'})
            self.engine_id = {'c': 'c-ndk', 'cpp': 'cpp-ndk', 'rust': 'rust-ndk'}[ident]
            self.tag = {'c': 1, 'cpp': 2, 'rust': 3}[ident]
            self.package = 'org.rustydroid.demo.' + ident
            self.launch()
            self.initial_native = self.call('native_state')['instructions']
        except BaseException:
            self.close()
            raise

    def notify(self, method):
        self.process.stdin.write((json.dumps({'jsonrpc': '2.0', 'method': method}) + '\n').encode())

    def rpc(self, method, params=None):
        self.seq += 1
        request = {'jsonrpc': '2.0', 'id': self.seq, 'method': method, 'params': params or {}}
        self.history.append({'request': request})
        self.process.stdin.write((json.dumps(request) + '\n').encode())
        deadline = time.monotonic() + 60
        while True:
            while b'\n' not in self.pending:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not self.selector.select(remaining):
                    raise RuntimeError('stdio request timed out: ' + method)
                chunk = os.read(self.process.stdout.fileno(), 65536)
                if not chunk:
                    raise RuntimeError('MCP exited: ' + str(self.process.poll()))
                self.pending += chunk
            line, self.pending = self.pending.split(b'\n', 1)
            response = json.loads(line)
            self.transcript.update(line + b'\n')
            if response.get('id') == self.seq:
                break
        assert 'error' not in response, response
        return response

    def call(self, name, args=None, error=None):
        result = self.rpc('tools/call', {'name': name, 'arguments': args or {}})['result']
        sid = result['sessionId']
        if self.session_id is None:
            self.session_id = sid
        assert sid == self.session_id, result
        self.last_result = result
        self.calls[name] += 1
        text = next((c['text'] for c in result.get('content', []) if c['type'] == 'text'), None)
        try:
            value = json.loads(text) if text is not None else None
        except ValueError:
            value = text
        if error:
            assert result.get('isError') and value['code'] == error, (name, value, error)
        else:
            assert not result.get('isError'), (name, value)
            assert not (isinstance(value, dict) and value.get('ok') is False), (name, value)
        self.history.append({'tool': name, 'isError': result.get('isError'),
                             'code': value.get('code') if isinstance(value, dict) else None})
        return value if text is not None else result

    def launch(self):
        self.call('launch', {'engine': self.engine_id})
        tree = self.call('accessibility', {'action': 'tree'})
        editors = [n['id'] for n in tree['nodes'] if n.get('editable')]
        assert len(editors) == 2, tree
        self.left, self.right = editors

    def image(self):
        self.call('screenshot')
        result = self.last_result
        assert not any(c['type'] == 'text' and 'draw pass failed' in c['text'] for c in result['content']), result
        image = next(c for c in result['content'] if c['type'] == 'image')
        raw = base64.b64decode(image['data'], validate=True)
        assert raw[:8] == b'\x89PNG\r\n\x1a\n'
        assert struct.unpack('>II', raw[16:24]) == (720, 1440)
        return hashlib.sha256(raw).hexdigest()

    def cycle(self):
        n = self.cycles
        left, right = (n * 37) % 8192 - 4096, (n * 19) % 4096 - 2048
        for view, text in ((self.left, str(left)), (self.right, str(right))):
            self.call('ime', {'action': 'focus', 'view': view})
            self.call('type_text', {'text': text, 'replace': True})
        if n % 30 == 0:
            self.call('type_text', {'text': 123, 'replace': True}, error='INVALID_FORMAT')
        selector = {'resource_id': self.package + ':id/compute', 'clickable': True}
        found = self.call('find_views', selector)
        assert found['total'] == 1 and found['total_is_exact'] and not found['truncated'], found
        token = found['tree_token']
        repeat = self.call('find_views', selector)
        assert repeat['tree_token'] == token
        tapped = self.call('tap_view', dict(selector, expected_token=token))
        assert tapped['listener_acionado'], tapped
        expected = 'Result: ' + str(digest(self.tag, left, right))
        self.call('assert_text', {'resource_id': self.package + ':id/result', 'expected': expected})
        self.oracles += 1
        present = self.call('wait_for_view', {'resource_id': self.package + ':id/result',
                                            'text': expected, 'timeout_ms': 0})
        assert present['condition_met'] and present['present'] and present['polls'] == 1, present
        absent = self.call('wait_for_view', {'text': 'not-a-real-soak-label',
                                           'state': 'absent', 'timeout_ms': 0})
        assert absent['condition_met'] and not absent['present'], absent
        self.call('tap_view', dict(selector, expected_token=token), error='STALE_VIEW')
        if 'last_error' in self.names:
            failure = self.call('last_error')['error']
            assert failure['tool'] == 'tap_view' and failure['code'] == 'STALE_VIEW', failure
            info = self.call('session_info')
            assert info['running'] and info['running_package'] == self.package, info
            tail = self.call('logs_tail', {'max_bytes': 31})
            self.call('logs_tail', {'max_bytes': 17, 'cursor': tail['next_cursor']})
            if n % 25 == 0:
                self.call('logs', {'clear': True})
                self.call('logs_tail', {'cursor': tail['next_cursor']}, error='STALE_LOG_CURSOR')
                failure = self.call('last_error', {'clear': True})['error']
                assert failure['tool'] == 'logs_tail' and failure['code'] == 'STALE_LOG_CURSOR'
                assert self.call('last_error')['error'] is None
        if n % 20 == 0:
            health = self.call('runtime_health', {'gc': True})
            assert health['running'] and health['heap_used_bytes'] < 256 * 1024 * 1024, health
            assert self.call('native_state')['instructions'] > self.initial_native
            first, second = self.image(), self.image()
            assert first == second, (first, second)
            self.image_pairs += 1
        if n and n % 100 == 0:
            self.launch()  # Refresh editor handles after a genuine warm lifecycle.
        self.cycles += 1
        try:
            for line in Path('/proc/' + str(self.process.pid) + '/status').read_text().splitlines():
                if line.startswith('VmRSS:'):
                    self.peak_rss_kb = max(self.peak_rss_kb, int(line.split()[1]))
        except FileNotFoundError:
            raise RuntimeError('MCP process disappeared')

    def summary(self):
        return {'language': self.ident, 'abi': self.abi, 'jit': self.jit,
                'cycles': self.cycles, 'result_oracles': self.oracles,
                'equal_png_pairs': self.image_pairs, 'calls': dict(self.calls),
                'peak_mcp_rss_kb': self.peak_rss_kb,
                'response_stream_sha256': self.transcript.hexdigest()}

    def close(self):
        if self.process.poll() is None:
            os.killpg(self.process.pid, signal.SIGTERM)
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(self.process.pid, signal.SIGKILL)
                self.process.wait(timeout=5)
        self.selector.close()
        self.stderr.close()


def main():
    if not __debug__:
        raise RuntimeError("Run the acceptance harness without Python -O; assertions are required")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rd', type=Path, required=True)
    parser.add_argument('--apks', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--seconds', type=float, default=60)
    args = parser.parse_args()
    assert 0 < args.seconds <= 7200, 'seconds must be in (0, 7200]'
    args.rd, args.apks, args.out = args.rd.resolve(), args.apks.resolve(), args.out.resolve()
    args.out.mkdir(parents=True, exist_ok=True)
    # Freeze the executable so rebuilds cannot silently mix versions between groups.
    pinned = tempfile.TemporaryDirectory(prefix='rd-mcp-soak-')
    binary = Path(pinned.name) / 'rd'
    shutil.copy2(args.rd, binary)
    args.rd = binary
    started = time.monotonic()
    result = {'success': False, 'requested_seconds': args.seconds,
              'binary_sha256': hashlib.sha256(args.rd.read_bytes()).hexdigest(), 'sessions': []}
    current = []
    try:
        abis = ('x86_64', 'arm64-v8a', 'armeabi-v7a')
        for group in range(6):
            deadline = started + args.seconds * (group + 1) / 6
            jit = ('off', 'on')[group // 3]
            current = []
            for index, ident in enumerate(('c', 'cpp', 'rust')):
                abi = abis[(index + group) % 3]
                current.append(Client(args.rd, args.apks, args.out / (jit + '-' + ident + '-' + abi), ident, abi, jit))
            # Even short smoke runs perform one complete oracle per session.
            while True:
                for client in current:
                    client.cycle()
                if time.monotonic() >= deadline:
                    break
            for client in current:
                warm = client.call('launch', {'engine': client.engine_id})
                assert warm['warm'], warm
                result['sessions'].append(client.summary())
                client.close()
            current = []
            print('FUNCTIONAL_SOAK_GROUP_PASS', group + 1,
                  'elapsed_s', round(time.monotonic() - started, 2), flush=True)
        result['success'] = True
    except BaseException as error:
        result['failure'] = repr(error)
        result['active_sessions'] = [dict(c.summary(), recent_events=list(c.history)) for c in current]
        raise
    finally:
        for client in current:
            client.close()
        pinned.cleanup()
        result['elapsed_seconds'] = round(time.monotonic() - started, 3)
        result['total_calls'] = sum(sum(s['calls'].values()) for s in result['sessions'])
        result['total_oracles'] = sum(s['result_oracles'] for s in result['sessions'])
        (args.out / 'summary.json').write_text(json.dumps(result, indent=2) + '\n')
    print('FUNCTIONAL_SOAK_PASS', result['total_calls'], 'calls', result['total_oracles'], 'native result oracles', flush=True)


if __name__ == '__main__':
    main()
