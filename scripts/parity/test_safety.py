"""Executable configuration must be rejected before a scratch client starts."""
from pathlib import Path
import tempfile
import unittest
from sessions import copy_config


class ConfigSafety(unittest.TestCase):
    def check_rejected(self, contents):
        with tempfile.TemporaryDirectory(prefix='hctui-config-safety-') as directory:
            root = Path(directory)
            config = root / 'original.toml'
            config.write_text(contents)
            with self.assertRaises(ValueError):
                copy_config(config, {'XDG_CONFIG_HOME': str(root / 'copy')})
            self.assertEqual(config.read_text(), contents)

    def test_automatic_tab_status_command_is_rejected(self):
        self.check_rejected('[ui]\ntab_bar_right=[{type="command",command="fixture"}]\n')

    def test_direct_scripted_prefix_command_is_rejected(self):
        self.check_rejected('[[keys.command]]\nkey="ctrl+b"\ncommand="fixture"\n')

    def test_unrelated_but_unaudited_command_is_rejected(self):
        self.check_rejected('[[keys.command]]\nkey="ctrl+x"\ncommand="fixture"\n')



class AttachRequestCapture(unittest.TestCase):
    def test_fragmented_framed_request_records_passive_recovery(self):
        import json, struct
        from transport_proxy import Requests
        seen=[];parser=Requests(seen.append)
        params={'terminal':'t1','resumed':True,'observe':False}
        data=json.dumps({'method':'hello','params':{'framed':True}}).encode()+b'\n'
        payload=json.dumps({'id':2,'method':'terminal.attach','params':params}).encode()
        data+=struct.pack('<IBI',0,0,len(payload))+payload
        for byte in data:parser.feed(bytes([byte]))
        self.assertEqual(seen,[params])
        parser.feed(b'not another request')
        self.assertEqual(seen,[params])

    def test_request_attempt_is_not_success_until_matching_attach_reply(self):
        import json, struct
        from transport_proxy import Responses
        from types import SimpleNamespace
        seen=[];parser=Responses(SimpleNamespace(attach_id=2),lambda *event: seen.append(event))
        parser.feed(b'{"result":{"framed":true}}\n')
        def reply(value):
            payload=json.dumps(value).encode()
            return struct.pack('<IBI',0,0,len(payload))+payload
        parser.feed(reply({'id':1,'result':{'channel':3}}))
        parser.feed(reply({'id':2,'error':{'code':'unavailable'}}))
        self.assertEqual(seen,[])
        parser.feed(reply({'id':2,'result':{'channel':7}}))
        self.assertEqual(seen,[('attached',7)])

    def test_plain_watcher_requests_are_forwarded_without_frame_parsing(self):
        from transport_proxy import Requests
        seen=[];parser=Requests(seen.append)
        parser.feed(b'{"method":"hello"}\n{"method":"terminal.list"}\n')
        self.assertTrue(parser.done)
        self.assertEqual(seen,[])

    def test_oversized_request_is_rejected_before_buffering_payload(self):
        import struct
        from transport_proxy import Requests
        parser=Requests(lambda _: self.fail('unexpected request'))
        with self.assertRaises(AssertionError):
            parser.feed(b'{"method":"hello","params":{"framed":true}}\n'+struct.pack('<IBI',0,0,(16<<20)+1))


class ReloadSafety(unittest.TestCase):
    def test_edits_preserve_sound_update_and_inert_command_guards(self):
        import tomllib
        from configuration import update
        original = '[keys]\nprefix="ctrl+b"\n[[keys.command]]\nkey="alt+f"\ncommand="fixture"\n[ui.sound]\nenabled=false\n[update]\nversion_check=false\nmanifest_check=false\n'
        edited = update(original, {'keys': {'prefix': 'ctrl+t', 'help': 'prefix+shift+k'},
                                   'theme': {'name': 'nord', 'auto_switch': False}})
        self.assertEqual(tomllib.loads(edited)['keys']['command'], tomllib.loads(original)['keys']['command'])
        self.assertEqual(tomllib.loads(edited)['keys']['prefix'], 'ctrl+t')

    def test_commented_headers_and_multiline_prefix_arrays_remain_valid(self):
        import tomllib
        from configuration import update
        original = '[keys] # shortcuts\nprefix=[\n"ctrl+b",\n"ctrl+a"\n]\n[[keys.command]]\nkey="alt+f"\ncommand="fixture 中"\n[ui.sound]\nenabled=false\n[update]\nversion_check=false\nmanifest_check=false\n'
        edited = update(original, {'keys': {'prefix': 'ctrl+t'}})
        result = tomllib.loads(edited)
        self.assertEqual(result['keys']['prefix'], 'ctrl+t')
        self.assertEqual(result['keys']['command'], tomllib.loads(original)['keys']['command'])

    def test_del_in_values_and_quoted_keys_is_escaped_for_toml(self):
        import tomllib
        from configuration import update
        original = '[keys]\n"field\\u007f"="guard"\n[[keys.command]]\nkey="alt+f"\ncommand="fixture\\u007f"\n[ui.sound]\nenabled=false\n[update]\nversion_check=false\nmanifest_check=false\n'
        edited = update(original, {'keys': {'prefix': 'ctrl+t'}})
        result = tomllib.loads(edited)
        self.assertNotIn(chr(127), edited)
        self.assertEqual(result['keys']['field'+chr(127)], 'guard')
        self.assertEqual(result['keys']['command'], tomllib.loads(original)['keys']['command'])


