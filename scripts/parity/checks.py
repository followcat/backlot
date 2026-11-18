"""Checkpoint readiness and exact title comparison."""
import re


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
               'narrow-pane-menu', 'narrow-pane-menu-close',
               'navigator-open', 'navigator-blocked', 'navigator-working',
               'navigator-idle', 'navigator-done', 'navigator-all',
               'navigator-search', 'navigator-search-none', 'navigator-move',
               'navigator-end', 'navigator-close',
               'copy-open', 'copy-move', 'copy-word', 'copy-visual',
               'copy-yank', 'copy-search-open',
               'copy-search-submit', 'copy-search-next', 'copy-search-prev',
               'copy-search-cancel', 'copy-cancel', 'copy-history',
               'copy-bottom', 'copy-half-up', 'copy-page-up', 'copy-page-down',
               'copy-linewise', 'copy-yank-line', 'copy-prefix-arm',
               'copy-prefix-cancel', 'copy-quit', 'copy-reopen'}
    if name.startswith('status-') or name in changed:
        return previous is None or frame != previous
    return True


def last_osc52(output):
    """The most recent OSC 52 write in a captured PTY stream — both clients
    copy through the same `\\x1b]52;c;<base64>\\x07` host sequence."""
    marker = b'\x1b]52;'
    index = bytes(output).rfind(marker)
    if index < 0:
        return None
    end = bytes(output).find(b'\x07', index)
    if end < 0:
        return None
    return bytes(output[index:end + 1])


# --- Known differences -----------------------------------------------------
#
# backlot carries features the pinned herdr reference has no way to render.
# Each is matched by exact cell and exact content here — never by a count,
# a column tolerance or a skipped checkpoint — and every excuse names the
# reason below. docs/parity.md documents the user-facing contract; a
# difference outside these cells stays a bug.

BADGE_REASON = ('W1 chrome: backlot draws the running version badge (right-aligned in the '
                'expanded sidebar\'s header row); the reference draws nothing there')
PANE_ID_REASON = ('navigator footer identity: hc names panes `t<N>` where the reference core '
                  'names them `w<W>:p<P>` (docs/parity.md)')

VERSION_BADGE = re.compile(r'v[0-9]+\.[0-9]+\.[0-9]+')
PANE_ID = re.compile(r't[0-9]+$')
REFERENCE_PANE_ID = re.compile(r'w[0-9]+:p[0-9]+$')
# The navigator's selected-row detail: `workspace / tab-index / pane`,
# independent of which footer variant the overlay is showing.
NAVIGATOR_TAB = re.compile(r' / [0-9]+$')


def _row_text(frame, row):
    return ''.join(cell['text'] for cell in frame['cells'][row]['cells'])


def _badge_columns(candidate):
    """Exactly the columns of the candidate's running-version badge on row
    0, or none. The badge is one contiguous `vX.Y.Z` run between blanks in
    the sidebar header (`spaces … v0.1.12 │`), so a difference anywhere
    else on the row is never excused."""
    if not candidate['cells']:
        return set()
    text = _row_text(candidate, 0)
    match = VERSION_BADGE.search(text)
    if not match:
        return set()
    before = text[:match.start()]
    after = text[match.end():]
    if (before and before[-1] not in ' │') or not after or after[0] not in ' │':
        return set()
    return set(range(match.start(), match.end()))


def _navigator_pane_id_cells(reference, candidate):
    """Exactly the cells of the "Go to" overlay's selected-row pane id.

    The detail line's last ` / ` field is a pane id: only a candidate
    `t<N>` against a reference `w<W>:p<P>` is excused, and only while
    nothing else on that row differs. The two cores' own names for one
    pane are the whole difference (docs/parity.md)."""
    if len(reference['cells']) != len(candidate['cells']):
        return set()
    for row in range(len(candidate['cells'])):
        reference_text, candidate_text = _row_text(reference, row), _row_text(candidate, row)
        start = candidate_text.rfind(' / ')
        if start < 0 or start != reference_text.rfind(' / '):
            continue
        if not NAVIGATOR_TAB.search(candidate_text[:start]):
            continue
        # The overlay's right border follows the field, so stop at it rather
        # than at the row's padding.
        reference_id = reference_text[start + 3:].split('│', 1)[0].rstrip()
        candidate_id = candidate_text[start + 3:].split('│', 1)[0].rstrip()
        if not (REFERENCE_PANE_ID.match(reference_id) and PANE_ID.match(candidate_id)):
            continue
        span = set(range(start + 3, start + 3 + len(reference_id)))
        if any(a != b and column not in span for column, (a, b) in enumerate(
                zip(reference['cells'][row]['cells'], candidate['cells'][row]['cells']))):
            continue
        return {(row, column) for column in span}
    return set()


def known_differences(reference, candidate):
    """The documented differences this pair carries, as `reason -> cells`.

    Empty unless the exact known cells really differ; a caller reports the
    count beside the checkpoint, never instead of comparing the rest."""
    badge = _badge_columns(candidate)
    pane_id = _navigator_pane_id_cells(reference, candidate)
    found = {}
    for row_a, row_b in zip(reference['cells'], candidate['cells'], strict=True):
        for column, (a, b) in enumerate(zip(row_a['cells'], row_b['cells'], strict=True)):
            if a == b:
                continue
            if row_a['y'] == 0 and column in badge and a['text'] == ' ':
                found.setdefault(BADGE_REASON, []).append((row_a['y'], column))
            elif (row_a['y'], column) in pane_id:
                found.setdefault(PANE_ID_REASON, []).append((row_a['y'], column))
    return found


# The Alt+P pane command's shell sets a cursor shape; when its pane closes,
# the reference's core keeps reporting that shape for the visible pane,
# while backlot shows the visible pane's own shape. Only the exact pair of
# checkpoints after that command, and only a shape/blink difference at an
# identical position and visibility, is excused.
PANE_COMMAND_CURSOR_REASON = ('reference core reports the closed pane command shell\'s last '
                              'cursor shape for the visible pane; hc-tui keeps the visible '
                              "pane's own shape (docs/parity.md)")
PANE_COMMAND_CURSOR_CHECKPOINTS = ('pane-exit', 'pane-closed')


def known_cursor_difference(reference, candidate, checkpoint):
    """The documented cursor difference for `checkpoint`, or `None`.

    The position and visibility must match exactly; only the shape and its
    blink may differ, and only on the two checkpoints that follow the pane
    command."""
    if checkpoint not in PANE_COMMAND_CURSOR_CHECKPOINTS:
        return None
    a, b = reference['cursor'], candidate['cursor']
    if (a['position'], a['visible']) != (b['position'], b['visible']):
        return None
    if (a['shape'], a['blink']) == (b['shape'], b['blink']):
        return None
    return PANE_COMMAND_CURSOR_REASON


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
