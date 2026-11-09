#!/usr/bin/env python3
"""Run both clients through owned scratch PTYs and compare native cells."""
import argparse
import json
from pathlib import Path
import subprocess
import time
import tomllib

from checks import last_osc52, ready, title_difference, verify_restored

from fixtures import prepare_core, prepare_reference, wait_program
from sessions import Core, Reference, digest
from repository import CASES as REPOSITORY_CASES
from pty_host import Pty

REFERENCE = 'd00953e081389eb5f8ea006896fbf4a17b2f9dfc'
REPO = Path(__file__).resolve().parents[2]


def checkpoint(owner, name, size, decoder):
    owner.ui.read(.25)
    quiet_deadline = time.monotonic() + .75
    while True:
        activity = owner.ui.read(.05)
        owner.capture_quiet = not activity
        if owner.capture_quiet or time.monotonic() >= quiet_deadline:
            break
    capture = owner.out / (owner.kind + '-' + name + '.raw')
    capture.write_bytes(owner.ui.output)  # Complete stream BEFORE detach.
    timeline = capture.with_suffix('.resize.json')
    timeline.write_text(json.dumps(getattr(owner, 'resizes', [])))
    initial = getattr(owner, 'initial_size', size)
    result = subprocess.run([str(decoder), str(initial[0]), str(initial[1]), str(capture), str(timeline)],
                            capture_output=True, check=True, timeout=15)
    path = capture.with_suffix('.json')
    path.write_bytes(result.stdout)
    return json.loads(result.stdout)


def differences(reference, candidate):
    differences = []
    for row_a, row_b in zip(reference['cells'], candidate['cells'], strict=True):
        for column, (a, b) in enumerate(zip(row_a['cells'], row_b['cells'], strict=True)):
            if a != b:
                differences.append({'row': row_a['y'], 'column': column, 'reference': a, 'candidate': b})
    if reference['cursor'] != candidate['cursor']:
        differences.append({'cursor_reference': reference['cursor'], 'cursor_candidate': candidate['cursor']})
    if reference['host_modes'] != candidate['host_modes']:
        differences.append({'reference_modes':reference['host_modes'], 'candidate_modes':candidate['host_modes']})
    title = title_difference(reference['title'],candidate['title'])
    if title:
        differences.append(title)
    return differences


def detach(owner, decoder=None, keys=b'\x02q'):
    owner.ui.send(keys)
    deadline = time.monotonic() + 5
    while owner.ui.child.poll() is None and time.monotonic() < deadline:
        owner.ui.read(.03)
    assert owner.ui.child.poll() == 0, 'owned UI detach did not exit successfully'
    assert owner.ui.terminal_restored(), 'owned outer terminal mode restoration failed'
    (owner.out / (owner.kind + '-teardown.raw')).write_bytes(owner.ui.output)
    if decoder:
        restored = checkpoint(owner, 'restored', owner.initial_size, decoder)
        verify_restored(restored)
        return restored


