"""Reload only owned config copies; compare the paired Settings surface."""
import json
from pathlib import Path
import datetime
import math
import tomllib


def quote(value):
    return json.dumps(value, ensure_ascii=False).replace('\x7f', '\\u007f')


def encode(value):
    if isinstance(value, dict):
        return '{' + ', '.join(quote(key) + ' = ' + encode(item)
                               for key, item in sorted(value.items())) + '}'
    if isinstance(value, list):
        return '[' + ', '.join(encode(item) for item in value) + ']'
    if isinstance(value, (datetime.datetime, datetime.date, datetime.time)):
        return value.isoformat()
    if isinstance(value, float) and not math.isfinite(value):
        return 'nan' if math.isnan(value) else ('inf' if value > 0 else '-inf')
    if isinstance(value, (str, bool, int, float)):
        return quote(value)
    raise ValueError('unsupported scratch TOML value')


def emit_key(key):
    # Bare-key form for `[ui]`/`name` lookups in write_edit; only unusual
    # key bytes are quoted.
    import re
    return key if re.fullmatch(r'[A-Za-z0-9_-]+', key) else quote(key)


def emit_into(prefix, table, lines):
    # Canonical section form, not "key" = {inline}: write_edit upserts by
    # `[section]` header and bare `key =` prefix, so the Settings save path
    # only exercises its in-place branches when the scratch file looks like
    # a real config.
    scalars = {key: value for key, value in table.items()
               if not isinstance(value, dict)
               and not (isinstance(value, list)
                        and any(isinstance(item, dict) for item in value))}
    for key, value in sorted(scalars.items()):
        lines.append(emit_key(key) + ' = ' + encode(value))
    for key, value in sorted(table.items()):
        if isinstance(value, dict):
            lines += ['', '[' + prefix + emit_key(key) + ']']
            emit_into(prefix + emit_key(key) + '.', value, lines)
        elif isinstance(value, list) and any(isinstance(item, dict) for item in value):
            for item in value:
                lines += ['', '[[' + prefix + emit_key(key) + ']]']
                emit_into(prefix + emit_key(key) + '.', item, lines)


def update(contents, changes):
    parsed = tomllib.loads(contents)
    for section, values in changes.items():
        parsed.setdefault(section, {}).update(values)
    # Only private fixture copies are rewritten. Parsed values are preserved
    # regardless of comments, quoted headers or multiline arrays.
    lines = []
    emit_into('', parsed, lines)
    contents = '\n'.join(lines) + '\n'
    checked = tomllib.loads(contents)
    for section, values in changes.items():
        assert all(checked[section][key] == value for key, value in values.items())
    assert checked['ui']['sound']['enabled'] is False
    assert checked['update']['version_check'] is False and checked['update']['manifest_check'] is False
    return contents


def run(owners, config, compare, size, decoder, out, result):
    from layout import binding
    reference, core = owners
    def send(data):
        for owner in owners:
            owner.ui.send(data)
    def edit(changes):
        parsed = []
        copies = []
        for owner in owners:
            path = Path(owner.env['XDG_CONFIG_HOME']) / 'herdr/config.toml'
            assert path.is_relative_to(owner.root)
            contents = update(path.read_text(), changes)
            temporary = path.with_suffix('.next')
            temporary.write_text(contents)
            temporary.replace(path)
            parsed.append(tomllib.loads(contents))
            copies.append(contents)
        assert copies[0] == copies[1], 'both clients must reload the same config'
        return parsed[0]
    def click(frames, label):
        rows = [(row['y'], ''.join(c['text'] for c in row['cells'])) for row in frames[0]['cells']]
        row, line = next((y, line) for y, line in rows if label in line)
        column = line.index(label) + 2
        send(f'\x1b[<0;{column};{row+1}M\x1b[<0;{column};{row+1}m'.encode())

    before = compare('reload-before')[0]
    next_config = edit({'keys': {'prefix': 'ctrl+t', 'help': 'prefix+shift+k'},
                        'theme': {'name': 'nord', 'auto_switch': False},
                        'ui': {'sidebar_width': 30}})
    send(binding(config, 'reload_config'))
    config.clear()
    config.update(next_config)
    changed = compare('reload-keyboard')[0]
    stable = zip(before['cells'][0]['cells'][:8], changed['cells'][0]['cells'][:8])
    assert any(a['text'] == b['text'] and a['text'].strip() and
               (a['resolved_fg'], a['resolved_bg']) != (b['resolved_fg'], b['resolved_bg'])
               for a, b in stable), 'source theme must actually change visible header colors'
    send(binding(config, 'help'))
    compare('reload-new-binding', required=('keybinds',), paint=False, geometry=False)
    send(b'\x1b[27u')
    compare('reload-help-close')

    frames = None
    if size[0] <= 64:
        send(binding(config, 'workspace_picker'))
        frames = compare('reload-menu', required=('reload config',), paint=False, geometry=False)
    next_config = edit({'keys': {'prefix': 'ctrl+y', 'help': 'prefix+shift+u'},
                        'theme': {'name': 'dracula'}, 'ui': {'sidebar_width': 32}})
    if frames:
        click(frames, 'reload config')
    else:
        send(binding(config, 'reload_config'))
    config.clear()
    config.update(next_config)
    compare('reload-menu-result' if frames else 'reload-second')
    send(binding(config, 'help'))
    compare('reload-second-binding', required=('keybinds',), paint=False, geometry=False)
    send(b'\x1b[27u')
    compare('reload-second-help-close')
    result['reload'] = {'passed': True, 'keyboard': True, 'mobile_menu': frames is not None,
                        'new_prefixes': ['ctrl+t', 'ctrl+y'], 'themes': ['nord', 'dracula']}

    def settings(entry, frames=None):
        if frames:
            click(frames, 'settings')
        else:
            send(binding(config, 'settings'))
        # Theme opens first; `j` previews the next theme live without a
        # write. The reference and hc-tui restore that preview on Esc.
        compare('settings-' + entry + '-open', required=('settings', 'catppuccin'),
                paint=False, geometry=False)
        send(b'j')
        compare('settings-' + entry + '-theme-preview', required=('catppuccin-latte',),
                paint=False, geometry=False)
        sections = [('indicators', 'agent status indicators'),
                    ('sound', 'sound alerts'),
                    ('toast', 'notification popups'),
                    ('integrations', 'agent integrations')]
        for name, marker in sections:
            send(b'l')
            compare('settings-' + entry + '-' + name, required=(marker,),
                    paint=False, geometry=False)
        send(b'h' * 3)  # Back to Indicators; `j` picks distinct symbols.
        send(b'j')
        # Enter writes `status_indicators = "symbols"` through write_edit
        # and reloads on both clients; the overlay stays open.
        send(b'\r')
        compare('settings-' + entry + '-applied', required=('distinct symbols',),
                paint=False, geometry=False)
        send(b'\x1b[27u')
        compare('settings-' + entry + '-close')
        result.setdefault('settings', []).append({'entry': entry, 'status': 'native'})
    settings('binding')
    if size[0] <= 64:
        send(binding(config, 'workspace_picker'))
        frames = compare('settings-menu', required=('settings',), paint=False, geometry=False)
        settings('menu', frames)
