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
    # The pane context menu also opens at mobile and boundary widths; Escape
    # dismisses it. (40,10) stays inside the pane at 40/64/65 columns.
    send(b'\x1b[<2;40;10M')
    compare('narrow-pane-menu')
    send(b'\x1b[27u')
    compare('narrow-pane-menu-close', terminal='fixture beta')
    # The Go to navigator (prefix+g) at boundary/mobile widths: the centered
    # popup renders over the mobile chrome; Esc backs out of search and then
    # closes the overlay.
    send(b'\x02g')
    compare('navigator-open', text_contains=['Go to'])
    send(b'i')
    compare('navigator-idle', text_contains=['/ idle'])
    send(b'/task')
    compare('navigator-search', text_contains=['/ task'])
    send(b'\x15zzz')
    compare('navigator-search-none', text_contains=['No matching'])
    send(b'\x1b[27ua' + b'k' * 2)
    compare('navigator-move')
    send(b'\x1b[F')
    compare('navigator-end')
    send(b'\x1b[27u')
    compare('navigator-close')
