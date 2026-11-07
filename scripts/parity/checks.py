"""Checkpoint readiness and exact title comparison."""


def text(frame):
    return '\n'.join(''.join(cell['text'] for cell in row['cells']) for row in frame['cells'])


def ready(frame, name, previous, program_input, expected=None):
    expected = expected or {}
    rendered = text(frame)
    if not all(value in rendered for value in expected.get('text_contains', [])):
        return False
    if expected.get('terminal') and (expected['terminal'] not in rendered
            or 'NAVIGATE' in rendered or rendered.startswith(' switch')
            or frame['host_modes']['kitty'] != 7):
        return False
    if expected.get('highlight') and not highlighted(frame, expected['highlight']):
        return False
    if expected.get('picker'):
        if frame['host_modes']['kitty'] != 31:
            return False
        if frame['cols'] <= 64:
            if not rendered.startswith(' switch') or not all(label in rendered for label in ['menu', 'settings', 'keybinds', 'reload config', 'detach']):
                return False
        elif 'NAVIGATE' not in rendered:
            return False
    if expected.get('help_overlay') and not ('keybinds' in rendered and 'esc close' in rendered):
        return False
    if 'status' in expected and not status_ready(frame, expected['status']):
        return False
    if name == 'baseline':
        return 'fixture alpha' in rendered
    if name == 'prefix':
        return 'PREFIX' in rendered
    if name == 'prefix-cancel':
        return 'PREFIX' not in rendered
    if name in ('help', 'help-filter', 'help-back', 'help-page'):
        if 'keybinds' not in rendered.lower():
            return False
    if name == 'help-page':
        if previous is None or frame['cells'] == previous['cells'] or 'session navigator' not in rendered:
            return False
    if name == 'live-cursor-and-mouse-mode':
        if frame['cursor']['shape'] != 'Bar' or not frame['cursor']['blink']:
            return False
        if not frame['host_modes']['all_mouse'] or not frame['host_modes']['sgr_mouse']:
            return False
    if name.startswith('resize-'):
        if (frame['cols'], frame['rows']) != tuple(expected['geometry']):
            return False
        if expected['grid_marker'] not in rendered:
            return False
    if name.startswith('status-'):
        if not status_ready(frame, expected['status']):
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
               'sidebar-expand', 'history-bottom', 'history-wheel', 'history-typing-reset',
               'menu-workspace', 'menu-highlight', 'menu-highlight-up',
               'menu-workspace-rename', 'menu-rename-cancel', 'menu-workspace-again',
               'menu-close-highlight', 'menu-workspace-close', 'menu-close-cancel',
               'menu-navigate-exit', 'menu-tab', 'menu-tab-newtab',
               'menu-newtab-cancel', 'menu-pane', 'menu-pane-escape',
               'narrow-pane-menu', 'narrow-pane-menu-close'}
    if name.startswith('status-') or name in changed:
        return previous is None or frame != previous
    return True


def title_difference(reference, candidate):
    if reference == candidate:
        return None
    return {'reference_title': reference, 'candidate_title': candidate}


def verify_restored(frame):
    modes = frame['host_modes']
    assert not any(modes[name] for name in ['alternate', 'paste', 'focus', 'all_mouse',
                                          'sgr_mouse', 'pixel_mouse', 'scheme'])
    assert not any(modes[name] for name in ['x10_mouse', 'normal_mouse', 'button_mouse',
                                          'utf8_mouse', 'urxvt_mouse', 'modify_other_keys'])
    assert modes['kitty'] == 0 and modes['line_wrap']
    assert frame['cursor']['visible']
    assert frame['cursor']['shape'] == 'Block' and frame['cursor']['blink'] is False


# Derived from herdr src/app/state.rs and src/client/shell.rs (Apache-2.0).
# Independent pinned-source default-dark fixture oracle. Unknown configured
# palettes must supply new source evidence, never candidate-generated colors.
STATUS = {
    'working': ('●', [249, 226, 175]),
    'blocked': ('●', [243, 139, 168]),
    'done': ('●', [148, 226, 213]),
    'idle': ('○', [166, 227, 161]),
    'unknown': ('·', [108, 112, 134]),
}


def status_ready(frame, states):
    for label, state in states.items():
        symbol, color = STATUS[state]
        found = False
        for row in frame['cells']:
            cells = row['cells']
            line = ''.join(cell['text'] for cell in cells)
            # Default desktop workspace rows and mobile workspace headings use
            # the same source status symbol/color; labels identify the target.
            if label in line and any(cell['text'] == symbol and cell['resolved_fg'] == color
                                     for cell in cells[:max(3, line.find(label))]):
                found = True
        if not found:
            return False
    return True


def highlighted(frame, label):
    # Source default-dark Navigate selection (surface0), distinct from the
    # active workspace's base highlight. Neutral fixture labels are ASCII.
    for row in frame['cells']:
        cells = row['cells']
        for index in range(len(cells) - len(label) + 1):
            span = cells[index:index + len(label)]
            if (''.join(cell['text'] for cell in span) == label
                    and all(cell['resolved_bg'] == [49, 50, 68] for cell in span)):
                return True
    return False
