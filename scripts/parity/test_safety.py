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
    def test_only_synthesized_release_order_can_differ(self):
        from vectors import teardown_equivalent
        a, b = b'\x1b[97;1:3u', b'\x1b[98;5:3u'
        same = lambda x, y: teardown_equivalent([x.hex()], [y.hex()])
        self.assertTrue(same(a + b, b + a))
        self.assertFalse(same(a + a, a))
        self.assertFalse(same(a, b))
        self.assertFalse(same(a + b'\x1b[O', a))
        self.assertFalse(same(b'a', b'b'))


if __name__ == '__main__':
    unittest.main()
