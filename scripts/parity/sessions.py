"""Explicit, owned scratch sessions. No discovery of existing user sessions."""
import hashlib
import errno
import json
import os
from pathlib import Path
import re
import shutil
import socket
import struct
import subprocess
import tempfile
import time
import tomllib
import uuid


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def scratch_environment(root):
    env = {'PATH': str(root / 'bin') + ':/usr/bin:/bin', 'LANG': 'C.UTF-8',
           'TERM': 'xterm-256color', 'COLORTERM': 'truecolor', 'SHELL': '/bin/sh',
           'HERDR_DISABLE_SOUND': '1'}
    for key, name in [('HOME', 'h'), ('XDG_CONFIG_HOME', 'c'),
                      ('XDG_STATE_HOME', 's'), ('XDG_RUNTIME_DIR', 'r'),
                      ('XDG_CACHE_HOME', 'a'), ('TMPDIR', 'tmp')]:
        path = root / name
        path.mkdir(mode=0o700)
        env[key] = str(path)
    (root / 'bin').mkdir()
    return env


def copy_config(original, env):
    """Read a baseline; modify only the private copy to disable update checks."""
    text = original.read_text()
    parsed = tomllib.loads(text)
    allowed = {'onboarding', 'keys', 'ui', 'theme', 'update'}
    if set(parsed) - allowed:
        raise ValueError('parity config has unaudited startup/connection sections')
    # Tab status commands execute automatically in the reference server.
    for entry in parsed.get('ui', {}).get('tab_bar_right', []):
        if not isinstance(entry, dict) or entry.get('type') == 'command' or 'command' in entry:
            raise ValueError('parity baseline contains executable tab-bar configuration')
    # Only these audited chords are permitted for custom commands: the
    # Alt+F popup and the Alt+P pane command, both executing the same inert
    # scratch shell. Arbitrary direct/prefix aliases are refused.
    for binding in parsed.get('keys', {}).get('command', []):
        if not isinstance(binding, dict):
            raise ValueError('unexpected command binding shape')
        keys = binding.get('key', [])
        keys = [keys] if isinstance(keys, str) else keys
        allowed = {'alt+f': 'popup', 'alt+p': 'pane'}
        if not keys or any(''.join(str(key).lower().split()) not in allowed for key in keys):
            raise ValueError('parity baseline has an unaudited custom command chord')
        for key in keys:
            chord = ''.join(str(key).lower().split())
            if binding.get('type') != allowed[chord] or \
                    binding.get('command') != 'exec "${SHELL:-sh}"':
                raise ValueError('parity baseline has an unaudited custom command entry')
    # Sound otherwise spawns real audio players and uses shared /tmp.
    sound = parsed.get('ui', {}).get('sound')
    if sound is not None:
        if not isinstance(sound, dict):
            raise ValueError('unaudited sound configuration shape')
        headers = list(re.finditer(r'(?m)^\s*\[\s*ui\.sound\s*\]\s*(?:#.*)?$', text))
        if len(headers) != 1:
            raise ValueError('use a standard [ui.sound] table for parity runs')
        begin = headers[0].end()
        following = re.search(r'(?m)^\s*\[', text[begin:])
        end = begin + following.start() if following else len(text)
        body = re.sub(r'(?m)^\s*enabled\s*=.*$', '', text[begin:end])
        text = text[:begin] + body + '\nenabled = false\n' + text[end:]
    else:
        text += '\n[ui.sound]\nenabled = false\n'
    if 'update' in parsed:
        headers = list(re.finditer(r'(?m)^\s*\[\s*["\']?update["\']?\s*\]\s*(?:#.*)?$', text))
        if len(headers) != 1:
            raise ValueError('use a standard [update] table in the parity baseline')
        begin = headers[0].end()
        following = re.search(r'(?m)^\s*\[', text[begin:])
        end = begin + following.start() if following else len(text)
        body = text[begin:end]
        for key in ['version_check', 'manifest_check']:
            pattern = rf'(?m)^\s*["\']?{key}["\']?\s*=\s*(?:true|false)\s*(?:#.*)?$'
            body = re.sub(pattern, '', body)
            body += f'\n{key} = false\n'
        text = text[:begin] + body + text[end:]
    else:
        text += '\n[update]\nversion_check = false\nmanifest_check = false\n'
    checked = tomllib.loads(text)
    assert checked['update']['version_check'] is False
    assert checked['update']['manifest_check'] is False
    target = Path(env['XDG_CONFIG_HOME']) / 'herdr/config.toml'
    target.parent.mkdir()
    target.write_text(text)
    target.chmod(0o600)


