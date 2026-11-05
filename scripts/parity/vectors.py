#!/usr/bin/env python3
"""Replay immutable reference input bytes through owned real-core PTYs."""
import argparse
import json
from pathlib import Path

from fixtures import prepare_core, prepare_reference, wait_program
from pty_host import Pty
from run import REFERENCE, REPO, detach
from sessions import Core, Reference, digest

FIXTURES = REPO / 'crates/hc-tui/tests/fixtures'


def teardown_equivalent(source, candidate):
    """Only synthesized CSI-u release order is unordered in the source.

    Its server drains a HashMap. Keep duplicates, exact release contents and
    every other byte (including focus) significant; ordinary cases stay exact.
    """
    import re
    from collections import Counter
    release = re.compile(rb'\x1b\[[0-9:;]*:3u')
    def parts(value):
        data = bytes.fromhex(value)
        return Counter(release.findall(data)), release.sub(b'', data)
    return len(source) == len(candidate) and all(parts(a) == parts(b) for a, b in zip(source, candidate))


def mouse_config(core):
    """Override only the two neutral values recorded by the mouse oracle."""
    import re
    path = Path(core.env['XDG_CONFIG_HOME']) / 'herdr/config.toml'
    text = path.read_text()
    if not re.search(r'(?m)^\[ui\]\s*$', text):
        text += '\n[ui]\n'
    start = re.search(r'(?m)^\[ui\]\s*$', text).end()
    following = re.search(r'(?m)^\s*\[', text[start:])
    end = start + following.start() if following else len(text)
    body = text[start:end]
    for key, value in [('right_click_passthrough_modifier', '"ctrl"'), ('mouse_scroll_lines', '3')]:
        body = re.sub(rf'(?m)^\s*{key}\s*=.*$', '', body)
        body += f'\n{key} = {value}\n'
    path.write_text(text[:start] + body + text[end:])