class SoundSafety(unittest.TestCase):
    def test_sound_is_disabled_and_tmpdir_is_owned(self):
        import tomllib
        from sessions import scratch_environment
        with tempfile.TemporaryDirectory(prefix='hctui-audio-safety-') as directory:
            root = Path(directory)
            config = root / 'original.toml'
            config.write_text('onboarding=false\n')
            env = scratch_environment(root)
            self.assertEqual(env['HERDR_DISABLE_SOUND'], '1')
            self.assertTrue(Path(env['TMPDIR']).is_relative_to(root))
            copy_config(config, env)
            copied = tomllib.loads((Path(env['XDG_CONFIG_HOME'])/'herdr/config.toml').read_text())
            self.assertIs(copied['ui']['sound']['enabled'], False)

class HostProbeOrder(unittest.TestCase):
    def test_primary_da_does_not_overtake_mode_and_keyboard_replies(self):
        from unittest.mock import patch
        from pty_host import Pty
        host = Pty.__new__(Pty)
        host.master = 99
        host._tail = b''
        host.cell_px = (10, 20)
        host.keyboard_flags_reply = 7
        host.sgr_pixel_mouse_reply = 2
        with patch('pty_host.os.write') as write:
            host._answer_queries(b'\x1b[?1016$p\x1b[?u\x1b[c')
            write.assert_called_once_with(99, b'\x1b[?1016;2$y\x1b[?7u\x1b[?62;22c')


class TeardownTails(unittest.TestCase):
    def test_only_named_synthesized_drain_can_be_unordered(self):
        from tails import teardown_equivalent
        a, b, focus = b'\x1b[97;1:3u', b'\x1b[98;5:3u', b'\x1b[O'
        spec = [[{'synthesized_releases_hex': [a.hex(), b.hex()]}, {'bytes_hex': focus.hex()}]]
        same = lambda x, y: teardown_equivalent([x.hex()], [y.hex()], spec)
        self.assertTrue(same(a + b + focus, b + a + focus))
        self.assertFalse(same(a + b + focus, focus + a + b))
        self.assertFalse(same(a + b + focus, a + focus + b))
        self.assertFalse(same(a + b + focus, a + a + focus))
        self.assertFalse(teardown_equivalent([(a+b).hex()], [(b+a).hex()]))
        self.assertFalse(same(a + b + focus, a + b + focus + a))


