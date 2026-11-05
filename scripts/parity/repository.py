"""Local-only Git fixture. Never fetches or inspects a user repository."""
import json
import subprocess
import time


CASES = ("standard", "missing-git", "nonexecutable-git", "detached",
         "promisor", "promisor-false", "promisor-empty")


def prepare(owner, case="standard"):
    assert case in CASES
    root = owner.cwd
    if not root.is_relative_to(owner.root) or any(root.iterdir()):
        raise ValueError('Git fixture requires the empty owned scratch cwd')
    env = dict(owner.env, GIT_CONFIG_NOSYSTEM='1',
               GIT_AUTHOR_DATE='2020-01-01T00:00:00Z',
               GIT_COMMITTER_DATE='2020-01-01T00:00:00Z')

    def git(*args):
        return subprocess.check_output(
            ['/usr/bin/git', '-c', 'user.name=alice', '-c', 'user.email=alice', *args],
            cwd=root, env=env, stderr=subprocess.PIPE, text=True).strip()

    git('init', '--initial-branch', 'fixture-main')
    (root / 'src').mkdir()
    (root / 'src/example.txt').write_text('common\n')
    git('add', '.')
    git('commit', '-m', 'fixture common')
    git('checkout', '-b', 'fixture-upstream')
    (root / 'upstream.txt').write_text('upstream\n')
    git('add', '.')
    git('commit', '-m', 'fixture upstream')
    git('checkout', 'fixture-main')
    for number in [1, 2]:
        (root / f'local-{number}.txt').write_text('local\n')
        git('add', '.')
        git('commit', '-m', f'fixture local {number}')
    # This tracks another LOCAL branch, with remote='.'. No fetch or URL.
    git('branch', '--set-upstream-to=fixture-upstream', 'fixture-main')
    linked = owner.root / 'linked-checkout'
    git('worktree', 'add', '-b', 'fixture-linked', str(linked), 'fixture-main')
    assert git('rev-list', '--left-right', '--count', 'HEAD...@{upstream}').split() == ['2', '1']
    assert git('status', '--porcelain') == ''
    if case == 'detached':
        git('checkout', '--detach')
    elif case.startswith('promisor'):
        value = {'promisor': 'true', 'promisor-false': 'false', 'promisor-empty': ''}[case]
        git('config', 'remote.fixture.promisor', value)
    elif case in ('missing-git', 'nonexecutable-git'):
        # Restrict only the owned server environment; setup still used absolute Git.
        owner.env['PATH'] = str(owner.root / 'bin')
        if case == 'nonexecutable-git':
            executable = owner.root / 'bin/git'
            executable.write_text('#!/bin/sh\nexit 1\n')
            executable.chmod(0o600)
    return {'alpha': root / 'src', 'beta': linked}


def verify_facts(owner, programs, case):
    deadline = time.monotonic() + 15
    while True:
        records = [owner.rpc('terminal.get', {'terminal': p.terminal}) for p in programs]
        if all(t.get('repository', {}).get('state') == 'ready' for t in records):
            break
        assert time.monotonic() < deadline, 'C3 repository observations did not settle'
        time.sleep(.03)
    facts = [record['repository'] for record in records]
    root, linked = [fact['checkout'] for fact in facts]
    assert root['repo_root'] == str(owner.cwd)
    assert linked['repo_root'] == str(owner.root / 'linked-checkout')
    assert root['repository_key'] == linked['repository_key']
    assert root['checkout_key'] != linked['checkout_key']
    assert facts[0]['core_id'] == facts[1]['core_id']
    assert not root['is_linked_worktree'] and linked['is_linked_worktree']
    assert not root['is_bare'] and not linked['is_bare']
    assert facts[0]['auto_label'] == 'acme-shop'
    assert facts[1]['auto_label'] == 'linked-checkout'
    assert facts[0]['branch'] == (None if case == 'detached' else 'fixture-main')
    assert facts[1]['branch'] == 'fixture-linked'
    assert facts[0]['ahead_behind'] == ({'ahead': 2, 'behind': 1} if case == 'standard' else None)
    assert facts[1]['ahead_behind'] is None
    for record in records:
        activity = record['activity']
        assert activity['core_id'] == record['repository']['core_id']
        assert activity['epoch'] and isinstance(activity['scope'], int)
        assert '' in activity['records']
    (owner.out / 'repository-facts.json').write_text(json.dumps(records, indent=2) + '\n')


def run(compare, case, owners, size, config):
    policy = ([{'fact': 'ahead-behind', 'core': None,
                'reason': 'Core omits counts whenever promisor configuration is present.'}]
              if case.startswith('promisor') else [])
    if size[0] <= 65:
        from layout import binding
        compare('repository-baseline', text_contains=['fixture alpha'])
        for owner in owners:
            owner.ui.send(binding(config, 'workspace_picker'))
        compare('repository-picker', picker=True, highlight='acme-shop',
                text_contains=['acme-shop', 'linked-checkout'], core_policy_differences=policy)
        for owner in owners:
            owner.ui.send(b'\r')
        compare('repository-return', terminal='fixture alpha')
        return
    required = ['acme-shop', 'linked-checkout']
    if case != 'detached':
        required += ['fixture-main', 'fixture-linked']
    else:
        required += ['fixture-linked']
    reference = required.copy()
    candidate = required.copy()
    if case == 'standard':
        reference += ['↑2', '↓1']
        candidate += ['↑2', '↓1']
    elif case.startswith('promisor'):
        reference += ['↑2', '↓1']
    compare('repository-root', reference={'text_contains': reference},
            core={'text_contains': candidate}, core_policy_differences=policy)
