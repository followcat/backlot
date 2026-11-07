"""Independent readiness oracle guards for multi-tab chronology captures."""
import unittest
from unittest.mock import patch
from types import SimpleNamespace
from pathlib import Path
import tempfile
import json
from chronology import agent_rows
from checks import STATUS


def row(line, state=None):
    cells = [{'text': ch, 'resolved_fg': [108, 112, 134]} for ch in line]
    if state:
        symbol, color = STATUS[state]
        index = line.index(symbol)
        cells[index]['resolved_fg'] = color
    return {'cells': cells}


class AgentSection(unittest.TestCase):
    def test_mobile_detail_separators_cannot_replace_primary_status(self):
        frame = {'cells': [row(' agents'), row('   ● Local · work', 'done'),
                           row('   beta · done · fixture'),
                           row('   ○ Local · work', 'idle'),
                           row('   alpha · idle · fixture'), row(' spaces'),
                           row(' alpha beta gamma')]}
        self.assertEqual(agent_rows(frame, True), [('beta', 'done'), ('alpha', 'idle')])

    def test_desktop_uses_sidebar_tab_labels_not_program_text(self):
        frame = {'cells': [row(' agents       priority│'),
                           row(' ○ work · gamma       │fixture alpha', 'idle'),
                           row('   fixture            │beta'),
                           row(' ● work · beta        │gamma', 'working')]}
        self.assertEqual(agent_rows(frame, False), [('gamma', 'idle'), ('beta', 'working')])

    def test_absent_section_cannot_satisfy_readiness(self):
        self.assertEqual(agent_rows({'cells': [row('alpha beta gamma')]}, True), [])


class FailureEvidence(unittest.TestCase):
    def test_exception_records_explicit_failure_before_reraising(self):
        from chronology import run
        with tempfile.TemporaryDirectory(prefix='hctui-chronology-evidence-') as root:
            args = SimpleNamespace(out=Path(root), hc='fixture', config='fixture')
            with patch('chronology.Core', side_effect=AssertionError('fixture failure')):
                with self.assertRaisesRegex(AssertionError, 'fixture failure'):
                    run(args, (40, 24), 'fixture')
            result = json.loads((Path(root) / '40x24/result.json').read_text())
            self.assertIs(result['passed'], False)
            self.assertIn('fixture failure', result['error'])


class AliasReportWords(unittest.TestCase):
    def test_report_sends_alias_and_named_osc_words(self):
        import json, tempfile
        from pathlib import Path
        from fixtures import Program
        with tempfile.TemporaryDirectory(prefix='hctui-alias-') as root:
            program = Program(SimpleNamespace(root=Path(root)), 'alpha')
            program.report('done')
            raw = bytes.fromhex(json.loads(program.request.read_text())['hex'])
            self.assertIn(b'state=done:', raw)
            self.assertNotIn(b'id=', raw)
            program.report('idle', 'build/test')
            raw = bytes.fromhex(json.loads(program.request.read_text())['hex'])
            self.assertIn(b'id=build/test:state=idle:', raw)
            self.assertTrue(raw.startswith(b'\x1b]7501;') and raw.endswith(b'\x1b\\'))


class PositionedTailReadiness(unittest.TestCase):
    def test_waits_for_complete_delayed_tail(self):
        from vectors import wait_tail
        release = '1b5b39373b313a3375'
        expected = [[{'synthesized_releases_hex': [release]}]]
        program = SimpleNamespace(received=unittest.mock.Mock(
            side_effect=[b'', bytes.fromhex(release)] + [bytes.fromhex(release)] * 30))
        owner = SimpleNamespace(ui=SimpleNamespace(read=lambda _: None))
        self.assertEqual(wait_tail(owner, [program], [0], expected), [release])

    def test_incomplete_or_extra_tail_cannot_pass(self):
        from vectors import wait_tail
        owner = SimpleNamespace(ui=SimpleNamespace(read=lambda _: None))
        for value in [b'', b'ab']:
            with self.assertRaisesRegex(AssertionError, 'detach tail'):
                wait_tail(owner, [SimpleNamespace(received=lambda: value)], [0],
                          [[{'bytes_hex': b'a'.hex()}]], timeout=0)


if __name__ == '__main__':
    unittest.main()
