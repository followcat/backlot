"""Checkpoint readiness and the one explicit core hostname adaptation."""
import socket


def text(frame):
    return '\n'.join(''.join(cell['text'] for cell in row['cells']) for row in frame['cells'])


def ready(frame, name, previous, program_input):
    rendered = text(frame)
    if name == 'baseline':
        return 'fixture alpha' in rendered
    if name == 'prefix':
        return 'PREFIX' in rendered
    if name == 'prefix-cancel':
        return 'PREFIX' not in rendered
    if name in ('help', 'help-filter', 'help-back', 'help-page'):
        if 'keybinds' not in rendered.lower():
            return False
    if name == 'switch' and 'fixture beta' not in rendered:
        return False
    if name == 'changed-pixel-pitch' and not frame['host_modes']['pixel_mouse']:
        return False
    if name == 'restored-pixel-pitch' and frame['host_modes']['pixel_mouse']:
        return False
    if name in ('mouse', 'tracked-wheel', 'changed-pitch-mouse'):
        summary = 'INPUT' + program_input.hex()[-42:][:32]
        if summary not in ''.join(rendered.split()):
            return False
    changed = {'help', 'help-filter', 'help-back', 'help-close', 'navigate',
               'navigate-next', 'switch', 'sidebar-drag', 'sidebar-collapse',
               'sidebar-expand', 'history-bottom', 'history-wheel', 'history-typing-reset'}
    if name.startswith('status-') or name in changed:
        return previous is None or frame != previous
    return True


def title_difference(reference, candidate, default_template):
    if reference == candidate:
        return None
    a, b = socket.gethostname() + ': ', 'host-a: '
    if (default_template and reference.startswith(a) and candidate.startswith(b)
            and reference[len(a):] == candidate[len(b):]):
        return {'adaptation': 'core-name-instead-of-hostname',
                'reference_title': reference, 'candidate_title': candidate}
    return {'reference_title': reference, 'candidate_title': candidate}


def verify_restored(frame):
    modes = frame['host_modes']
    assert not any(modes[name] for name in ['alternate', 'paste', 'focus', 'all_mouse',
                                          'sgr_mouse', 'pixel_mouse', 'scheme'])
    assert modes['kitty'] == 0 and modes['line_wrap']
    assert frame['cursor']['visible']
