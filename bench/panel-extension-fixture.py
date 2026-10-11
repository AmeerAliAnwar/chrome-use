#!/usr/bin/env python3
"""Own an isolated headed Chrome/profile/native host for panel acceptance.

Run on the SSH build host, never the maintainer's desktop. CDP observes extension
surfaces, so this fixture is not evidence for stealth detector results.
"""
import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import socket
import struct
import subprocess
import tempfile
import threading
import time
import uuid

parser = argparse.ArgumentParser()
parser.add_argument('--binary', required=True)
parser.add_argument('--binary-sha256', required=True)
parser.add_argument('--chrome', required=True)
parser.add_argument('--extension', required=True)
parser.add_argument('--agent-program', required=True)
parser.add_argument('--real-agent', action='store_true')
parser.add_argument('--output', required=True)
args = parser.parse_args()
output = Path(args.output).resolve()
root = Path(tempfile.mkdtemp(prefix='chrome-use-panel-fixture-'))
profile = root / 'profile'
profile.mkdir()
registry = profile / 'relay-registry'
registry.mkdir(mode=0o700)
binary = root / 'chrome-use'
assert hashlib.sha256(Path(args.binary).read_bytes()).hexdigest() == args.binary_sha256
shutil.copy2(args.binary, binary)
extension = root / 'extension'
shutil.copytree(args.extension, extension, ignore=shutil.ignore_patterns('*.test.js'))
launcher = root / 'native-host.sh'
launcher.write_text('#!/bin/sh\nexport CHROME_USE_NO_UPDATE_CHECK=1\nexport CHROME_USE_RELAY_DIR=' + shlex.quote(str(registry)) + '\nexec ' + shlex.quote(str(binary)) + ' __nm-host "$@"\n')
launcher.chmod(0o700)
manifests = profile / 'NativeMessagingHosts'
manifests.mkdir()
(manifests / 'com.agent_browser.connect.json').write_text(json.dumps({
    'name': 'com.agent_browser.connect', 'description': 'Isolated panel fixture',
    'path': str(launcher), 'type': 'stdio',
    'allowed_origins': ['chrome-extension://ciiljdlhdpfckdcfkphgmfalanpdejep/'],
}))
clicks = 0


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_GET(self):
        if self.path == '/state':
            body, kind = json.dumps({'clicks': clicks}).encode(), 'application/json'
        elif self.path == '/fixture':
            body = b'''<html><title>Panel browser fixture</title><body><button onclick="fetch('/clicked',{method:'POST'}).then(()=>document.getElementById('status').textContent='Action verified')">Verify panel browser task</button><p id="status">Not verified</p></body></html>'''
            kind = 'text/html'
        else:
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header('Content-Type', kind)
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        global clicks
        if self.path != '/clicked':
            self.send_error(404)
            return
        clicks += 1
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b'{}')


server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
log = (root / 'chrome.log').open('w')
process = subprocess.Popen([
    args.chrome, '--no-first-run', '--no-default-browser-check', '--disable-sync',
    '--disable-features=MacAppCodeSignClone', '--password-store=basic', '--use-mock-keychain',
    '--user-data-dir=' + str(profile), '--load-extension=' + str(extension),
    '--disable-extensions-except=' + str(extension), '--remote-debugging-port=0', 'about:blank',
], stdout=log, stderr=log, start_new_session=True)


def private_rpc(endpoint, operation, params):
    """Only the capability of this fixture's profile is read, never printed."""
    host, port = endpoint['address'].rsplit(':', 1)
    assert host == '127.0.0.1'
    request_id = uuid.uuid4().hex
    raw = json.dumps({'capability': endpoint['capability'], 'requestId': request_id, 'operation': operation, 'params': params}).encode()
    with socket.create_connection((host, int(port)), timeout=3) as stream:
        stream.sendall(struct.pack('<I', len(raw)) + raw)
        def read(count):
            data = b''
            while len(data) < count:
                part = stream.recv(count - len(data))
                if not part:
                    raise RuntimeError('Private fixture RPC closed')
                data += part
            return data
        length = struct.unpack('<I', read(4))[0]
        assert length < 768 * 1024
        reply = json.loads(read(length))
        assert reply['requestId'] == request_id
        if 'error' in reply:
            raise RuntimeError('Private fixture operation failed: ' + operation)
        return reply['result']


verdict = 1
try:
    deadline = time.monotonic() + 20
    portfile = profile / 'DevToolsActivePort'
    while not portfile.exists():
        if process.poll() is not None or time.monotonic() > deadline:
            raise RuntimeError('Owned fixture Chrome did not become ready')
        time.sleep(.1)
    port, path = portfile.read_text().splitlines()[:2]
    state = root / 'state.json'
    state.write_text(json.dumps({'pid': process.pid, 'profile': str(profile), 'registry': str(registry), 'binarySha256': args.binary_sha256, 'browserWebSocket': 'ws://127.0.0.1:' + port + path}))
    command = ['node', str(Path(__file__).with_name('panel-extension-check.mjs')),
               '--state-file', str(state), '--output', str(output)]
    if args.real_agent:
        command += ['--real-agent', str(Path(args.agent_program).resolve()), '--fixture-url', f'http://127.0.0.1:{server.server_port}/fixture', '--timeout-ms', '90000']
    else:
        command += ['--fixture-agent', str(Path(args.agent_program).resolve())]
    verdict = subprocess.run(command, timeout=150).returncode
finally:
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait(timeout=5)
    log.close()
    server.shutdown()
    # A failed UI check must not leave a native service or real model running.
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        endpoints = list((registry / 'agent-panel/profiles').glob('*/service.json'))
        if not endpoints:
            break
        for path in endpoints:
            try:
                endpoint = json.loads(path.read_text())
                chats = private_rpc(endpoint, 'conversationList', {})['conversations']
                for chat in chats:
                    if chat['phase'] in ('starting', 'running', 'waitingInput', 'stopping'):
                        private_rpc(endpoint, 'conversationStop', {'conversationId': chat['id']})
                private_rpc(endpoint, 'serviceStop', {})
            except (OSError, ValueError, RuntimeError):
                pass
        time.sleep(.1)
    if list((registry / 'agent-panel/profiles').glob('*/service.json')):
        raise RuntimeError('Fixture native service exit was not confirmed; preserve ' + str(root))
    print(json.dumps({'fixtureRoot': str(root), 'nativeServicesExited': True, 'checkExit': verdict}))
raise SystemExit(verdict)
