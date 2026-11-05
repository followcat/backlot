#!/usr/bin/env python3
"""Verify passive desktop transport recovery against an owned phone-size viewer."""
import argparse
import json
from pathlib import Path
import time
import tomllib

from checks import text
from fixtures import Program, wait_program
from layout import binding
from pty_host import Pty
from run import checkpoint, detach, REPO
from sessions import Core, digest
from transport_proxy import Proxy


def run(args):
    args.out.mkdir(parents=True, exist_ok=False)
    core = Core(args.hc, args.config, args.out)
    proxy = phone = None
    result = {'passed': False, 'checks': [], 'cleanup': {},
              'candidate_sha256': digest(args.hc_tui), 'core_sha256': digest(args.hc),
              'config_sha256': digest(args.config), 'decoder_sha256': digest(args.decoder)}
    try:
        core.start()
        program = Program(core, 'alpha')
        program.terminal = core.rpc('terminal.run', {'argv': program.argv,
            'cwd': str(core.cwd), 'labels': {'herdr.space': 'work', 'herdr.tab': 'main'},
            'no_parent': True, 'no_notify': True})['address']
        wait_program(core, program)
        path = core.root / 'r/client-proxy.sock'
        proxy = Proxy(path, core.endpoint)
        env = dict(core.env, HC_SOCKET=str(path))
        core.ui = Pty([str(args.hc_tui), '--session', core.session], env,
                      core.cwd, 120, 40, cell_px=(10, 20))
        core.ui.keyboard_flags_reply = 7
        core.ui.sgr_pixel_mouse_reply = 2
        core.initial_size = (120, 40)
        core.resizes = []
        deadline = time.monotonic() + 8
        while 'fixture alpha' not in text(checkpoint(core, 'desktop', (120, 40), args.decoder)):
            assert time.monotonic() < deadline, 'desktop readiness'

        def grid():
            terminal = core.rpc('terminal.get', {'terminal': program.terminal})
            return [terminal['cols'], terminal['rows']]

        def wait_grid(expected):
            deadline = time.monotonic() + 8
            while True:
                core.ui.read(.02)
                if phone:
                    phone.read(.02)
                actual = json.loads(Path(str(program.log) + '.grid').read_text())
                if grid() == expected and actual == expected:
                    return
                assert time.monotonic() < deadline, f'expected owned PTY grid {expected}'

        desktop = grid()
        assert desktop != [40, 20]
        phone = Pty([str(args.hc), 'attach', program.terminal], core.env,
                    core.cwd, 40, 20, cell_px=(10, 20))
        wait_grid([40, 20])
        result['checks'].append('phone attach owns 40x20')
        initial = [r for r in proxy.snapshot() if not r['params'].get('observe', False)]
        assert len(initial) >= 2 and all(not r['params'].get('resumed', False) for r in initial)
        result['cut_connections'] = proxy.cut()
        assert result['cut_connections'] >= 3
        deadline = time.monotonic() + 10
        while True:
            core.ui.read(.03)
            phone.read(.03)
            recovered = [r for r in proxy.snapshot() if r['epoch'] == 1 and r['success'] and r['live']]
            active = [r for r in recovered if not r['params'].get('observe', False)]
            if len(active) >= 2 and any(r['params'].get('observe', False) for r in recovered):
                break
            assert time.monotonic() < deadline, 'desktop attach streams did not recover'
        assert all(r['params'].get('resumed', False) for r in active)
        program.send(b'\x1b[8;2HRECOVERED STREAM\x1b[K')
        wait_program(core, program, program.serial)
        deadline = time.monotonic() + 8
        while True:
            frame = checkpoint(core, 'recovered', (120, 40), args.decoder)
            observers = [r for r in proxy.snapshot() if r['epoch'] == 1 and r['success']
                         and r['live'] and r['params'].get('observe', False)]
            if observers and all(r['fresh_marker'] for r in observers) and 'RECOVERED STREAM' in text(frame):
                break
            assert time.monotonic() < deadline, 'fresh recovered observer output missing'
        confirmed = [r for r in proxy.snapshot() if r['epoch'] == 1 and r['success'] and r['live']]
        assert len([r for r in confirmed if not r['params'].get('observe', False)]) >= 2
        result['recovered_attach_confirmations'] = confirmed
        until = time.monotonic() + 2
        while time.monotonic() < until:
            core.ui.read(.02)
            phone.read(.02)
            assert grid() == [40, 20], 'transport recovery stole the phone size'
        result['checks'].append('recovered desktop streams stay passive; phone keeps 40x20')
        phone.send(b'\x1c')  # hc attach's documented Ctrl+backslash detach.
        deadline = time.monotonic() + 5
        while phone.child.poll() is None:
            phone.read(.02)
            assert time.monotonic() < deadline
        assert phone.child.returncode == 0 and phone.terminal_restored()
        phone.close()
        phone = None
        wait_grid(desktop)
        result['checks'].append('phone detach restores passive desktop size automatically')
        program.send(b'\x1b[=31u')
        wait_program(core, program, program.serial)
        core.ui.read(.2)
        press, release = b'\x1b[120;5u', b'\x1b[120;5:3u'
        core.ui.send(press)
        deadline = time.monotonic() + 8
        while program.received() != press:
            core.ui.read(.02)
            assert time.monotonic() < deadline, 'desktop physical press not delivered'
        phone = Pty([str(args.hc), 'attach', program.terminal], core.env,
                    core.cwd, 40, 20, cell_px=(10, 20))
        wait_grid([40, 20])
        before_release = {r['index']: r['quiet_release_hex'] for r in proxy.snapshot()}
        core.ui.send(release)
        deadline = time.monotonic() + 8
        while program.received() != press + release:
            core.ui.read(.02)
            phone.read(.02)
            assert time.monotonic() < deadline, 'desktop quiet release not delivered'
        quiet = []
        for record in proxy.snapshot():
            before = before_release.get(record['index'], '')
            assert record['quiet_release_hex'].startswith(before)
            interval = record['quiet_release_hex'][len(before):]
            if interval:
                assert record['live'] and record['quiet_human_release']
                quiet.append(bytes.fromhex(interval))
        assert b''.join(quiet) == release, 'kind7 did not carry this exact human release'
        until = time.monotonic() + 2
        while time.monotonic() < until:
            core.ui.read(.02)
            phone.read(.02)
            assert grid() == [40, 20], 'human release stole phone size'
        result['checks'].append('confirmed kind7 human release reaches child without claiming phone size')
        core.resizes.append((len(core.ui.output), 121, 40))
        core.ui.resize(121, 40)
        wait_grid([desktop[0] + 1, desktop[1]])
        result['checks'].append('real desktop allocation change reclaims size')
        phone.send(b'\x1c')
        deadline = time.monotonic() + 5
        while phone.child.poll() is None:
            phone.read(.02)
            assert time.monotonic() < deadline
        assert phone.child.returncode == 0 and phone.terminal_restored()
        phone.close()
        phone = None
        assert program.received() == press + release, 'UI/phone controls leaked into program input'
        detach(core, args.decoder, binding(tomllib.loads(args.config.read_text()), 'detach'))
        result['termios_restored'] = True
        result['attach_requests'] = proxy.snapshot()
        result['passed'] = True
    except BaseException as error:
        result['passed'] = False
        result['error'] = repr(error)
        raise
    finally:
        errors = []
        resources = [('core', core), ('proxy', proxy)]
        if phone:
            resources.insert(0, ('phone', phone))
        for name, resource in resources:
            if resource is None:
                continue
            try:
                resource.close()
                result['cleanup'][name] = 'pass'
            except BaseException as error:
                result['cleanup'][name] = repr(error)
                errors.append(error)
        if errors:
            result['passed'] = False
        (args.out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
        if errors:
            raise RuntimeError('owned recovery resources did not clean up') from errors[0]
    print(json.dumps(result), flush=True)


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    for name in ['hc', 'hc-tui', 'config', 'decoder', 'out']:
        p.add_argument('--' + name, type=Path, required=True)
    args = p.parse_args()
    for name in ['hc', 'hc_tui', 'config', 'decoder']:
        setattr(args, name, getattr(args, name).resolve(strict=True))
    args.out = args.out.resolve()
    assert not args.out.is_relative_to(REPO), 'captures must be outside the repository'
    run(args)