def one_size(args, size, binary):
    if args.scenario == "chronology":
        from chronology import run as run_chronology
        return run_chronology(args,size,binary)
    if args.scenario == "layout":
        from layout import run as run_layout
        return run_layout(args,size,binary)
    out = args.out / f'{size[0]}x{size[1]}'
    out.mkdir(parents=True, exist_ok=False)
    core = reference = None
    result = {'geometry': size, 'checkpoints': [], 'cleanup': {}}
    try:
        core = Core(args.hc, args.config, out)
        if args.host_program:
            core.env['TERM_PROGRAM'] = args.host_program
        repository_scenario = args.scenario == 'repository'
        if repository_scenario:
            from repository import prepare as prepare_repository
        candidate_paths = prepare_repository(core, args.repository_case) if repository_scenario else None
        core.start()
        candidate_programs = prepare_core(core, candidate_paths, labelled=not repository_scenario)
        if repository_scenario:
            from repository import verify_facts
            verify_facts(core, candidate_programs, args.repository_case)
        reference = Reference(binary, args.config, out)
        if args.host_program:
            reference.env['TERM_PROGRAM'] = args.host_program
        reference_paths = prepare_repository(reference, args.repository_case) if repository_scenario else None
        reference.ui = Pty([str(binary), '--session', reference.session], reference.env,
                           reference_paths['alpha'] if reference_paths else reference.cwd,
                           *size, cell_px=(10, 20))
        reference.ui.keyboard_flags_reply = 7
        reference.ui.sgr_pixel_mouse_reply = 2
        reference.ready()
        reference_programs = prepare_reference(reference, reference_paths, labelled=not repository_scenario)
        core.ui = Pty([str(args.hc_tui), '--session', core.session], core.env,
                      core.cwd, *size, cell_px=(10, 20))
        core.ui.keyboard_flags_reply = 7
        core.ui.sgr_pixel_mouse_reply = 2

        for owner in [reference, core]:
            owner.initial_size = size
            owner.resizes = []

        previous = {}
        def capture_ready(owner, name, program, expected):
            deadline = time.monotonic() + 5
            stable = None
            while True:
                frame = checkpoint(owner, name, size, args.decoder)
                requirements = dict(expected)
                requirements.update(expected.get(owner.kind, {}))
                if ready(frame, name, previous.get(owner.kind), program.received(), requirements):
                    # Demand a quiet output interval rather than taking a stale
                    # pair merely because both clients are equally late. The
                    # reference keeps repainting an identical copy-mode frame
                    # on its client timer; a repeated decode also counts as
                    # settled.
                    if owner.capture_quiet or stable == frame:
                        previous[owner.kind] = frame
                        return frame
                    stable = frame
                assert time.monotonic() < deadline, f'{owner.kind} {name} did not reach a settled expected checkpoint'

        def compare(name, **expected):
            a = capture_ready(reference, name, reference_programs[1], expected)
            b = capture_ready(core, name, candidate_programs[1], expected)
            diff = differences(a, b)
            (out / (name + '-diff.json')).write_text(json.dumps(diff, ensure_ascii=False, indent=2) + '\n')
            bugs = diff
            result['checkpoints'].append({'name':name, 'different_cells':len(bugs),

                'status': 'fail' if bugs else ('core-policy-difference' if expected.get('core_policy_differences') else 'pass'),
                'core_policy_differences': expected.get('core_policy_differences', [])})
            print(json.dumps({'geometry':size,'checkpoint':name,'differences':len(bugs),

                'status':'fail' if bugs else ('core-policy-difference' if expected.get('core_policy_differences') else 'pass')}),flush=True)

        if repository_scenario:
            from repository import run as run_repository
            run_repository(compare, args.repository_case, (reference, core), size,
                           tomllib.loads(args.config.read_text()))
        else:
            compare('baseline')
        result['usable_frame_seconds'] = core.ui.first_seen(b'fixture alpha')
        if repository_scenario:
            result['repository_case'] = args.repository_case
        elif not args.smoke and size[0] <= 65:
            from narrow import run as run_narrow
            run_narrow((reference, core), (reference_programs, candidate_programs), compare, size)
        elif not args.smoke:
            help_keys = b'\x02\x18' if args.scenario == 'prefix-conflict' else b'\x02?'
            sidebar_keys = b'\x18' if args.scenario == 'prefix-conflict' else b'\x02b'
            for name, keys in [
                ('prefix', b'\x02'), ('prefix-cancel', b'\x1b[27u'),
                ('help', help_keys), ('help-filter', b'/pane'),
                ('help-back', b'\x1b[27u'), ('help-page', b'\x1b[6~'),
                ('help-close', b'\x1b[27u'), ('navigate', b'\x02w'),
                ('navigate-next', b'\x1b[1;1B\x1b[1;1:3B'), ('switch', b'\r'),
                ('sidebar-drag', b'\x1b[<0;26;9M\x1b[<32;32;9M\x1b[<0;32;9m'),
                ('sidebar-collapse', sidebar_keys), ('sidebar-expand', sidebar_keys),
                # Context menu: right-press on the alpha workspace row, Down/Up
                # move the highlight, Enter activates — the reference's menu
                # key/mouse contract. ConfirmClose Escape lands in Navigate;
                # a second Escape cancels back to Terminal.
                ('menu-workspace', b'\x1b[<2;5;3M'),
                ('menu-highlight', b'\x1b[B'),
                ('menu-highlight-up', b'\x1b[A'),
                ('menu-workspace-rename', b'\r'),
                ('menu-rename-cancel', b'\x1b[27u'),
                ('menu-workspace-again', b'\x1b[<2;5;3M'),
                ('menu-close-highlight', b'\x1b[B'),
                ('menu-workspace-close', b'\r'),
                ('menu-close-cancel', b'\x1b[27u'),
                ('menu-navigate-exit', b'\x1b[27u'),
                ('menu-tab', b'\x1b[<2;37;1M'),
                ('menu-tab-newtab', b'\r'),
                ('menu-newtab-cancel', b'\x1b[27u'),
                ('menu-pane', b'\x1b[<2;60;10M'),
                ('menu-pane-escape', b'\x1b[27u'),
            ]:
                reference.ui.send(keys)
                core.ui.send(keys)
                compare(name)

            for programs, owner in [(reference_programs, reference), (candidate_programs, core)]:
                for program in programs:
                    program.send(b'\x1b[5 q\x1b[?1003;1006h')
                    wait_program(owner, program, program.serial)
            compare('live-cursor-and-mouse-mode')
            for name, keys in [
                ('mouse', b'\x1b[<35;38;5M\x1b[<0;38;5M\x1b[<32;42;7M\x1b[<0;42;7m'),
                ('tracked-wheel', b'\x1b[<64;38;5M\x1b[<65;38;5M'),
            ]:
                reference.ui.send(keys)
                core.ui.send(keys)
                compare(name)
            for index in range(2):
                assert reference_programs[index].received() == candidate_programs[index].received(), 'program input differs'

            # A font/geometry change can keep the same character grid. Pixel
            # hit tests and the child's geometry must refresh together.
            for owner, programs in [(reference, reference_programs), (core, candidate_programs)]:
                owner.ui.cell_px = (20, 40)
                owner.ui.resize(*size)
                programs[1].send(b'\x1b[?1016h\x1b[16;2HPHASE pitch\x1b[K\x1b[18;8H')
                wait_program(owner, programs[1], programs[1].serial)
            compare('changed-pixel-pitch')
            raw_mouse = b'\x1b[<35;757;193M\x1b[<0;757;193M\x1b[<0;757;193m'
            for owner in [reference, core]:
                owner.ui.send(raw_mouse)
            compare('changed-pitch-mouse')
            assert reference_programs[1].received() == candidate_programs[1].received(), 'changed-pitch input differs'
            for owner, programs in [(reference, reference_programs), (core, candidate_programs)]:
                programs[1].send(b'\x1b[?1016l\x1b[16;2HPHASE restored\x1b[K\x1b[18;8H')
                wait_program(owner, programs[1], programs[1].serial)
                owner.ui.cell_px = (10, 20)
                owner.ui.resize(*size)
            compare('restored-pixel-pitch')

            # The reference owns status detection; core receives the matching
            # authoritative OSC report. No status is inferred from screen text.
            for index, state in [(0, 'working'), (1, 'working'), (0, 'blocked'),
                                 (0, 'idle'), (1, 'idle')]:
                program = reference_programs[index]
                if not getattr(program, 'metadata_set', False):
                    reference.cli('pane', 'report-metadata', program.terminal,
                        '--source', 'fixture-meta', '--agent', 'fixture',
                        '--applies-to-source', 'fixture', '--title', 'task-' + program.name,
                        '--seq', '1')
                    program.metadata_set = True
                reference.cli('pane', 'report-agent', program.terminal,
                    '--source', 'fixture', '--agent', 'fixture', '--state', state,
                    '--seq', str(100 + len(result['checkpoints'])))
                candidate_programs[index].status(state)
                wait_program(core, candidate_programs[index], candidate_programs[index].serial)
                compare('status-' + str(index) + '-' + state,
                        status={('alpha', 'beta')[index]: 'done' if state == 'idle' and index == 0 else state})

            # The Go to navigator (prefix+g): open, each status filter, a
            # search matching the fixture titles, a search with no hits,
            # selection movement and close. Esc leaves search with the
            # query kept; `a` clears it before `k` moves the selection off
            # the last row so End produces a change.
            for name, keys, expected in [
                ('navigator-open', b'\x02g', ['Go to']),
                ('navigator-blocked', b'b', ['/ blocked']),
                ('navigator-working', b'w', ['/ working']),
                ('navigator-idle', b'i', ['/ idle']),
                ('navigator-done', b'd', ['/ done']),
                ('navigator-all', b'a', ['/ search']),
                ('navigator-search', b'/task', ['/ task']),
                ('navigator-search-none', b'\x15zzz', ['No matching']),
                ('navigator-move', b'\x1b[27ua' + b'k' * 2, []),
                ('navigator-end', b'\x1b[F', []),
                ('navigator-close', b'\x1b[27u', []),
            ]:
                reference.ui.send(keys)
                core.ui.send(keys)
                compare(name, text_contains=expected)

            # Disable tracking before exercising the source's local history
            # viewport and typing-to-bottom behavior on the selected beta pane.
            scroll = (b'\x1b[?1003;1006l\x1b[0m\x1b[2J\x1b[3J\x1b[H' +
                      b''.join(f'line {n:03d} 中 é\r\n'.encode() for n in range(100)))
            for owner, programs in [(reference, reference_programs), (core, candidate_programs)]:
                programs[1].send(scroll)
                wait_program(owner, programs[1], programs[1].serial)
            compare('history-bottom')
            for name, keys in [('history-wheel', b'\x1b[<64;40;5M' * 5),
                               ('history-typing-reset', b'z')]:
                reference.ui.send(keys)
                core.ui.send(keys)
                compare(name)

            # Keyboard copy mode (prefix+[) over beta's 100-line history:
            # motions, a visual and a linewise selection copied through
            # OSC 52 (byte-compared), a literal search with N/M paging,
            # g/G history jumps, half/page scrolling, the prefix key
            # arming inside copy mode, and both exits (y/Enter, q).
            for name, keys in [
                ('copy-open', b'\x02['),
                ('copy-move', b'kkhh'),
                ('copy-word', b'we'),
                ('copy-visual', b'vjj$'),
                ('copy-yank', b'y'),
            ]:
                reference.ui.send(keys)
                core.ui.send(keys)
                compare(name)
            assert last_osc52(reference.ui.output) == last_osc52(core.ui.output), \
                'copy-mode selection bytes differ'
            # The `copied to clipboard` card lives exactly two seconds on
            # both clients while the captures run seconds apart; without a
            # settle, later checkpoints would race its expiry repaint.
            time.sleep(2.4)
            for name, keys in [
                ('copy-search-open', b'\x02[/line 09'),
                ('copy-search-submit', b'\r'),
                ('copy-search-next', b'n'),
                ('copy-search-prev', b'N'),
                ('copy-search-cancel', b'\x1b[27u'),
                ('copy-cancel', b'\x1b[27u'),
                ('copy-history', b'\x02[g'),
                ('copy-bottom', b'G'),
                ('copy-half-up', b'\x15'),
                ('copy-page-up', b'\x1b[5~'),
                ('copy-page-down', b'\x1b[6~'),
                ('copy-linewise', b'Vj'),
                ('copy-yank-line', b'\r'),
            ]:
                reference.ui.send(keys)
                core.ui.send(keys)
                compare(name)
            assert last_osc52(reference.ui.output) == last_osc52(core.ui.output), \
                'copy-mode linewise bytes differ'
            time.sleep(2.4)
            for name, keys in [
                ('copy-prefix-arm', b'\x02[\x02'),
                ('copy-prefix-cancel', b'\x1b[27u'),
                ('copy-quit', b'q'),
            ]:
                reference.ui.send(keys)
                core.ui.send(keys)
                compare(name)

            # Decode each full output stream using the actual resize offsets;
            # feeding old bytes into only the final dimensions is not valid.
            for new_size in [(120, 40), (80, 24), (200, 50), size]:
                for owner in [reference, core]:
                    owner.ui.read(.1)
                    owner.resizes.append((len(owner.ui.output), *new_size))
                    owner.ui.resize(*new_size)
                # The child acknowledges its ACTUAL PTY allocation before
                # painting a unique marker. Decoder.resize alone cannot prove
                # that either client handled the host's resize event.
                grid = (new_size[0] - 33, new_size[1] - 1)
                marker = f'GRID {grid[0]}x{grid[1]} {len(reference.resizes)}'
                for owner, programs in [(reference, reference_programs), (core, candidate_programs)]:
                    programs[1].send(('\x1b[2;2H' + marker + '\x1b[K\x1b[18;8H').encode(), grid=grid)
                    wait_program(owner, programs[1], programs[1].serial)
                compare('resize-' + str(len(reference.resizes)), geometry=new_size, grid_marker=marker)

            # keys.command: the audited Alt+F popup (80%x80% floating
            # terminal running the scratch shell) opened and typed into.
            # The reference forwards every key — including Escape — into
            # the popup terminal; the popup closes when its program exits.
            # The forwarded Escape leaves the shell's line editor holding
            # a partial escape sequence that swallows a following CR, so
            # Ctrl-U kills the pending line before `exit`. Then the Alt+P
            # pane command — a horizontal split the reference zooms over
            # the whole tab — and its shell exit.
            for name, keys, expected in [
                ('popup-open', b'\x1b[102;3u', ['popup']),
                ('popup-type', b'echo P5\r', ['P5']),
                ('popup-escape', b'\x1b[27u', []),
                ('popup-exit', b'\x15exit\r', []),
                ('popup-closed', b'', []),
                ('pane-open', b'\x1b[112;3u', []),
                ('pane-exit', b'exit\r', []),
                ('pane-closed', b'', []),
            ]:
                if keys:
                    reference.ui.send(keys)
                    core.ui.send(keys)
                compare(name, text_contains=expected)
            result['idle'] = core.ui.idle_measurement(10)


        result['first_candidate_byte_seconds'] = core.ui.first_byte_seconds
        restored_candidate = detach(core, args.decoder)
        restored_reference = detach(reference, args.decoder)
        title = title_difference(restored_reference['title'],restored_candidate['title'])
        diff = differences(restored_reference, restored_candidate)
        (out / 'restored-diff.json').write_text(json.dumps(diff, indent=2) + '\n')
        result['checkpoints'].append({'name':'restored', 'different_cells':len(diff),
                                      'status':'fail' if diff else 'pass'})
        result['restored_title_difference'] = title
        result['termios_restored'] = True
        result['passed'] = all(stage['status'] == 'pass' and stage['different_cells'] == 0
                               for stage in result['checkpoints'])
        result['status'] = 'pass' if result['passed'] else ('core-policy-difference'
            if all(stage['status'] in ('pass','core-policy-difference') for stage in result['checkpoints']) else 'fail')
    except BaseException as error:
        result['passed'] = False
        result['status'] = 'fail'
        result['error'] = repr(error)
        raise
    finally:
        failures = []
        for owner in [core, reference]:
            if owner is None:
                continue
            try:
                owner.close()
                result['cleanup'][owner.kind] = 'pass'
            except BaseException as error:
                owner.recovery()
                result['cleanup'][owner.kind] = repr(error)
                failures.append(error)
        if failures:
            result.update(passed=False, status='fail')
        (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
        if failures:
            raise RuntimeError('owned scratch cleanup failed; recovery records retained') from failures[0]
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--hc', type=Path, required=True)
    parser.add_argument('--hc-tui', type=Path, required=True)
    parser.add_argument('--reference-manifest', type=Path, required=True)
    parser.add_argument('--config', type=Path, required=True)
    parser.add_argument('--decoder', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--size', choices=['80x24', '120x40', '200x50', '65x24', '64x24', '40x24'], action='append')
    parser.add_argument('--repository-case', choices=REPOSITORY_CASES, default='standard')
    parser.add_argument('--scenario', choices=['default', 'prefix-conflict', 'repository', 'layout', 'chronology'], default='default')
    parser.add_argument('--host-program', choices=['wezterm', 'alacritty'])
    parser.add_argument('--smoke', action='store_true', help='baseline only; not an acceptance run')
    args = parser.parse_args()
    for field in ['hc', 'hc_tui', 'config', 'decoder', 'reference_manifest']:
        setattr(args, field, getattr(args, field).resolve(strict=True))
    parsed_config = tomllib.loads(args.config.read_text())
    if parsed_config.get('theme'):
        raise ValueError('status readiness requires the pinned source default-dark palette oracle')
    if parsed_config.get('ui', {}).get('status_indicators', 'dots') != 'dots':
        raise ValueError('status readiness requires the pinned dots oracle')
    args.default_title = parsed_config.get("ui",{}).get("window_title", "{hostname}: {workspace}") == "{hostname}: {workspace}"
    if args.scenario == 'repository' and not args.default_title:
        raise ValueError('repository fixture requires the default title template')
    if args.scenario == 'prefix-conflict':
        keys = parsed_config.get('keys',{})
        assert keys.get('help') == 'prefix+ctrl+x' and keys.get('toggle_sidebar') == 'ctrl+x', 'prefix-conflict requires its recorded bindings'
    args.out = args.out.resolve()
    if args.out.is_relative_to(REPO):
        raise ValueError('captures must be outside the public repository')
    args.out.mkdir(parents=True, exist_ok=False)
    info = json.loads(args.reference_manifest.read_text())
    assert info['source_revision'] == REFERENCE
    binary = Path(info['binary']).resolve(strict=True)
    assert digest(binary) == info['sha256'], 'reference binary provenance mismatch'
    metadata = {'reference_revision': REFERENCE, 'reference_sha256': digest(binary),
                'candidate_sha256': digest(args.hc_tui), 'core_sha256': digest(args.hc),
                'config_sha256': digest(args.config), 'decoder_sha256': digest(args.decoder),
                'smoke': args.smoke, 'scenario': args.scenario, 'host_program': args.host_program,
                'harness_sha256': {path.name:digest(path) for path in Path(__file__).parent.glob('*.py')},
                'fixture_overrides': {'sound_enabled':False,'disable_sound_env':True,'update_checks':False}}
    (args.out / 'provenance.json').write_text(json.dumps(metadata, indent=2) + '\n')
    default_sizes = ['120x40'] if args.scenario == 'repository' else ['80x24', '120x40', '200x50', '65x24', '64x24', '40x24']
    sizes = [tuple(map(int, size.split('x'))) for size in (args.size or default_sizes)]
    results = [one_size(args, size, binary) for size in sizes]
    raise SystemExit(3 if all(result.get('status') == 'core-policy-difference' for result in results)
                     else any(not result['passed'] for result in results))


if __name__ == '__main__':
    main()