class Readiness(unittest.TestCase):
    def test_named_checkpoints_reject_stale_or_wrong_state(self):
        from checks import ready, verify_restored, STATUS
        import copy
        cells = [{'text': t, 'resolved_fg': [0, 0, 0]} for t in ' ● alpha']
        cells[1]['resolved_fg'] = STATUS['working'][1]
        frame = {'cells': [{'cells': cells}], 'cols': 80, 'rows': 24,
                 'cursor': {'shape': 'Bar', 'blink': True, 'visible': True},
                 'host_modes': {'all_mouse': True, 'sgr_mouse': True}}
        self.assertTrue(ready(frame, 'status-0-working', None, b'', {'status': {'alpha': 'working'}}))
        self.assertFalse(ready(frame, 'status-0-blocked', None, b'', {'status': {'alpha': 'blocked'}}))
        self.assertFalse(ready(frame, 'help-page', frame, b''))
        self.assertTrue(ready(frame, 'live-cursor-and-mouse-mode', None, b''))
        wrong = copy.deepcopy(frame); wrong['cursor']['blink'] = False
        self.assertFalse(ready(wrong, 'live-cursor-and-mouse-mode', None, b''))
        self.assertFalse(ready(frame, 'resize-1', None, b'', {'geometry': (120, 40), 'grid_marker': 'GRID 87x39 1'}))
        expected = {'geometry': (80, 24), 'grid_marker': 'GRID 47x23 1'}
        self.assertFalse(ready(frame, 'resize-1', None, b'', expected))
        painted = copy.deepcopy(frame)
        painted['cells'] = [{'cells': [{'text': ch} for ch in 'GRID 47x23 0']}]
        self.assertFalse(ready(painted, 'resize-1', None, b'', expected))
        painted['cells'] = [{'cells': [{'text': ch} for ch in 'GRID 47x23 1']}]
        self.assertTrue(ready(painted, 'resize-1', None, b'', expected))
        # Every omitted legacy mouse mode and MOK must fail restoration.
        names = ['alternate', 'paste', 'focus', 'all_mouse', 'sgr_mouse', 'pixel_mouse',
                 'scheme', 'x10_mouse', 'normal_mouse', 'button_mouse', 'utf8_mouse',
                 'urxvt_mouse', 'modify_other_keys', 'kitty']
        clean = {'host_modes': dict.fromkeys(names, False),
                 'cursor': {'visible': True, 'shape': 'Block', 'blink': False}}
        clean['host_modes']['line_wrap'] = True
        verify_restored(clean)
        for name in ['x10_mouse', 'normal_mouse', 'button_mouse', 'utf8_mouse',
                     'urxvt_mouse', 'modify_other_keys']:
            dirty = copy.deepcopy(clean); dirty['host_modes'][name] = True
            with self.assertRaises(AssertionError): verify_restored(dirty)
        for key, value in [('shape', 'Bar'), ('blink', True)]:
            dirty = copy.deepcopy(clean); dirty['cursor'][key] = value
            with self.assertRaises(AssertionError): verify_restored(dirty)


class NarrowReadiness(unittest.TestCase):
    def test_highlight_and_terminal_mode_cannot_pass_from_the_old_picker(self):
        from checks import ready
        rows = [[{'text': ch, 'resolved_bg': [49, 50, 68]} for ch in 'alpha'],
                [{'text': ch, 'resolved_bg': [30, 30, 46]} for ch in 'beta'],
                [{'text': ch, 'resolved_bg': [40, 44, 52]} for ch in 'fixture beta NAVIGATE']]
        frame = {'cols': 65, 'cells': [{'cells': row} for row in rows], 'host_modes': {'kitty': 31}}
        self.assertFalse(ready(frame, 'narrow-next', None, b'', {'highlight': 'beta'}))
        self.assertFalse(ready(frame, 'narrow-beta', None, b'', {'terminal': 'fixture beta'}))
        for cell in rows[1]: cell['resolved_bg'] = [49, 50, 68]
        self.assertTrue(ready(frame, 'narrow-next', None, b'', {'highlight': 'beta'}))


class RepositoryFixture(unittest.TestCase):
    def test_local_divergence_worktree_and_subdirectory_are_real(self):
        import subprocess
        from types import SimpleNamespace
        from repository import prepare
        from sessions import scratch_environment
        with tempfile.TemporaryDirectory(prefix='hctui-parity-git-') as directory:
            root = Path(directory)
            cwd = root / 'acme-shop'; cwd.mkdir()
            owner = SimpleNamespace(root=root, cwd=cwd, env=scratch_environment(root))
            paths = prepare(owner)
            def git(path, *args):
                return subprocess.check_output(['/usr/bin/git', '-C', str(path), *args],
                                               env=owner.env, text=True).strip()
            self.assertEqual(git(paths['alpha'], 'rev-list', '--left-right', '--count',
                                 'HEAD...@{upstream}').split(), ['2', '1'])
            self.assertEqual(Path(git(paths['alpha'], 'rev-parse', '--show-toplevel')), cwd)
            self.assertEqual(git(paths['beta'], 'branch', '--show-current'), 'fixture-linked')
            self.assertEqual(git(cwd, 'rev-parse', '--path-format=absolute', '--git-common-dir'),
                             git(paths['beta'], 'rev-parse', '--path-format=absolute', '--git-common-dir'))


class LayoutBindingSafety(unittest.TestCase):
    def test_layout_bindings_use_user_prefix_and_reject_command_chord(self):
        from layout import binding, chord
        self.assertEqual(binding({'keys':{'prefix':'ctrl+a'}},'split_vertical'),b'\x01v')
        self.assertEqual(binding({'keys':{'new_tab':'alt+c'}},'new_tab'),b'\x1bc')
        self.assertEqual(binding({},'rename_tab'),b'\x02T')
        self.assertEqual(binding({},'cycle_pane_previous'),b'\x02\x1b[Z')
        with self.assertRaises(AssertionError): chord('alt+f')
        with self.assertRaises(AssertionError): chord('super+c')


