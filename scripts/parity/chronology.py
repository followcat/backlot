"""Owned multi-tab chronology and local acknowledgement against pinned runtimes."""
import json
from pathlib import Path
import shlex
import time
import tomllib

from checks import STATUS, text
from configuration import update
from fixtures import Program, wait_program
from layout import binding, chord, grids
from pty_host import Pty
from sessions import Core, Reference

NAMES = ('alpha', 'beta', 'gamma')


def launch(owner, binary, size):
    owner.ui = Pty([str(binary), '--session', owner.session], owner.env,
                   owner.cwd, *size, cell_px=(10, 20))
    owner.ui.keyboard_flags_reply = 7
    owner.ui.sgr_pixel_mouse_reply = 2
    owner.initial_size = size
    owner.resizes = []
    # Wait before sending indexed-tab keys; startup cooked-mode echo would
    # contaminate the restored outer screen and is a harness input race.
    owner.ui.until(lambda data: b'\x1b[?1049h' in data and b'\x1b[?2004h' in data)


def agent_rows(frame, mobile):
    """Read only the semantic agent section; full frames are still compared."""
    rows = frame['cells']
    begin = next((i for i, row in enumerate(rows)
                  if 'agents' in ''.join(c['text'] for c in row['cells'])), None)
    if begin is None:
        return []
    found = []
    previous_state = None
    for row in rows[begin + 1:]:
        line = ''.join(c['text'] for c in row['cells'])
        if not mobile:
            line = line.split('│', 1)[0]
        first = next((c for c in row['cells'][:len(line)] if c['text'].strip()), None)
        current_state = next((state for state, (symbol, color) in STATUS.items()
                              if first and first['text'] == symbol
                              and first['resolved_fg'] == color), None)
        if mobile and 'spaces' in line:
            break
        for name in NAMES:
            needle = name
            if needle in line and name not in [n for n, _ in found]:
                # Mobile details and desktop titles each carry a status token.
                found.append((name, current_state or previous_state))
        previous_state = current_state
    return found


