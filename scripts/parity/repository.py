"""Local-only Git fixture. Never fetches or inspects a user repository."""
import subprocess


def prepare(owner):
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
    return {'alpha': root / 'src', 'beta': linked}


PENDING = [
    {'fact': 'repository-branch', 'source': 'fixture-main / fixture-linked', 'candidate': 'absent'},
    {'fact': 'ahead-behind', 'source': '↑2 ↓1 against local fixture-upstream', 'candidate': 'absent'},
    {'fact': 'repo-root-label', 'source': 'acme-shop for unlabelled src terminal', 'candidate': 'src'},
    {'fact': 'worktree-identity', 'source': 'linked checkout shares the common Git directory', 'candidate': 'unavailable'},
]


def run(compare):
    # These are requirements on the independently running source, not masks.
    # Every diff is retained. Pending fact scenarios are NEVER counted as passes.
    compare('repository-root',
            reference={'text_contains': ['acme-shop', 'fixture-main', '↑2', '↓1', 'fixture-linked']},
            core={'text_contains': ['src', 'linked-checkout']}, pending_core_facts=PENDING)


def verify_title(reference, candidate):
    import socket
    assert reference == socket.gethostname() + ': acme-shop'
    assert candidate == 'host-a: src'