def replay(args, fixture, reference=False):
    expected = json.loads(fixture.read_text())
    assert expected['source_revision'] == REFERENCE
    out = args.out / fixture.stem / ("reference" if reference else "candidate")
    out.mkdir(parents=True)
    core = Reference(args.reference_binary, args.config, out) if reference else Core(args.hc, args.config, out)
    result = {'fixture_sha256': digest(fixture), 'cases': []}
    try:
        if 'groups' in expected:
            mouse_config(core)
        if reference:
            core.ui = Pty([str(args.reference_binary), '--session', core.session], core.env,
                          core.cwd, 120, 40, cell_px=expected.get('host_cell_px', (10,20)))
            core.ui.keyboard_flags_reply = expected.get('host_keyboard_flags_reply', 0)
            core.ui.sgr_pixel_mouse_reply = expected.get('host_sgr_pixel_mouse_reply', 0)
            core.ready()
            programs = prepare_reference(core)
        else:
            core.start()
            programs = prepare_core(core)
        setup = (b'\x1b[?2004h' if expected.get('inner_bracketed_paste') else b'')
        flags = expected.get('inner_kitty_flags', 31 if 'pane_a_hex' in expected['cases'][0] else 0) if 'cases' in expected else 0
        setup += f'\x1b[={flags}u'.encode()
        if expected.get('inner_focus_reporting'):
            setup += b'\x1b[?1004h'
        for program in programs:
            program.send(setup)
            wait_program(core, program, program.serial)
        if not reference:
            core.ui = Pty([str(args.hc_tui), '--session', core.session], core.env,
                          core.cwd, 120, 40, cell_px=expected.get('host_cell_px', (10, 20)))
        core.ui.keyboard_flags_reply = expected.get('host_keyboard_flags_reply', 0)
        core.ui.sgr_pixel_mouse_reply = expected.get('host_sgr_pixel_mouse_reply', 0)
        core.ui.until(lambda data: b'fixture alpha' in data)
        core.ui.read(.3)
        offsets = [len(program.received()) for program in programs]

        def case(case, mouse=False):
            action = expected.get('before_case', {})
            if action.get('name') == case['name']:
                assert action['fixture_action'] == 'both-programs-set-kitty-flags'
                for program in programs:
                    program.send(bytes.fromhex(action['sequence_hex']))
                    wait_program(core, program, program.serial)
                core.ui.read(.25)
            core.ui.send(bytes.fromhex(case['outer_hex']))
            core.ui.read(.3)
            data = [program.received() for program in programs]
            actual = [data[i][offsets[i]:].hex() for i in range(2)]
            offsets[:] = [len(value) for value in data]
            if 'pane_a_hex' in case:
                wanted = [case['pane_a_hex'], case['pane_b_hex']]
            else:
                wanted = [case['program_hex' if mouse else 'received_hex'], '']
            result['cases'].append({'name': case['name'], 'actual': actual,
                                    'expected': wanted, 'match': actual == wanted})

        if 'groups' in expected:
            for group in expected['groups']:
                # Mode-only updates need a visible paint in the pinned source
                # before its thin-client surface publishes the new mouse mode.
                # This is fixture output, never input or an expected-byte edit.
                programs[0].send(bytes.fromhex(group['setup_sequence_hex']) +
                    b'\x1b[16;2HPHASE mouse\x1b[K\x1b[18;8H')
                wait_program(core, programs[0], programs[0].serial)
                core.ui.read(.25)
                desired = group.get('host_pixel_mode', False)
                import time
                deadline = time.monotonic() + 5
                while True:
                    data = core.ui.output
                    actual = data.rfind(b'\x1b[?1016h') > data.rfind(b'\x1b[?1016l')
                    if actual == desired:
                        break
                    assert time.monotonic() < deadline, 'fixture host mouse mode not ready'
                    core.ui.read(.05)
                for item in group['cases']:
                    case(item, mouse=True)
        else:
            for item in expected['cases']:
                case(item)
        detach(core)
        core.ui.read(.1)
        result['tail_hex'] = [program.received()[offsets[i]:].hex() for i, program in enumerate(programs)]
        result['termios_restored'] = True
    except BaseException as error:
        result['error'] = repr(error)
        raise
    finally:
        try:
            if core.ui:
                (out / 'outer.raw').write_bytes(core.ui.output)
            core.close()
            result['cleanup'] = 'pass'
        finally:
            (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    failed = sum(not case['match'] for case in result['cases'])
    print(json.dumps({'fixture': fixture.name, 'reference':reference, 'total':len(result['cases']), 'failed':failed}), flush=True)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['hc', 'hc-tui', 'config', 'out']:
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--reference-manifest', type=Path)
    parser.add_argument('--fixture', choices=[p.name for p in FIXTURES.glob('reference-*.json')], action='append')
    args = parser.parse_args()
    for name in ['hc', 'hc_tui', 'config']:
        setattr(args, name, getattr(args, name).resolve(strict=True))
    args.reference_binary = None
    if args.reference_manifest:
        manifest = json.loads(args.reference_manifest.resolve(strict=True).read_text())
        assert manifest['source_revision'] == REFERENCE
        args.reference_binary = Path(manifest['binary']).resolve(strict=True)
        assert digest(args.reference_binary) == manifest['sha256']
    args.out = args.out.resolve()
    if args.out.is_relative_to(REPO):
        raise ValueError('captures must be outside the public repository')
    args.out.mkdir(parents=True, exist_ok=False)
    (args.out / 'provenance.json').write_text(json.dumps({
        'candidate_sha256': digest(args.hc_tui), 'core_sha256': digest(args.hc),
        'config_sha256': digest(args.config), 'reference_revision': REFERENCE,
    }, indent=2) + '\n')
    selected = args.fixture or sorted(p.name for p in FIXTURES.glob('reference-*.json'))
    failures = 0
    for name in selected:
        source = replay(args, FIXTURES/name, reference=True) if args.reference_binary else None
        candidate = replay(args, FIXTURES/name)
        failures += sum(not case['match'] for case in candidate['cases'])
        if source:
            failures += sum(not case['match'] for case in source['cases'])
            tail_match = teardown_equivalent(source['tail_hex'], candidate['tail_hex'])
            failures += not tail_match
            (args.out/name.replace('.json','')/'tail-comparison.json').write_text(json.dumps({
                'reference':source['tail_hex'],'candidate':candidate['tail_hex'],'match':tail_match},indent=2)+'\n')
            print(json.dumps({'fixture':name,'detach_tail_match':tail_match}),flush=True)
    raise SystemExit(bool(failures))


if __name__ == '__main__':
    main()