class Owned:
    def __init__(self, binary, original_config, out, kind):
        self.binary = Path(binary).resolve(strict=True)
        self.original_config = original_config
        self.config_hash = digest(original_config)
        self.root = Path(tempfile.mkdtemp(prefix='hctui-parity-'))
        self.session = 'hctui-test-' + uuid.uuid4().hex[:12]
        self.env = scratch_environment(self.root)
        self.cwd = self.root / 'acme-shop'
        self.cwd.mkdir()
        self.out = out
        self.kind = kind
        self.peer_pid = None
        self.child = None
        self.ui = None
        self.closed = False
        try:
            copy_config(original_config, self.env)
        except BaseException:
            shutil.rmtree(self.root)
            raise

    def recovery(self):
        (self.out / (self.kind + '-recovery.json')).write_text(json.dumps({
            'root': str(self.root), 'session': self.session, 'binary': str(self.binary),
            'peer_pid': self.peer_pid, 'env': self.env,
        }, indent=2) + '\n')

    def original_unchanged(self):
        assert digest(self.original_config) == self.config_hash, 'original config changed'

    def execute(self, args):
        return subprocess.run([str(self.binary), *args], env=self.env, cwd=self.cwd,
                              stdin=subprocess.DEVNULL, capture_output=True, timeout=20)


class Core(Owned):
    def __init__(self, binary, config, out):
        super().__init__(binary, config, out, 'core')
        self.env['HC_SESSION'] = self.session
        state = self.root / 's/herdr-core' / self.session
        state.mkdir(parents=True)
        (state / 'core.json').write_text(json.dumps({'id': 'host-a-scratch', 'name': 'host-a'}))
        self.endpoint = self.root / 'r/herdr-core' / (self.session + '.sock')
        self.env['HC_SOCKET'] = str(self.endpoint)
        (self.root / 'bin/hc').symlink_to(self.binary)

    def start(self):
        self.recovery()
        with (self.out / 'core-serve.log').open('ab') as log:
            self.child = subprocess.Popen([str(self.binary), 'serve'], env=self.env,
                cwd=self.cwd, stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                start_new_session=True)
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if self.child.poll() is not None:
                raise RuntimeError('owned scratch core exited')
            try:
                self.rpc('terminal.list', {})
                return
            except (OSError, ValueError):
                time.sleep(.03)
        raise TimeoutError('owned scratch core readiness')

    def rpc(self, method, params):
        assert self.session.startswith('hctui-test-')
        with socket.socket(socket.AF_UNIX) as stream:
            stream.settimeout(5)
            stream.connect(str(self.endpoint))
            with stream.makefile('rwb', buffering=0) as wire:
                for ident, name, args in [(0, 'hello', {'generation': 1}), (1, method, params)]:
                    wire.write(json.dumps({'jsonrpc': '2.0', 'id': ident,
                                           'method': name, 'params': args}).encode() + b'\n')
                    result = json.loads(wire.readline())
                    if 'error' in result:
                        raise ValueError(result['error'])
                return result['result']

    def close(self):
        if self.closed:
            return
        if self.ui:
            self.ui.close()
        if self.child:
            if self.child.poll() is not None:
                # Restart only this explicit scratch core to adopt its holders.
                self.start()
            result = self.execute(['--json', 'server', 'stop'])
            assert result.returncode == 0 and json.loads(result.stdout)['stopped'] is True
            self.child.wait(timeout=10)
        self.original_unchanged()
        shutil.rmtree(self.root)
        (self.out / 'core-recovery.json').unlink(missing_ok=True)
        self.closed = True


class Reference(Owned):
    def __init__(self, binary, config, out):
        super().__init__(binary, config, out, 'reference')
        self.env['HERDR_SESSION'] = self.session
        (self.root / 'bin/herdr').symlink_to(self.binary)
        self.endpoint = self.root / 'c/herdr/sessions' / self.session / 'herdr.sock'

    def ready(self):
        self.recovery()
        deadline = time.monotonic() + 15
        while True:
            assert time.monotonic() < deadline, 'owned reference socket readiness'
            assert self.ui.child.poll() is None, 'owned reference UI exited'
            try:
                with socket.socket(socket.AF_UNIX) as stream:
                    stream.connect(str(self.endpoint))
                    self.peer_pid, uid, _ = struct.unpack('3i', stream.getsockopt(
                        socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize('3i')))
                break
            except OSError as error:
                if error.errno not in (errno.ENOENT, errno.ECONNREFUSED):
                    raise
                self.ui.read(.03)
        assert uid == os.getuid()
        assert str(self.binary).encode() in Path(f'/proc/{self.peer_pid}/cmdline').read_bytes()
        self.recovery()

    def cli(self, *args):
        assert args[:2] != ('session', 'list')
        result = self.execute(['--session', self.session, *args])
        assert result.returncode == 0, 'owned reference command failed; see capture'
        if not result.stdout.strip():
            return {}
        data = json.loads(result.stdout)
        assert 'error' not in data, data
        return data['result']

    def close(self):
        if self.closed:
            return
        if self.ui:
            self.ui.close()
        if self.endpoint.exists():
            self.cli('server', 'stop')
        if self.peer_pid:
            deadline = time.monotonic() + 15
            while Path(f'/proc/{self.peer_pid}').exists() and time.monotonic() < deadline:
                time.sleep(.03)
            assert not Path(f'/proc/{self.peer_pid}').exists(), 'owned reference server not reaped'
        self.original_unchanged()
        shutil.rmtree(self.root)
        (self.out / 'reference-recovery.json').unlink(missing_ok=True)
        self.closed = True