class FixtureIncarnationSafety(unittest.TestCase):
    def test_reinstall_resets_control_but_preserves_input_evidence(self):
        from types import SimpleNamespace
        from layout import install
        with tempfile.TemporaryDirectory(prefix='hctui-fixture-safety-') as directory:
            root=Path(directory)
            request=root/'gamma-request.json';ready=root/'gamma-ready';log=root/'gamma-input.bin'
            request.write_text('old request');ready.write_text('old ready');log.write_bytes(b'earlier input')
            def rpc(method,params):
                self.assertEqual(method,'terminal.send')
                self.assertFalse(request.exists());self.assertFalse(ready.exists())
                self.assertEqual(log.read_bytes(),b'earlier input')
                ready.write_text('initial')
            install(SimpleNamespace(root=root,kind='core',rpc=rpc,ui=None),'gamma','t4')
            self.assertEqual(log.read_bytes(),b'earlier input')


class FixtureResizeSafety(unittest.TestCase):
    def test_rapid_resize_keeps_control_thread_and_latest_grid_alive(self):
        import json,time
        from types import SimpleNamespace
        from fixtures import Program,wait_program
        from pty_host import Pty
        from sessions import scratch_environment
        with tempfile.TemporaryDirectory(prefix='hctui-resize-safety-') as directory:
            root=Path(directory);owner=SimpleNamespace(root=root,kind='fixture',ui=None)
            program=Program(owner,'alpha')
            owner.ui=Pty(program.argv,scratch_environment(root),root,80,24)
            try:
                wait_program(owner,program)
                for index in range(400):owner.ui.resize(40+index%30,20+index%7)
                owner.ui.resize(65,24)
                program.send(b'fixture resize alive',grid=(65,24))
                wait_program(owner,program,program.serial)
                self.assertIsNone(owner.ui.child.poll())
                self.assertEqual(json.loads(Path(str(program.log)+'.grid').read_text()),[65,24])
            finally:owner.ui.close()


class GeometryPublicationSafety(unittest.TestCase):
    def test_one_snapshot_is_published_for_aba_and_ack_waits_for_publication(self):
        import json,os
        from types import SimpleNamespace
        from fixtures import GEOMETRY
        with tempfile.TemporaryDirectory(prefix='hctui-grid-safety-') as directory:
            samples=iter([(80,24),(40,20),(80,24),(40,20),(40,20),(40,20)])
            namespace={'json':json,'os':SimpleNamespace(get_terminal_size=lambda _:next(samples),replace=os.replace)}
            exec(GEOMETRY,namespace)
            path=Path(directory)/'grid.json';geometry=namespace['Geometry'](str(path))
            for expected in [(80,24),(40,20),(80,24)]:
                self.assertEqual(geometry.sample(),expected)
                self.assertEqual(geometry.published,expected)
                self.assertEqual(json.loads(path.read_text()),list(expected))
            self.assertFalse(geometry.ready([40,20]))
            geometry.sample();self.assertTrue(geometry.ready([40,20]))


class ReferenceReadinessSafety(unittest.TestCase):
    def test_bound_socket_is_retried_until_listening_and_peer_is_verified(self):
        import os,struct
        from types import SimpleNamespace
        from unittest.mock import Mock,patch
        from sessions import Reference
        calls=[]
        class Socket:
            def __enter__(self):return self
            def __exit__(self,*_):pass
            def connect(self,path):
                calls.append(path)
                if len(calls)==1:raise ConnectionRefusedError(111,'fixture not listening yet')
            def getsockopt(self,*_):return struct.pack('3i',123,os.getuid(),0)
        owner=SimpleNamespace(recovery=Mock(),endpoint=Path('/tmp/owned-fixture.sock'),binary=Path('/usr/bin/herdr-fixture'),ui=SimpleNamespace(child=SimpleNamespace(poll=lambda:None),read=Mock()))
        with patch('sessions.socket.socket',return_value=Socket()),patch('sessions.Path.read_bytes',return_value=b'/usr/bin/herdr-fixture'):
            Reference.ready(owner)
        self.assertEqual(len(calls),2);self.assertEqual(owner.peer_pid,123)
        owner.ui.read.assert_called_once_with(.03)


if __name__ == '__main__':
    unittest.main()
