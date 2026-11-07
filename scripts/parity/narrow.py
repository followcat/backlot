"""Boundary/mobile script; same keys and authoritative fixture states on both clients."""
from fixtures import wait_program


def run(owners, programs, compare, size):
    reference, core = owners
    rp, cp = programs
    def send(data):
        for owner in owners:
            owner.ui.send(data)
    send(b'\x02w')
    compare('narrow-picker', picker=True, highlight='alpha', status={'alpha': 'unknown', 'beta': 'unknown'})
    send(b'\x1b[1;1B\x1b[1;1:3B')
    compare('narrow-next', picker=True, highlight='beta')
    send(b'\r')
    compare('narrow-beta', terminal='fixture beta')
    for number, (index, state) in enumerate([(0, 'working'), (1, 'working'),
                                          (0, 'blocked'), (0, 'idle'), (1, 'idle')]):
        p = rp[index]
        if not getattr(p, 'metadata_set', False):
            reference.cli('pane', 'report-metadata', p.terminal, '--source', 'fixture-meta',
                          '--agent', 'fixture', '--applies-to-source', 'fixture',
                          '--title', 'task-' + p.name, '--seq', '1')
            p.metadata_set = True
        reference.cli('pane', 'report-agent', p.terminal, '--source', 'fixture',
                      '--agent', 'fixture', '--state', state, '--seq', str(100 + number))
        cp[index].status(state)
        wait_program(core, cp[index], cp[index].serial)
        send(b'\x02w')
        shown = 'done' if state == 'idle' and index == 0 else state
        compare(f'narrow-status-{index}-{state}-picker', picker=True, highlight='beta',
                status={('alpha', 'beta')[index]: shown})
        send(b'\x1b[27u')
        compare(f'narrow-status-{index}-{state}-shell', terminal='fixture beta')
    # A mouse opens the same mobile switcher; at65 columns navigation is desktop.
    if size[0] <= 64:
        send(f'\x1b[<0;{size[0]-3};1M\x1b[<0;{size[0]-3};1m'.encode())
        compare('narrow-mouse-picker', picker=True, status={'alpha': 'done', 'beta': 'idle'})
        send(b'\x1b[27u')
        compare('narrow-mouse-close', terminal='fixture beta')
        send(f'\x1b[<0;{size[0]-3};1M\x1b[<0;{size[0]-3};1m'.encode())
        compare('narrow-menu-picker', picker=True, highlight='beta')
        send(b'\x1b[<0;5;20M\x1b[<0;5;20m')
        compare('narrow-menu-help', help_overlay=True)
        send(b'\x1b[27u')
        compare('narrow-menu-help-close', terminal='fixture beta')
        # The pane context menu also opens under the mobile width; Escape
        # dismisses it.
        send(b'\x1b[<2;8;20M')
        compare('narrow-pane-menu')
        send(b'\x1b[27u')
        compare('narrow-pane-menu-close', terminal='fixture beta')