def run(args, size, binary):
    from run import checkpoint, differences, detach
    out = args.out / f'{size[0]}x{size[1]}'
    out.mkdir(parents=True, exist_ok=False)
    core = reference = None
    result = {'geometry': size, 'scope': 'M3b', 'checkpoints': [], 'cleanup': {}, 'passed': False}
    programs = {}
    config = None
    phase = 0
    try:
        core = Core(args.hc, args.config, out)
        reference = Reference(binary, args.config, out)
        owners = (reference, core)
        for owner in owners:
            if args.host_program:
                owner.env['TERM_PROGRAM'] = args.host_program
            path = Path(owner.env['XDG_CONFIG_HOME']) / 'herdr/config.toml'
            path.write_text(update(path.read_text(), {'ui': {'agent_panel_sort': 'priority'}}))
            config = tomllib.loads(path.read_text())
        core.start()
        programs['core'] = []
        tabs = []
        for name in NAMES:
            p = Program(core, name)
            p.terminal = core.rpc('terminal.run', {'argv': p.argv, 'cwd': str(core.cwd),
                'labels': {'herdr.space': 'work', 'herdr.tab': name},
                'no_parent': True, 'no_notify': True})['address']
            wait_program(core, p)
            programs['core'].append(p)
            tabs.append({'id': name, 'name': name, 'root': {'type': 'block',
                         'content': {'terminal': p.terminal}}})
        foreign = {'spaces': [{'id': 'work', 'name': 'work', 'folder': str(core.cwd), 'tabs': tabs}]}
        core.rpc('doc.set', {'name': 'herdr-app', 'version': 0, 'value': foreign})
        core.rpc('doc.set', {'name': 'hc-tui', 'version': 0,
                            'value': {'schema': 1, 'focus': programs['core'][2].terminal}})
        launch(reference, binary, size)
        reference.ready()
        ws = reference.cli('workspace', 'list')['workspaces'][0]['workspace_id']
        reference.cli('workspace', 'rename', ws, 'work')
        first = reference.cli('tab', 'list')['tabs'][0]['tab_id']
        reference.cli('tab', 'rename', first, 'alpha')
        initial = reference.cli('pane', 'list')['panes'][0]['pane_id']
        programs['reference'] = []
        for index, name in enumerate(NAMES):
            p = Program(reference, name)
            if index:
                created = reference.cli('tab', 'create', '--workspace', ws, '--cwd',
                                        str(reference.cwd), '--label', name, '--no-focus')
                p.terminal = created['root_pane']['pane_id']
            else:
                p.terminal = initial
            reference.cli('pane', 'run', p.terminal, 'exec ' + shlex.join(p.argv))
            wait_program(reference, p)
            reference.cli('pane', 'report-metadata', p.terminal, '--source', 'fixture-meta',
                          '--agent', 'fixture', '--applies-to-source', 'fixture',
                          '--title', 'task-' + name, '--seq', '1')
            programs['reference'].append(p)
        launch(core, args.hc_tui, size)
        serial = 0

        def send(data):
            for owner in owners:
                owner.ui.send(data)

        def select(index):
            keys = config.get('keys', {})
            switch = keys.get('switch_tab', 'prefix+1..9')
            assert switch == 'prefix+1..9', 'chronology requires audited indexed tab bindings'
            prefix = keys.get('prefix', 'ctrl+b')
            if isinstance(prefix, list):
                prefix = prefix[0]
            send(chord(prefix) + str(index + 1).encode())

        def fact(p):
            return core.rpc('terminal.get', {'terminal': p.terminal})

        def report(index, state, record=''):
            nonlocal serial
            serial += 1
            source = programs['reference'][index]
            # `done` has no reference-side word: both idle/done spellings are
            # the same endpoint — the pinned core keeps them as aliases.
            ref_state = {'done': 'idle'}.get(state, state)
            reference.cli('pane', 'report-agent', source.terminal, '--source', 'fixture',
                          '--agent', 'fixture', '--state', ref_state, '--seq', str(serial))
            p = programs['core'][index]
            p.report(state, record)
            wait_program(core, p, p.serial)
            deadline = time.monotonic() + 8
            while True:
                value = fact(p)
                target = value.get('status') if not record else value.get('records', {}).get(record)
                if target and target.get('state') == state:
                    break
                for owner in owners:
                    owner.ui.read(.03)
                assert time.monotonic() < deadline, 'authoritative status readiness'
            return value['activity']

        def compare(name, visible, expected, order=None, picker=False):
            nonlocal phase
            phase += 1
            marker = f'phase {phase:02d}'
            if not picker:
                a = grids(reference, [programs['reference'][visible]])
                b = grids(core, [programs['core'][visible]])
                deadline = time.monotonic() + 8
                while a != b:
                    a = grids(reference, [programs['reference'][visible]])
                    b = grids(core, [programs['core'][visible]])
                    assert time.monotonic() < deadline, 'chronology visible geometry'
                for owner in owners:
                    p = programs[owner.kind][visible]
                    p.send((f'\x1b[0m\x1b[2J\x1b[3J\x1b[Hfixture {p.name}\r\n{marker}'
                            '\x1b[2 q\x1b[18;8H').encode())
                    wait_program(owner, p, p.serial)
            frames = []
            observed = {}
            for owner in owners:
                deadline = time.monotonic() + 8
                while True:
                    frame = checkpoint(owner, name, size, args.decoder)
                    rendered = text(frame)
                    rows = agent_rows(frame, size[0] <= 64)
                    semantic = not picker or (len(rows) == 3 and dict(rows) == expected
                                and (order is None or [n for n, _ in rows] == order))
                    if not picker:
                        semantic = marker in rendered and 'fixture ' + NAMES[visible] in rendered
                    if semantic and 'Action failed' not in rendered and owner.capture_quiet:
                        break
                    assert time.monotonic() < deadline, f'{owner.kind} {name} readiness: {rows}'
                frames.append(frame)
                observed[owner.kind] = rows
            diff = differences(*frames)
            (out / (name + '-diff.json')).write_text(json.dumps(diff, indent=2) + '\n')
            result['checkpoints'].append({'name': name, 'different_cells': len(diff),
                'status': 'fail' if diff else 'pass', 'agent_rows': observed})
            print(json.dumps({'geometry': size, 'checkpoint': name, 'differences': len(diff)}), flush=True)
            for owner in owners:
                assert all(not p.received() for p in programs[owner.kind]), 'UI input leaked to child'

        def inspect(name, visible, states, order=None):
            compare(name, visible, states)
            send(binding(config, 'workspace_picker'))
            compare(name + '-picker', visible, states, order, picker=True)
            send(b'\x1b[27u')

        select(2)
        for i in range(3):
            activity = report(i, 'idle')
            assert activity['records']['']['completion_ordinal'] is None
        idle = dict.fromkeys(NAMES, 'idle')
        inspect('initial-readiness', 2, idle)
        report(0, 'working')
        report(1, 'working')
        inspect('background-working', 2, {'alpha': 'working', 'beta': 'working', 'gamma': 'idle'},
                ['beta', 'alpha', 'gamma'])
        alpha = report(0, 'idle')['records']['']['completion_ordinal']
        beta = report(1, 'idle')['records']['']['completion_ordinal']
        assert alpha is not None and beta > alpha
        result['completion_order'] = {'alpha': alpha, 'beta': beta}
        both = {'alpha': 'done', 'beta': 'done', 'gamma': 'idle'}
        inspect('ordered-completions', 2, both, ['beta', 'alpha', 'gamma'])
        before = fact(programs['core'][0])['activity']
        report(0, 'idle')
        assert fact(programs['core'][0])['activity']['records'] == before['records']
        inspect('repeated-word-stable', 2, both, ['beta', 'alpha', 'gamma'])
        select(1)
        inspect('visit-acknowledges-beta', 1, {'alpha': 'done', 'beta': 'idle', 'gamma': 'idle'},
                ['alpha', 'beta', 'gamma'])
        report(2, 'working')
        report(2, 'idle')
        pending = {'alpha': 'done', 'beta': 'idle', 'gamma': 'done'}
        inspect('other-tab-attention', 1, pending, ['gamma', 'alpha', 'beta'])
        old = fact(programs['core'][0])['activity']['epoch']
        core.rpc('server.restart', {})
        core.child.wait(timeout=10)
        core.start()
        restart_done = time.monotonic()
        deadline = time.monotonic() + 10
        while fact(programs['core'][0])['activity']['epoch'] == old:
            assert time.monotonic() < deadline, 'actual scratch registry epoch must change'
            time.sleep(.03)
        retained = [fact(p)['activity']['records']['']['completion_ordinal']
                    for p in programs['core']]
        assert retained[:2] == [alpha, beta] and retained[2] is not None, \
            'planned core handover must retain the genuine unseen markers'
        result['restart'] = {'actual_core_epoch_changed': True,
            'retained_completion_markers': retained,
            'reference': 'fresh client baseline on retained owned server; herdr live-handoff resets chronology to null',
            'candidate': 'same client reconnects to actual restarted registry'}
        detach(reference, keys=binding(config, 'detach'))
        reference.ui.close()
        launch(reference, binary, size)
        reference.ready()
        select(1)
        # Transient reconnect overlay: the retained screen stays rendered
        # while the fresh attach opens, so a capture can land mid-Opening.
        # Measure how long it takes to clear before the baseline checkpoint.
        needles = ('Reconnecting', 'fresh terminal paint',
                   'terminal state before forwarding')
        frame = checkpoint(core, 'epoch-baseline-clear', size, args.decoder)
        appeared = any(n in text(frame) for n in needles)
        deadline = time.monotonic() + 12
        while appeared and any(n in text(frame) for n in needles):
            frame = checkpoint(core, 'epoch-baseline-clear', size, args.decoder)
            assert time.monotonic() < deadline, 'reconnect overlay did not clear'
        result['reconnect_overlay'] = {
            'appeared': appeared,
            'cleared_seconds': round(time.monotonic() - restart_done, 2)}
        inspect('epoch-baseline', 1, idle)
        report(0, 'working')
        report(0, 'idle')
        inspect('post-epoch-completion', 1, {'alpha': 'done', 'beta': 'idle', 'gamma': 'idle'},
                ['alpha', 'gamma', 'beta'])
        select(0)
        inspect('final-visit-acknowledgement', 0, idle, ['alpha', 'gamma', 'beta'])

        # Readiness aliases (.9 core): adjacent idle/done words retain the
        # completion ordinal — an off-tab agent keeps Done until visited,
        # for the root record and for a named record, in both orders.
        base = dict.fromkeys(NAMES, 'idle')
        select(2)  # gamma tab visible; beta stays off-tab
        report(1, 'working')
        inspect('alias-root-working', 2, dict(base, beta='working'))
        report(1, 'done')
        inspect('alias-root-done', 2, dict(base, beta='done'))
        report(1, 'idle')
        inspect('alias-root-idle-keeps-done', 2, dict(base, beta='done'))
        select(1)
        inspect('alias-root-visit-ack', 1, base)
        select(2)
        report(1, 'working')
        report(1, 'idle')
        inspect('alias-root2-completion', 2, dict(base, beta='done'))
        report(1, 'done')
        inspect('alias-root2-done-keeps-done', 2, dict(base, beta='done'))
        select(1)
        inspect('alias-root2-visit-ack', 1, base)

        record = 'build/test'
        select(2)
        report(1, 'working', record)
        inspect('alias-named-working', 2, dict(base, beta='working'))
        report(1, 'done', record)
        inspect('alias-named-done', 2, dict(base, beta='done'))
        report(1, 'idle', record)
        inspect('alias-named-idle-keeps-done', 2, dict(base, beta='done'))
        select(1)
        inspect('alias-named-visit-ack', 1, base)
        select(2)
        report(1, 'working', record)
        report(1, 'idle', record)
        inspect('alias-named2-completion', 2, dict(base, beta='done'))
        report(1, 'done', record)
        inspect('alias-named2-done-keeps-done', 2, dict(base, beta='done'))
        select(1)
        inspect('alias-named2-visit-ack', 1, base)

        assert core.rpc('doc.get', {'name': 'herdr-app'})['value'] == foreign
        restored = [detach(owner, args.decoder, binding(config, 'detach')) for owner in owners]
        diff = differences(*restored)
        (out / 'restored-diff.json').write_text(json.dumps(diff, indent=2) + '\n')
        result['checkpoints'].append({'name': 'restored', 'different_cells': len(diff),
                                      'status': 'fail' if diff else 'pass'})
        result['passed'] = all(c['status'] == 'pass' for c in result['checkpoints'])
    except BaseException as error:
        result.update(passed=False, error=repr(error))
        raise
    finally:
        failures = []
        for owner in (core, reference):
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
            result.update(passed=False, cleanup_errors=[repr(e) for e in failures])
        (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
        if failures:
            raise RuntimeError('chronology owned cleanup failed') from failures[0]
    return result
